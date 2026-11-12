# Roadmap and known limits

## What has been verified, and how

| Check | Result |
|---|---|
| Engine unit tests (`cargo test -p fast-core`) | All pass on Linux (`cargo test --workspace`) |
| Engine type-check for `x86_64-pc-windows-msvc` and `aarch64-apple-darwin` | Passes (compile check only, not run) |
| Frontend type-check, unit tests, production build | Pass |
| App launched on Linux (WebKitGTK) | Browsing, thumbnails, tabs and groups, search, disk usage, This PC, settings, right-click menu, copy and paste of a 303-file folder all exercised |
| Linux `.deb` bundle | Built |
| Windows, macOS | **Never run.** Expect issues in `platform.rs` (terminal and elevation commands) and in webview differences first |
| Transfer speed against Explorer, Finder or `cp` | **Not measured.** The strategy logic is reasoned and unit-tested, not benchmarked |

## Next, in order

1. **First run on Windows and macOS.** Fix what breaks; confirm terminal
   launch, trash, elevation prompt, and drive listing including mapped
   network drives.
2. **Live folder watching** (`notify` crate) for open tabs, so changes made by
   other programs appear without F5.
3. **Run-test the system clipboard, drag-out and drops from other programs**
   on Windows and macOS (written with `clipboard-rs`, `drag` and Tauri's drop
   events, see INTERFACE.md; none of them has run on those systems).
4. **Incremental index updates.** Today the index is rebuilt by a full rescan.
   Windows: optional elevated helper reading the NTFS MFT and USN journal for
   near-instant builds and live updates. macOS: FSEvents. Linux: fanotify.
5. **Remote protocols.** Port SFTP, FTP and WebDAV from the Electron
   version behind a `Source`/`Sink` trait in the transfer engine. (rsync, rclone,
   SMB-tuned and NFS-tuned copies already exist as copy methods; see TRANSFERS.md.)
6. **Transfer benchmarks** against the native tools on SSD, HDD and SMB, then
   tune thresholds and worker counts from data. The method chooser already
   learns per machine; benchmarks would tune its starting point. Add unbuffered I/O for very
   large files on Windows if the numbers support it.
7. **Undo**, **archives** (zip browse/extract/create), **"Open with"**,
   batch rename.
8. **Tabs:** drag to reorder within the strip.
9. **Details:** EXIF, audio/video metadata, owner names, Windows ACL summary.
10. **Drive health** (SMART). Needs elevated access on every OS; show it only
    when available.
11. **Mobile shell.** Android via the Storage Access Framework; iOS via the
    document picker and the app's own container. Whole-device indexing is not
    possible on either; the mobile build will be a reduced-feature app.
12. **Signing and auto-update** for installers.

## Known limits in this build

* Cut and paste into another program arrives as a copy (operating systems do not let the sender mark "cut").
* SMB-tuned copies on Windows need Robocopy `/COMPRESS` (Windows 11 / Server 2022 or later); otherwise Robocopy or the built-in engine is used.
* rclone is used only when installed; the app does not bundle it.
* The temporary download link works only while the app runs. On the network it is plain HTTP; over the internet it goes through a Cloudflare quick tunnel (needs `cloudflared` installed; not run-tested here because it needs the real service). No other tunnel providers, passwords or uploads to a hosting service yet.
* A folder open in a tab does not update when another program changes it,
  until it is revisited or refreshed.
* Folder listings are sent to the interface whole. Folders with several
  hundred thousand entries will list slowly; paged delivery is planned.
* When "ask" is the conflict setting, the question covers top-level items.
  Files that collide inside merged folders are kept under a new name.
* The index is capped by the cache budget (Settings → Performance). When the
  cap is hit, search reports partial results.
* Network shares are never indexed automatically; searching one scans it live.
* Elevated retry re-runs the transfer with existing files skipped and reports
  only success or failure, not per-file progress.
* Thumbnails cover JPEG, PNG, GIF, WebP and BMP.
* Media type detection (SSD or hard disk) comes from the OS and is often
  "unknown" or wrong inside virtual machines.
