# Roadmap and known limits

## What has been verified, and how

| Check | Result |
|---|---|
| Engine unit tests (`cargo test -p fast-core`) | All pass on Linux (`cargo test --workspace`) |
| Engine type-check for `x86_64-pc-windows-msvc` and `aarch64-apple-darwin` | Passed before the remote protocols were added (compile check only, not run). **Not re-checked since:** `ring` (TLS and SSH) compiles C code, which needs that OS's toolchain; this Linux machine has none, so the check fails at `ring`, not in our code. The CI matrix builds all three, so the first CI run answers it |
| Frontend type-check, unit tests, production build | Pass |
| App launched on Linux (WebKitGTK) | Browsing, thumbnails, tabs and groups, search, disk usage, This PC, settings, right-click menu, copy and paste of a 303-file folder all exercised |
| Linux `.deb` bundle | Built |
| Windows, macOS | **Never run.** Expect issues in `platform.rs` (terminal and elevation commands) and in webview differences first |
| Transfer speed against `cp` and `rsync` | Measured on one Linux VM (2 vCPU, virtual disk, files mostly cached): the engine matches or beats `cp` on small and mixed folders and trails it by about 20% on four large files. See TRANSFERS.md, Benchmarks. **Not measured:** Explorer, Finder, SSD vs HDD, SMB, Windows, macOS |

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
5. **Remote protocols:** FTP, SFTP and WebDAV exist (browse, copy, move,
   delete, rename, open). Remaining: FTPS, SSH agent login, resume, keep
   modification times, and run-testing on Windows and macOS. (rsync, rclone,
   SMB-tuned and NFS-tuned copies already exist as copy methods; see TRANSFERS.md.)
6. **Transfer benchmarks** on real hardware: the benchmark program exists
   (`transfer_bench`) and has run on a Linux VM, where it found and fixed two
   engine problems. Remaining: SSD, HDD and SMB numbers, Windows and macOS, and
   Explorer/Finder as the comparison. The method chooser already learns per
   machine; benchmarks would tune its starting point. Add unbuffered I/O for
   very large files on Windows if the numbers support it.
7. **Archives:** (done: browse zip, tar, 7z, gz/bz2/xz like folders, rar and
   others through 7-Zip) still to do: add files to an archive in place, passwords, nested archives, rar without 7-Zip. (Create zip,
   extract zip/tar/tar.gz, undo, redo, batch rename, "Open with" and live
   folder watching exist.)
8. **Tabs:** done. Tabs reorder by dragging, and a group moves as one block when its chip is dragged.
9. **Details:** EXIF, audio/video metadata, owner names, Windows ACL summary.
10. **Drive health** (SMART). Needs elevated access on every OS; show it only
    when available.
11. **Mobile, next steps.** The Android and iOS builds exist (docs/MOBILE.md).
    Still to do: opening and sharing files through the system on iOS (needs a
    small Swift plugin), picking folders outside the app's own on iOS, and
    a Play Store bundle (.aab) for Android.
12. **Signing and auto-update** for installers. Signing is wired into the release workflow and switches on when the certificate secrets exist (see RELEASING.md); it has never run with a real certificate. Auto-update is not built.

## Known limits in this build

* **In-app locks are a privacy screen, not encryption.** Argon2 hashes protect the passcodes, but the files themselves are untouched and every other program can read them. Deleting a folder that *contains* a locked folder is not blocked.
* **Written but never run on Windows or macOS:** drive eject, map/disconnect network drive, the registry-based "Open with" list and Windows' own chooser, `pkexec`/UAC/`osascript` elevation (including the per-drive "Run as administrator" SMART read), and the `smartctl` fallback. Linux paths were exercised.
* **Drive health** needs `smartmontools`. Without it the About page shows what the OS reports and says so.
* **Cross-window tab outline** relies on window positions from the OS; it was written against Tauri's window API and checked only with two windows on Linux (X11).
* **Window buttons on the left** are drawn by the app on Windows and Linux. On macOS the system's own buttons stay where the OS puts them.
* **Fluent icons** replace the toolbar and menu glyphs; file-type icons stay the app's own drawings.
* **Thumbnails for documents and videos** use the Windows shell's thumbnail handlers (through `IShellItemImageFactory`) and macOS Quick Look. Both were written but never run. On Linux, PDF (`pdftoppm`) and video (`ffmpeg`) rendering was run in tests. Office documents have no preview on Linux, and what Windows and macOS show depends on the handlers installed there.
* **Terminal choices for Windows (PowerShell 7, WSL, Git Bash, Windows Terminal profiles and new tabs) and macOS (iTerm, startup commands)** are covered by tests of the command that would be started, not by starting those programs.
* **Resource limits** cap worker threads and cache memory per performance level; they are not a hard limit on processor time or disk throughput.
* **Live drive activity** reads per-drive throughput from the operating system through the sysinfo library, which was not run against a Windows or macOS drive here. The files-per-second expectation uses typical figures per drive type until the scan has its own pace; the total and time-left estimate for a whole drive comes from used space and the average size found so far, so it is rough.
* **Folders and drives with saving off** are skipped by the search index and searched live, so searching them is slower. A disk-usage snapshot is not kept for a drive that contains such a folder.
* **Performance presets** are starting points chosen by reasoning about memory and cores, not tuned from measurements on each class of hardware.

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
* **Languages:** the setup wizard and the language setting are translated into nine languages (no native-speaker review). The main window, menus, dialogs and the long performance-level descriptions are still English only, and right-to-left languages (Arabic, Hebrew) are not supported yet because the layout has not been mirrored.
