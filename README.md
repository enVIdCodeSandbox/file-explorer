# FAST File Explorer

A fast, cross-platform file explorer: the Windows 11 look with Windows 10's
density, built on a Rust engine with a TypeScript interface (Tauri 2).

This branch is a ground-up rewrite of the Electron version. It is an early
build: the engine and the main interface work, and the list under
[Status](#status) says exactly what is and is not done.

## Run it

Prerequisites: [Rust](https://rustup.rs), Node.js 20+, and the
[Tauri system dependencies](https://tauri.app/start/prerequisites/) for your OS
(WebView2 on Windows, Xcode command line tools on macOS, WebKitGTK on Linux).

```sh
npm install
npm run app:dev      # run with hot reload
npm run app:build    # build installers for the current OS into target/release/bundle
```

Checks:

```sh
cargo test -p fast-core   # engine tests
npm test                  # frontend unit tests
npm run typecheck
cargo run --release -p fast-core --example bench -- ~   # engine speed on your machine
```

## How it is put together

```
crates/fast-core   Rust engine. No UI or Tauri dependency.
src-tauri          Thin Tauri shell: exposes the engine as commands and events.
src                React + TypeScript interface.
docs               Architecture, theming guide, roadmap.
```

All disk work happens on Rust threads; the interface only draws what is on
screen. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Status

Run-tested on Linux. The engine also type-checks for Windows and macOS, but
nothing has been run on those systems yet, so expect platform bugs there first.

| Area | State | Notes |
|---|---|---|
| Folder browsing | Working | Virtualized details and thumbnail views, sorting, multi-select, keyboard, type-ahead, inline rename |
| Tabs | Working | Duplicate, groups with names and colours, collapse, restored at launch. No drag-to-reorder yet |
| Right-click menu | Working | Flat Windows 10-style list; every entry can be hidden in settings. No "Open with" or shell extensions yet |
| File index and search | Working | Parallel scan, saved to disk, loaded at launch; searches the index first, then scans the folder. Filters: `ext:` `type:` `size:` and wildcards |
| Listing cache and preloading | Working | Cached folders show instantly and refresh in the background; folders are preloaded on hover and from learned habits |
| Transfers | Working | Strategy chosen per job; pause, resume, cancel; conflict handling; time estimate from a fitted per-file + per-byte model |
| Remote protocols | Not started | SFTP, FTP, WebDAV and rsync from the Electron version are not ported. Mounted shares (SMB, NFS) work as ordinary folders |
| This PC | Working | Cached list paints first; bars or rings; used or free. Drive health (SMART) not started |
| Details pane | Working | Timestamps, size on disk, permissions, MIME type, image dimensions, SHA-256, folder statistics. No EXIF or media metadata yet |
| Disk usage | Working | Treemap with zoom, largest-files list with delete |
| Pins, Home | Working | Default and user pins; nothing is added automatically. Recents live on Home |
| Settings | Working | Searchable, sectioned, applied immediately |
| Themes | Working | Custom CSS over documented variables. See [docs/THEMING.md](docs/THEMING.md) |
| Resource limits | Working | Threads, caches and background work adapt to CPU, RAM and battery, or follow manual limits |
| Terminal | Written, untested | Here, home, drive root, as administrator |
| Elevation | Written, untested | A failed transfer can be retried through the OS prompt; the app itself never runs elevated |
| Installers | Configured | Linux `.deb`, `.rpm` and `.AppImage` built and verified locally. The GitHub Action (`.github/workflows/build.yml`) builds Windows (x64, ARM64), macOS (universal) and Linux (x64, ARM64) and drafts a release when you push a `v*` tag. Windows and macOS installers have not been run |
| Drag and drop, system clipboard, undo, archives | Not started | Copy and paste currently works inside the app only |
| Live folder watching | Not started | Folders refresh when revisited, after the app's own operations, and on F5 |
| Android, iOS | Not started | The engine has mobile fallbacks; no mobile shell yet |

The full list of what is planned, and the known limits, is in
[docs/ROADMAP.md](docs/ROADMAP.md).

## Keyboard

| Keys | Action |
|---|---|
| Ctrl+T / Ctrl+W / Ctrl+Tab | New tab / close tab / next tab |
| Alt+Left / Alt+Right / Alt+Up | Back / forward / up |
| Ctrl+L | Edit the address |
| Ctrl+F | Search |
| Ctrl+C / Ctrl+X / Ctrl+V | Copy / cut / paste |
| F2 / Delete / Shift+Delete | Rename / delete / delete permanently |
| Ctrl+Shift+N | New folder |
| F5 | Refresh |
| Ctrl+, | Settings |

## Licence

GPL-3.0, as in the original repository.


### Known limits
- Verified here: Rust tests, TypeScript build, Linux runtime (screenshots). Compiles for Windows and macOS targets but has **not been run** on them: UAC prompt, native Properties, Windows shell right-click entries, frameless resize.
- Share is copy path/name only; no media-server entry; macOS gets no extra shell-menu entries.
