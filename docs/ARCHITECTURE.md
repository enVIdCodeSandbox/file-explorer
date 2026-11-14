# Architecture

## Layers

```
┌──────────────────────────────────────────────────────────────┐
│ src/            React + TypeScript interface (in the webview) │
│                 draws only what is on screen                  │
├──────────────── invoke() commands ▲ ▼ events ────────────────┤
│ src-tauri/      Tauri shell: state, caches, command handlers  │
├──────────────────────────────────────────────────────────────┤
│ crates/fast-core  Engine: listing, index, search, transfers,  │
│                   analysis, platform layer. No UI dependency  │
└──────────────────────────────────────────────────────────────┘
```

`fast-core` knows nothing about Tauri. If a platform ever needs a native
interface, only the top two layers are replaced.

## Rules that keep it fast

1. **No disk access on the UI thread.** Every command in
   `src-tauri/src/commands.rs` that touches the filesystem runs through
   `spawn_blocking`. Long work (indexing, transfers) has its own threads and
   reports through events.
2. **Answer from memory first, verify after.** A cached folder listing is
   returned immediately; a background re-read emits `dir-updated` only if the
   folder changed. The drive list from the last run is shown at launch while
   the live query runs.
3. **Render only what is visible.** Lists are virtualized. Thumbnails are
   requested only for rows that have stayed on screen for a moment, so fast
   scrolling queues nothing.
4. **Bounded everything.** Thread counts, index size, listing cache size and
   thumbnail concurrency all come from one `Budget` (see below).

## Engine modules (`crates/fast-core/src`)

| Module | Purpose |
|---|---|
| `entry.rs` | Directory listing (parallel metadata for large folders), create, rename, delete |
| `index.rs` | Compact file index: 40-byte nodes in depth-first order plus name arenas. Parallel scan, save, load |
| `search.rs` | Query parser and search. One SIMD substring pass over the name arena, then filters and ranking |
| `transfer/strategy.rs` | Picks a transfer plan from job shape and device types |
| `transfer/engine.rs` | Copy/move engine: worker lanes, pause, cancel, conflicts, progress |
| `transfer/eta.rs` | Time estimate model |
| `details.rs` | File properties, folder statistics, SHA-256 |
| `treemap.rs` | Size tree and largest files for the disk usage view |
| `predict.rs` | Learns which folders are opened next |
| `resources.rs` | Turns CPU, RAM and battery state into a `Budget` |
| `devices.rs` | Phones, cameras and other devices that are not drives: Linux sysfs + GVFS (`FAST_EXPLORER_USB_ROOT` / `FAST_EXPLORER_GVFS_DIR` override the locations for tests), macOS `system_profiler`, Windows shell "This PC" items that are not file-system folders. `path` when the app can browse it, `open` for the system's own tool |
| `drives.rs` | Volumes and media types; `signature()` is a cheap fingerprint of what is mounted (mountinfo, /Volumes, drive letters plus media presence) that the shell's `drive-watch` thread polls to emit `drives-changed`; on Windows every drive letter is listed natively (empty drives as offline removable) |
| `thumbs.rs` | Thumbnails with a disk cache |
| `platform.rs` | Everything OS-specific: known folders, trash (and restoring from it), terminals, elevation |
| `watch.rs` | Live folder watching: the `notify` crate on the open folders only (not recursive), events merged until a folder is quiet for 200 ms (at most every 2 s), at most 48 folders. The shell invalidates the listing cache and emits `fs-changed` |
| `archive.rs` | Zip create, and zip/tar/tar.gz extract, in pure Rust. `safe_relative` refuses names that leave the target folder; links are skipped; cancel flag removes a half-written zip |
| `browse.rs` | Archives opened like folders: `archive://<file>!/<inside>` locations, a cached index of what is inside (zip, tar, tar.gz/bz2/xz, 7z, single gz/bz2/xz in Rust; rar, cab, iso and more through 7-Zip or bsdtar when installed), folder listings built from it, and unpacking of chosen items (`extract_items_with` asks a callback about each file that already exists: skip, replace through a temp file, or keep both; the shell job asks the person through the same card the transfers use). Read-only; same name safety rules as `archive.rs` |
| `edit.rs` | Changing archives: add, move, rename, delete, new folder. Zip, tar and tar.gz are rewritten to a temporary file beside the archive and swapped in when complete (cancel or failure leaves the original); 7z goes through 7-Zip; other kinds say they cannot be changed. Names that exist are never overwritten |
| `settings.rs`, `session.rs` | Persisted settings; tabs, pins, recents |

### The index

Nodes are stored parent-before-child, so:

* a subtree is a contiguous range of ids;
* folder sizes for the whole tree are one reverse pass (`subtree_sizes`);
* every name sits in one contiguous lower-case buffer, so a search is a single
  `memmem` scan followed by a binary search to map hits back to nodes.

Measured on a 2-core cloud VM (`cargo run --release -p fast-core --example bench`):
100,000 files scanned in about 0.13 s, searched in 3 to 5 ms, held in about
7 MB. Run the benchmark on your own machine; disks vary far more than CPUs.

The index is built by an unprivileged parallel directory walk and saved to the
cache directory. At launch the saved index is loaded first (search works
immediately), then rescanned in the background if the budget allows.

### Transfers

`strategy::choose` looks at the job and the devices:

| Situation | Plan |
|---|---|
| Move within one volume | Rename. No data is copied |
| Copy within one volume | Try a copy-on-write clone first (APFS, Btrfs, XFS, ReFS) |
| Mostly large files | Few streams, large buffers, chunked so pause and cancel respond mid-file |
| Many small files | Many workers; each file is one OS fast-path call |
| Mixed | A wide lane for small files and a narrow lane for large ones |
| Hard disk involved | At most 2 small-file workers and 1 large stream |
| Network involved | Up to 16 small-file workers, 2 large streams, 1 MB buffers |

### Time estimates

Each file is modelled as `time = a + b × size`. `a` (seconds per file) and
`b` (seconds per byte) are fitted while the transfer runs, using exponentially
weighted least squares so the fit follows changing conditions.

```
remaining = (files_left × a + bytes_left × b) ÷ active_workers
```

The share of the remaining time that comes from `a` decides the readout:
files/s when per-file work dominates, MB/s when data dominates, both in
between. Fitted values are saved per pair of volumes so the next transfer
between the same drives starts with a good estimate.

### Resource budget

`resources::compute(settings, system_snapshot)` returns thread counts, cache
sizes and whether background indexing and preloading may run. In automatic
mode it picks a tier (full, balanced, power saver) from CPU count, RAM and
battery state; in manual mode it uses the user's limits. The shell
re-evaluates it every 30 seconds and when settings change.

## Shell (`src-tauri/src`)

| File | Purpose |
|---|---|
| `lib.rs` | App setup, background threads, command registration, `--job` helper mode |
| `state.rs` | Shared state and the listing cache |
| `commands.rs` | One function per frontend call |

Events sent to the interface: `transfer-progress`, `dir-updated`,
`fs-changed`, `index-status`, `budget-changed`.

### Elevation

The app never runs as administrator. When a transfer fails with permission
errors, the interface offers "Retry as administrator". That launches the same
executable with `--job <file>` through the OS prompt (UAC, polkit, or the
macOS authorization dialog). The helper runs that one transfer and exits.

## Interface (`src`)

| File | Purpose |
|---|---|
| `api.ts` | Typed wrappers for every command and event |
| `store.ts` | App state (zustand): tabs, groups, pins, settings, transfers |
| `components/Chrome.tsx` | Tab strip, toolbar, sidebar |
| `components/FileView.tsx` | Folder view, selection, keyboard, right-click menu |
| `components/Views.tsx` | Home, This PC, search results, disk usage |
| `components/DetailsPane.tsx` | Properties |
| `components/Overlays.tsx` | Transfer cards, dialogs, toasts |
| `components/SettingsView.tsx`, `settings/schema.ts` | Settings screen generated from a schema |
| `styles/tokens.css`, `styles/app.css` | Design variables and layout |

Locations are strings: a filesystem path, or `fast://home`, `fast://thispc`,
`fast://search?q=…&scope=…`, `fast://analyze?path=…`.

## Adding things

* **A setting:** add the field in `crates/fast-core/src/settings.rs`, the type
  in `src/api.ts`, and one entry in `src/settings/schema.ts`.
* **A command:** write it in `commands.rs`, register it in `lib.rs`, add a
  wrapper in `api.ts`.
* **A right-click entry:** add it in `FileView.tsx` (`buildMenu`) and to
  `MENU_CATALOG` in `ContextMenu.tsx` so it can be hidden in settings.
* **OS-specific behavior:** put it in `platform.rs` behind `#[cfg(...)]`, with
  a version for every desktop OS and a fallback for mobile.

## Detached windows and reattaching

Settings, the advanced Transfers view and dragged-out tabs can each live in their own native window.

- `open_detached` (src-tauri/src/commands.rs) opens a window labelled `settings` or `transfers` with `window.__VIEW__` set, and broadcasts a `detached` event when it opens or closes. The frontend renders only that panel in such a window (`App.tsx`).
- Every detached window has a Reattach button. It calls the `reattach` command, which tells the main window what came back (`reattach` event), brings it forward and closes the detached window. Tabs return to the index they were taken from (`__INDEX__`).
- The in-window Settings panel is draggable; its offset lives in the store (`settingsPos`), so after reattaching it appears exactly where it was left.
- Detached state is not remembered by default. With Settings > General > "Remember detached windows" on, the main window reopens them at launch.
- Settings saved in any window are broadcast as `settings-changed`, so every window follows.

## Size units

`files.sizeUnits` is `decimal` (default: KB/MB/GB, 1000), `binary` (KiB/MiB/GiB, 1024) or `windows` (1024 labelled KB/MB/GB). Everything goes through `formatSize` in `src/util/format.ts`; hardware memory uses `formatMemory`, which rounds to the number on the box.

## About page and drive health

`crates/fast-core/src/sysreport.rs` gathers the report off the UI thread. Windows reads PowerShell CIM and storage reliability counters; Linux reads /sys and `smartctl -j`; macOS uses `system_profiler`, `diskutil` and `smartctl` when installed. Expected drive life is a projection only when the drive reports a wear counter; otherwise it is labelled a rule of thumb. Developer details are in `src/about.ts`.

## Thumbnails

`Thumb.tsx` queues requests in a small scheduler: newest first, at most `thumbThreads + 1` in flight, starts spaced 16 ms apart, and requests for tiles that scrolled away are dropped before they start. The backend caps decoder memory and decodes on a bounded blocking pool, so listing and rendering never wait on image work.

## Temporary download links (`fast-core/src/share.rs`)

"Share → Temporary download link…" starts a small HTTP server for one file. The address holds a random 128-bit token (`getrandom`), so it cannot be guessed. The server only serves that file (no path from the request is ever used, so there is no traversal), ends on expiry or after the allowed number of complete downloads, and allows at most 8 simultaneous connections. Interrupted downloads do not use up the allowance. It is plain HTTP, reachable on the local network (or only this computer, by choice), and only while the app runs. With "internet" selected the server listens on loopback only and a `cloudflared tunnel --url http://127.0.0.1:<port>` child process (a Cloudflare quick tunnel) provides the public https address, parsed from its output (`find_tunnel_url`). The tunnel is killed when the share is stopped, expires or is used up, and by `stop_all` when the app exits. Folders are refused; archive them first. Windows may ask the firewall for permission the first time.

## Undo, redo and batch rename

Undo lives in the interface (`store.ts`): `UndoAction` is `rename`, `renameMany`, `create`, `move`, `copy` or `trash`, kept in two stacks (`undo`, `redo`; any new action empties `redo`). `reverse(action, "undo" | "redo")` runs the opposite or the same operation, so each kind is defined once. Create and copy are undone by moving the result to the Trash (`trash_paths`), which is what makes redo possible (`restore_from_trash`, Linux and Windows). A delete registers its undo when `delete-progress` reports it finished without errors.

Batch rename keeps its rules in `src/util/batchRename.ts`: `planRenames` turns the selection and rules into new names and problems (empty, invalid, too long, duplicate, exists), and `orderRenames` splits them into rounds so a shifted series or a swap goes through temporary names and never overwrites. The dialog only displays `planRenames`, so what the preview shows is what happens. `applyRenames` carries out the rounds and records one `renameMany` undo.

## Live folder watching

The interface reports its open folders with `watch_dirs` whenever the set of tab locations changes (150 ms debounce; app pages such as Home and Search are skipped). The shell keeps one set per window and watches the union. Events from reading files are ignored so the app's own listing cannot cause a loop. Setting `general.liveRefresh` turns it off, and the listing then refreshes only after the app's own operations and on F5.

## Servers (`fast-core/src/remote`)

A location on a server is a URL, `sftp://anna@host:2222/home/anna`, and travels through the app like any path (`isRemote` in `util/path.ts`, `Location` in Rust). The path part is verbatim, not percent-encoded. Passwords never appear in it and are never written to disk.

* `mod.rs`: `Server`, `Location`, the `Session` trait (`list`, `download`, `upload`, `mkdir`, `remove`, `rename`, `exists`), and protocol-independent tree helpers (`download_tree_as`, `upload_tree_as`, `remove_tree`, `measure_tree`, `free_name`) that every protocol shares. `Control` carries the cancel flag and a byte counter.
* `ftp.rs` (suppaftp; MLSD with a LIST fallback), `sftp.rs` (russh + russh-sftp on a one-thread tokio runtime, trust on first use with a `known_hosts` file, password or key login), `webdav.rs` (PROPFIND/MKCOL/MOVE/DELETE; listings parsed with quick-xml). All pure Rust: no C toolchain, no OpenSSL.
* `http.rs` is a small HTTP/1.1 client written for WebDAV, because the common clients refuse the WebDAV methods. Short requests reuse one connection; transfers stream on their own. TLS is rustls with the system's trusted certificates.
* Shell side, `src-tauri/src/remote.rs`: one pool per server key (`scheme://user@host:port`), at most four idle connections, a dead connection is replaced and the call retried once. Jobs (`remote_copy_start`, `remote_remove_start`) run on their own thread and report on the same `delete-progress` card the delete and archive jobs use, with byte counts. Uploads and downloads stream straight between disk and socket; a copy between two servers passes through a temporary folder.
* `list_dir` and `get_details` dispatch on the address, so the existing folder view, breadcrumbs, tabs and drag and drop work unchanged; the interface hides actions that only make sense locally (`localOnly` in `FileView.tsx`).
* Tests: unit tests in each module, and `crates/fast-core/tests/remote_servers.rs`, which runs the same checks against real FTP, SFTP and WebDAV servers. `scripts/test-servers.sh` starts pyftpdlib, asyncssh and wsgidav locally and runs them; without servers the tests are skipped.
* Not done: FTPS (encrypted FTP), SFTP agent login, resuming interrupted transfers, preserving modification times on download.

## Tabs and windows

Tabs open at `tabs.width` and shrink together down to `tabs.minWidth` before the bar scrolls. A window made by dragging a tab out is an ordinary new window. Dropping a tab on another window's tab bar calls `drop_tab`, which hit-tests every app window by screen position, emits `tab-drop` to the one under the pointer (it picks the slot), and closes the source window if it had no other tab.

## Disk usage map

`layoutNested` draws folders as frames holding their own contents (WinDirStat-style grouping); files take their type's color, the same color as in the type list. Frames precede their contents in the tile list, so the last tile under a point is the deepest.

## Mapped drives when run as administrator

An elevated process runs in a different logon session and does not see the user's mapped drive letters. The drive list also reads `HKCU\Network` and mounts those shares by UNC path, keeping the letter in the name.

## Presets, profiles and locks

* `src/settings/presets.ts` holds the twelve performance presets, the three interface presets and the profile helpers. A preset is a plain object of "setting key -> value", applied with the same `setSetting` the settings screen uses, so presets can never drift from real settings (a test checks every key exists).
* Profiles are stored in `settings.profiles` (Rust `Profile`), each with a snapshot of every setting. `store.updateSettings` saves changes into the active profile of the user's own.
* `crates/fast-core/src/locks.rs` keeps Argon2id hashes in `locks.json`; unlocked state lives only in memory with a timestamp. `commands::guard` is called by every command that reads or changes a path (list, details, thumbnails, open, search results, transfers, deletes, rename, create) and returns `locked:<path>`, which the interface turns into a passcode card.
* `system_report` stores the last hardware reading in `hardware-report.json` (`cache.hardware`, `cache.hardwareMaxAgeHours`). `disk_smart_admin` runs `smartctl` through `platform::run_elevated`, redirecting its output to a private scratch file because the elevated helper cannot return output.
* `resource_usage` reads the app's own process with `sysinfo`; `compact_memory` drops cached listings and calls `malloc_trim` on glibc.
* `tab_hover` / `drop_tab` find the other window whose tab strip is under the pointer (`tab_bar_under`); the target shows an outline through the `tab-hover` event and shifts its tabs.
