# Roadmap and known limits

## What has been verified, and how

| Check | Result |
|---|---|
| Engine unit tests (`cargo test -p fast-core`) | 47 pass on Linux |
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
3. **System clipboard and drag and drop**, so files move between this app and
   others. Needs per-OS code (CF_HDROP, NSPasteboard, `text/uri-list`).
4. **Incremental index updates.** Today the index is rebuilt by a full rescan.
   Windows: optional elevated helper reading the NTFS MFT and USN journal for
   near-instant builds and live updates. macOS: FSEvents. Linux: fanotify.
5. **Remote protocols.** Port SFTP, FTP, WebDAV and rsync from the Electron
   version behind a `Source`/`Sink` trait in the transfer engine.
6. **Transfer benchmarks** against the native tools on SSD, HDD and SMB, then
   tune thresholds and worker counts from data. Add unbuffered I/O for very
   large files on Windows if the numbers support it.
7. **Undo**, **archives** (zip browse/extract/create), **"Open with"**,
   batch rename.
8. **Tabs:** drag to reorder, drag out to a new window.
9. **Details:** EXIF, audio/video metadata, owner names, Windows ACL summary.
10. **Drive health** (SMART). Needs elevated access on every OS; show it only
    when available.
11. **Mobile shell.** Android via the Storage Access Framework; iOS via the
    document picker and the app's own container. Whole-device indexing is not
    possible on either; the mobile build will be a reduced-feature app.
12. **Signing and auto-update** for installers.

## Known limits in this build

* Copy, cut and paste work inside the app only.
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
