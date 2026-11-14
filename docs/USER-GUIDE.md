# User guide

## Getting around

**Tabs.** Ctrl+T opens a tab, Ctrl+W closes it, Ctrl+Shift+T brings back the one you closed last (with its back and forward history; the last 25 are kept, see Advanced > Tabs), Ctrl+Tab and Ctrl+Shift+Tab switch. Tabs start wide and shrink together as you open more; past the minimum width the strip scrolls. Drag a tab out of the strip to make it its own window. Drag a tab onto another window's tab bar to move it there. A window made from a tab has no Back button. Tabs can be grouped with names and colors; drag a group's colored label to move the whole group, and click it to collapse. Tabs are reopened at launch (Settings → General).

**Address bar.** Click the path to type one. The parts of the path are clickable and are also drop targets: drop files on a part to move or copy them there.

**Navigation pane (left).** Sections: Pinned, Frequent, Drives and others. Drag a section heading up or down to reorder the sections. Ctrl+B hides or shows the pane.

**Pinning.** Drag any folder onto the Pinned heading, or between pinned entries, to pin it. Drag pinned entries to reorder. Right-click works too. The standard folders (Downloads, Documents, Pictures, Music, Videos, Desktop) have their own icons.

**Home and This PC.** Home shows pins and recent items. This PC (Ctrl+Shift+C) lists drives and network shares with free space. The list keeps itself current: USB drives, memory cards, discs and network drives show up within a second or two of being connected (and go when removed), with a notice, and the free space is re-read every ten seconds. On Windows every drive letter is listed, including empty card readers and DVD drives (shown as *No media*) and drives that are locked. Settings → Advanced → Files and lists has the switch, the check interval, the free-space interval and the notice. **Phones, cameras and other devices** get their own sections in This PC (phones and cameras also in the navigation pane): Windows lists the portable devices File Explorer shows under This PC; macOS and Linux list USB devices. **Phones and cameras open inside the app on every system**, as a folder of their storages (*Internal storage*, *SD card*): you can browse, open files, copy both ways, paste, drag, rename, move within a storage, create folders and delete (a phone has no Recycle Bin, so the app asks first). Windows and macOS read the phone over MTP themselves (unlock the phone and choose *File transfer* in its USB options; the phone's own name is shown). On macOS, macOS's camera service or Android File Transfer can hold the phone: quit Android File Transfer, and if the phone is still refused the error message gives the one Terminal command that frees it. Linux uses the file manager's mount: a phone GVFS has mounted opens at once, and one that has not is mounted when you open it (needs `gvfs-mtp` / `gvfs-gphoto2`). File Explorer, Android File Transfer, OpenMTP or Image Capture stay as a fallback for devices the app cannot read (an iPhone's photos, for example). Printers, keyboards and receivers are listed too and can be hidden in Settings → Advanced → Files and lists.

## Selecting

* Click selects one. Ctrl+click adds or removes one. Shift+click selects the range from the last click.
* Selections build on each other the way a spreadsheet's do: select a block with Shift, Ctrl+click one more item, Shift+click elsewhere, and the first block stays selected.
* Ctrl+A selects everything. Typing a name jumps to it.

## Moving and copying with the mouse

Drag items onto a folder, a tab, a breadcrumb, or a drive in the navigation pane. It works like Windows Explorer:

* same drive: **move**; another drive: **copy**
* Ctrl while dropping: always copy. Shift: always move
* drop into the folder they already live in: nothing happens (with Ctrl, a duplicate is made)
* a folder cannot be dropped into itself or something inside it
* hover a tab for a moment while dragging and it opens, so you can drop into its folder

Drag items out of the window onto another program to hand it the files, as if you had pasted them there.

**Dragging files in from other programs** (Explorer, Finder, a file manager) copies them into the folder you drop them on: a folder in the list, a breadcrumb, a tab or a sidebar entry. Dropped anywhere else in the window, they go into the folder that is open. If no folder is open (Home, This PC, search results), the drop is refused with a message. Files from other programs are always copied, never moved, and the usual "file already exists" question applies.

## Clipboard

Copy and Cut put the files on the system clipboard, so you can paste into other programs, and files copied in other programs paste here. Cut shows the items dimmed until pasted. A cut that is pasted into another program arrives as a copy; that is an operating-system limit. **Pasting what another program copied:** ordinary files (Explorer, Finder, Nautilus, Dolphin, Files and so on) paste as a copy, and if the other program used *Cut* they are moved, as in any file manager. On Windows, files that Explorer copies from *inside* a zip, from a phone or from a cloud folder (they have no place on disk yet) are first saved to a temporary place and then pasted; a note says so while it happens. Paths copied as text (such as Explorer's *Copy as path*) paste too, when every line is a real file or folder.

## Transfers

Copies, moves and deletes appear as cards at the bottom. Each shows speed, time remaining and the algorithm in use.

* **Pause / resume.** Work continues where it stopped.
* **Cancel.** Stops safely. If a transfer is stuck (a share that stopped answering, for example) a **Force stop** button appears; Shift-click the ✕ to force at once. A force-stopped transfer disappears immediately; files already copied stay.
* **Time remaining** starts as a rough guess (it says so) and improves as the transfer settles. For big jobs the app first runs a very short speed test of the drives involved; it leaves nothing behind.
* **The graph** shows bytes/s and files/s over time. The dashed line at the right edge is the current speed.
* **Algorithm.** *Automatic* uses the built-in engine and learns, from your own transfers, whether a system tool is faster for a given pair of drives: Robocopy on Windows, rsync on macOS and Linux, rclone if installed, and settings tuned for SMB and NFS shares when a share is involved. Small jobs always use the built-in engine. You can force one in Settings → File transfers.
* **Update rate.** Settings → File transfers lets you set how often progress is redrawn. Automatic is slower on laptops and weaker machines to save power.

## When a file already exists

If a copy or move would overwrite a file with the same name, the transfer pauses on that file and shows both sizes and dates, so you can see which is newer. Choose:

* **Replace** or **Skip** for this file only
* **Replace all** or **Skip all** for every other conflict in this transfer
* **Keep both** to save the incoming file under a new name ("name (2)")
* **Cancel transfer** to stop altogether

Folders with the same name are merged, and the files inside them are asked about one at a time. Files without a conflict keep copying while the question is open. Settings → File transfers → "When a file already exists" can make one answer automatic for every transfer instead of asking. A transfer retried as administrator skips existing files rather than replacing them, because that helper has no window to ask in.

## Deleting

Delete (or the toolbar button) sends items to the Recycle Bin / Trash after the usual confirmation. **Shift+Delete**, or **Shift-click on the toolbar Delete button**, deletes permanently after a confirmation. As administrator, items cannot go to the bin and are deleted permanently; the prompt says so.

## Undo and redo

**Ctrl+Z** undoes the last rename, new file or folder, copy, move or delete to the Recycle Bin / Trash, and **Ctrl+Y** (or Ctrl+Shift+Z) does it again. Both are also in the "…" menu of the command bar. Undoing a delete puts the items back where they were (Windows and Linux; on macOS use Put Back in the Trash). Permanent deletes cannot be undone. How many steps are remembered is in Settings → Advanced ("Actions Undo can reverse").

## Renaming many items

Select two or more items and press **F2** (or Rename in the command bar or right-click menu). The dialog shows every old and new name before anything changes:

* **New name** can use `{name}` (the current name), `{n}` (a counter) and `{date}` (the date it was changed), for example `Holiday {n}`.
* **Find / Replace with** changes part of each name; **Match case** is off by default.
* **Start at** and **Digits** control the counter (3 digits gives 001, 002…).
* **Letters** changes case.

Extensions stay as they are. Names that would be empty, contain characters the system does not allow, clash with each other or with another item are marked in red and the button stays off. Shifting a series (1→2, 2→3) works without overwriting anything. One Ctrl+Z undoes the whole batch.

## Zip and other archives

Right-click → **Compress to ZIP** makes `name.zip` next to the item (`Archive.zip` for several items). Right-click a `.zip`, `.tar` or `.tar.gz` / `.tgz` file → **Extract to "name"** unpacks into a new folder next to it, or **Extract here**. Files that already exist are kept, never replaced. Compressing and extracting can be cancelled from the progress card. Symbolic links are left out of zips, and archive entries that try to reach outside the target folder are refused.

### Opening archives like folders

Double-click a zip, 7z, rar, tar, tar.gz / tgz, tar.bz2, tar.xz, single .gz / .bz2 / .xz, cab, iso or similar file and it opens as a folder. The address bar shows the way in (`Documents › photos.zip › 2024`), you can type `/path/photos.zip/2024` to go straight there, and Back, Up and tabs work as usual.

* **Open** a file inside to unpack it to a temporary place and open it with its usual program.
* **Copy** items (Ctrl+C, then paste or drag into a folder) to unpack them there. Folders keep their contents. A file that already exists follows Settings → File transfers → **When a file already exists**: with *Ask me* you get the same Replace / Skip / Keep both question as in any copy (for that file or for all that follow), otherwise it is replaced, skipped or kept under a second name automatically. Skipped files are counted in the notice, not reported as failures. A move out of an archive only takes the originals out when everything was copied or replaced.
* Right-click empty space inside an archive → **Extract all to a folder next to the archive** or **Extract all here**.
* **Changing an archive** works like a zip folder in Windows, for zip and every kind of tar (tar, tar.gz, tar.bz2, tar.xz) by the app itself, and for 7z, wim, iso, cpio, tar.zst, tar.lz and rar through an outside tool (7z and wim need 7-Zip; iso, cpio, tar.zst and tar.lz need bsdtar, which Windows 10 and later and macOS include; rar needs WinRAR's `rar`, because only WinRAR can write RAR files). Those are unpacked to a temporary folder, changed and packed again, so a big one takes longer: paste or drag files and folders in from anywhere, cut and paste or drag to move things between folders of the same archive, rename, make new folders and files, and delete (deleted items do not go to the Trash). A name that already exists is never overwritten; the new item becomes "name (2)". The archive is rewritten beside itself and swapped in only when finished, so a cancel or an error leaves it as it was. Big archives take a moment to rewrite. cab, arj, lzh, rpm, deb and single compressed files (a lone .gz, .xz, .zst) can be read but not changed, because nothing can write them here; the app says so. **Dragging out:** items of any readable archive can be dragged into other programs; they are unpacked as the drag starts.
* **Cut out of an archive** moves the items into the folder you paste in, and removes them from the archive once they have all arrived. Dragging out copies, as in Windows.
* **Other programs:** copying items from inside an archive unpacks them to a temporary place and puts those real files on the system clipboard, so you can paste them into Explorer, Finder, a mail window and so on. Files that Windows Explorer copies from inside a zip paste here too (they are saved to a temporary place first).
* Zip, tar (plain, gzip, bzip2, xz), 7z and single gz/bz2/xz files are read by the app itself. **rar, cab, iso, wim, zstd and the rest need 7-Zip** (Windows: 7-Zip installed, or the `tar.exe` that comes with Windows 11; macOS: `brew install sevenzip`, or the built-in `bsdtar`; Linux: `7zip` / `p7zip-full`). Without one the app says so instead of failing quietly.
* Password-protected archives are not supported yet; the app says so. Names that try to leave the destination folder and links are never unpacked.
* Turn it off in Settings > Advanced > Files and lists > "Open archives like folders" to have a double click hand archives to another program.

### Sort order and filters across folders

The sort order you choose (for example Date modified, newest first) stays for every folder, and is still there when you start the app again. Change it from the column headings, Sort, or Settings > Advanced > Files and lists. Turn off "Use the same sort order in every folder" to start each folder sorted by name. A filter is cleared when you open another folder unless you turn on "Keep the filter when opening another folder".

## Folders refresh by themselves

A folder that is open updates when another program adds, renames or deletes files in it, a moment after the change. Only open folders are watched, not their subfolders. Turn it off in Settings → General ("Refresh folders when they change"); F5 always works.

## Sharing

Right-click → Share:

* **Email link / copy link:** starts a temporary download link for one file. Anyone on the same network who has the address can download it until it expires (1 minute to 7 days) or reaches the number of downloads you set. The address contains a random code nobody can guess.
* This is plain HTTP on your local network, and works only while the app is running. It is not encrypted; do not use it for secrets, and do not expect it to work from outside your network. Windows may ask for firewall permission the first time. Folders must be archived first.
* **Outside your network:** choose "Anyone with the link (internet)". This needs the free `cloudflared` program (Cloudflare's quick tunnels: no account, nothing to change on your router, https). The file stays on your computer and is not uploaded, but the download passes through Cloudflare's network. Anyone who has the link can download until it expires, reaches its limit, or you stop it; it works only while this app is open, and it is closed when you quit. Prefer a short time and a limit of 1 download for anything private. If `cloudflared` is not installed the dialog says so.
* Stop a link at any time from the share dialog.

## Details pane (Alt+P)

One item: type, size, size on disk, dates, permissions, SHA-256 and more. Several items: how many files and folders, total size, the largest file, where they are, the oldest and newest change, a breakdown by type, and a **Measure folders** button that adds up everything inside the selected folders.

## Searching

Type in the search box. Results come from an index that is built in the background, so search is immediate. Filters: `ext:png`, `type:image`, `size:>10mb`, and wildcards like `report*.pdf`.

## Disk usage

Right-click a folder or drive → Analyze disk usage. Folders are drawn as frames containing their contents; colors are file types. Click to zoom, hover for sizes. The list beside it shows the largest files, which can be deleted from there.

On a computer with power to spare (8 or more cores, 8 GB of memory, on mains power), the banner shown during a scan also draws two small graphs: how busy the drive is (bytes per second, against the busiest it has been during this scan) and how many files per second are being found, with the pace expected for that kind of drive. When the whole drive is being scanned it also estimates how many items there are in all and how long is left. This is an estimate from used space and the average size of what has been found so far, so it can jump around early on. Turn it on or off in Settings > Advanced > *Live drive activity in Disk usage* (Automatic, Always, Never).

## Servers (FTP, SFTP, WebDAV)

Click **Connect to server…** at the bottom of the sidebar. Choose SFTP (SSH), FTP or WebDAV, type the server, your user name and password, and optionally a folder to start in. The server opens as a folder you can browse in a tab.

* **Copy, move, delete, rename, new folder** work like on your own disks. Drag files between a server tab and a local tab, or copy and paste. Moves between a server and this computer copy first and delete the original only when everything arrived. Moving inside one server is a rename on the server.
* **Opening a file** downloads it to a temporary place and opens it with your usual program. Changes you make there are not sent back; copy the file to the server again.
* **Deleting from a server cannot be undone** (servers have no Recycle Bin), so you are always asked first. Undo and Redo do not cover servers.
* **Passwords are never saved.** "Keep this address in the sidebar" saves only the server, user name and folder; you type the password once per session. Each address also keeps its own login in memory, so several tabs share one.
* **SFTP asks you to trust a server the first time.** The dialog shows the server's fingerprint; compare it with the one your server's owner gave you. If the key later changes, the app warns you and does not connect until you confirm. You can also log in with a private key file (the password box then holds its passphrase). Next to *Private key file*, **Browse…** hides the form, shows your files in the window (starting in `~/.ssh` when it exists) and asks you to open or select the key and press Choose, or to type its full path; the form comes back with the path filled in. The same picker is used wherever the app asks for a local file.
* **FTP and plain-HTTP WebDAV are not encrypted**: the dialog warns you. Prefer SFTP, or WebDAV with HTTPS.
* Phones and cameras (`mtp://…` addresses) work like server folders for browsing and copying. Thumbnails of files on a phone are not made, to avoid reading the phone's files all the time.
* Not available on server folders: thumbnails, folder sizes, checksums, "Compress", "Share", terminal, disk usage, Properties, new empty files. WebDAV needs a server that allows PROPFIND, MKCOL and MOVE (Nextcloud, ownCloud, Synology, Apache and nginx with DAV do).

## Network drives

Mapped drives and shares show under Drives with their drive letter. If you run the app as administrator, drives mapped in your normal session are still listed (they are found through your user's saved mappings).

## Running as administrator

The app does not need it. When a transfer is refused for permissions it offers **Retry as administrator**, which runs only that job elevated.

## First run

The first time the app starts it shows one setup page: the interface style (Windows, macOS or Linux), light or dark, a performance preset, what the app may remember, and whether server addresses are hidden. The recommended answer is marked on each, based on this computer. Choose **Skip** to keep the defaults. Everything on that page is also in Settings. The interface style starts on the one for the system you are using, marked Recommended. Performance is a slider from light to heavy with the recommended level marked above it. The Privacy view says plainly that all information is stored locally and nothing is sent to a cloud.

## Language

The app follows the system language when it is one of English, Spanish, French, German, Portuguese, Italian, Russian, Simplified Chinese, Japanese or Korean, and uses English otherwise. Change it on the first welcome view or in Settings > Interface > Language. The setup wizard, the language setting and the Settings group names are translated; other labels still appear in English and will follow as they are translated. The translations were written without a native-speaker review, so wording fixes are welcome. To add a language, add a file in `src/i18n/locales`, list it in `src/i18n/keys.ts` order, and add one line to `LANGUAGES`.

## Presets and profiles

* **Interface style** (Settings > Presets and profiles) rearranges the window the way that system's file manager is arranged, using the same parts: for macOS a Finder-style toolbar with the window buttons, back and forward, the folder name, view, sort and share buttons and search along the top, a Favorites and Locations sidebar, no tab strip until there are two tabs, and a path bar at the bottom; for Linux a header bar with joined back and forward buttons, a pill-shaped path bar, view and menu buttons and search, and a Places sidebar. It also sets the layout, colors, icons, corner shapes, the side of the window buttons and the keyboard shortcuts to match Windows 11 File Explorer, macOS Finder or a GNOME/KDE file manager. Colors and icons are open-source (Fluent System Icons, MIT); no Windows or Apple files are used.
* **Performance presets**, from Barebones to God-like, set threads, cache size, indexing, thumbnails, folder sizes and what is saved. Each one lists the kind of device it suits, and the one recommended for this computer is marked.
* **Profiles** are saved copies of all your settings, as many as you like. While one is in use, your changes are saved into it automatically.

## What the app uses

Settings > Performance > *What the app is using* shows live processor, memory and disk use, and what is kept for later (caches on disk, caches in memory). *Give back memory now*, *Clear caches that rebuild themselves* and *Smallest footprint* shrink what the app keeps. They do not compress your files. Settings > Storage lists each cache with its own Clear button and an on/off switch (thumbnails, folder sizes, disk-usage results, folder learning, hardware details, search history). Below the table, **Folders and drives** clears what is saved about one folder or drive, and can switch saving off for it: pick a drive, choose a folder, or type a path, then *Clear what is saved here* or *Stop saving for this place*. A folder covers everything inside it. Nothing is then kept about those places (listings, folder sizes, thumbnails, disk-usage results, folder learning, search entries), and what was kept is cleared at once. Search still works there, but reads the folder each time. *Save again* removes the rule.

Each performance level also sets a **processor limit** and a **memory limit** (Settings > Performance), as a share of all cores and of installed memory. The processor limit caps how many worker threads run at once, and the memory limit caps the caches. Both can be changed by hand, and 0 means no limit. God-like sets no limit at all. These are caps on the app's threads and caches, not a hard limit on processor time: a single thread can still use a full core.

## Locking folders and drives

Right-click a folder or a drive and choose **Lock with passcode…** (the last item). A locked folder cannot be opened, listed, searched, previewed, copied from or deleted inside this app until the passcode is entered; it relocks after the time set in Advanced. Locks are managed under Settings > Privacy and locks, and removing one needs the main password. **This is a privacy screen, not encryption:** other programs and other file managers can still read the files. Use disk or file encryption for that.

## Thumbnails

In the larger icon views (Medium, Large, Extra large) files show a preview of their contents: pictures, the first page of PDFs and Office documents, and a frame from videos. Details, List and Small icons keep the plain icon, and the details pane always shows a preview of the selected file. All of that is in Settings > Thumbnails: each kind of file, each view, sharpness, how many are made at once, network drives (**Automatic** by default: thumbnails are made while the computer is on a cable, Ethernet or fibre, and not on Wi-Fi or mobile data; or Always on / Always off), the largest picture to preview and which frame of a video to show. Windows uses the same preview handlers as Explorer and macOS uses Quick Look. On Linux, PDFs need `pdftoppm` (poppler-utils) and videos need `ffmpegthumbnailer` or `ffmpeg`; without them those files keep their icons. Office documents are previewed only on Windows and macOS. Every performance level that has thumbnails shows them for pictures, documents and videos; videos and documents can be switched off separately in Settings > Thumbnails. Photos taken in portrait are turned the right way up from their camera's orientation tag.

## Terminal

Settings > Terminal chooses the terminal (on Windows Command Prompt, PowerShell, PowerShell 7, Windows Terminal, WSL or Git Bash), the Windows Terminal profile, a command to run when it opens (for example `git status`), whether to open a tab in the window that is already open, which folder the Open terminal button uses (the one being viewed or your home folder), whether a single selected folder is used, the shell to run on Linux and macOS, and a custom command with `{path}` for the folder. A custom command replaces the other choices. The startup command is not used for administrator terminals. On macOS the startup command works with Terminal and iTerm only.

## Opening files with no app

Opening a file goes in this order: the app set for that kind of file, then whatever the system itself does with it (the system's own choice is trusted, including Store apps such as Photos), and only if neither works the app picks the most likely installed app for the file type, counting programs that say they can open it (an image viewer for a picture, a media player for a video, and so on for the common types). Only when none is found does it show a chooser (Default app, Suggested apps, More options, Always / Just once), or the system's own chooser on Windows, or only a message. Choose which in Advanced > *When no app is set to open a file*. *Open with…* always shows the chooser.

## Settings

Settings (Ctrl+,) has a search box at the top. Each change applies and saves at once. Sections: General, Presets and profiles, Appearance, Themes, Layout, Navigation pane, Performance, Storage, Privacy and locks, File transfers, Search, Notifications, Right-click menu, Terminal, Advanced (keyboard shortcuts, your own right-click actions, maintenance), Documentation and About.

* **Performance** controls how much of the computer the app may use. Automatic adapts to CPU, memory and battery.
* **Right-click menu:** every entry can be hidden, and you can add your own commands under Advanced.
* **Keyboard shortcuts** can be changed under Advanced.
* Sizes can be shown as decimal (KB = 1000), binary (KiB = 1024) or Windows style (1024 labelled KB).

## Troubleshooting

* **A share is missing:** check that it is reachable; offline shares are shown greyed out.
* **Transfers feel slow:** open the card's details to see the algorithm and rate. A hard disk or a network link limits speed more than any setting. Leave Automatic on and the app learns what is fastest.
* **A transfer will not stop:** Cancel, then Force stop.
* **Search misses new files:** the index catches up in the background; give it a moment, or press F5 in the folder.
* **Something looks wrong after an update:** Settings → Advanced → Maintenance has options to clear stored data.

**Sort, Filter and View are remembered.** The column and direction you sort by, what is in the Filter menu, and the view you pick (Details, List, small to extra large icons) stay when you open another folder. The sort and the view are also still there the next time you start the app. The filter is kept across folders (not across restarts, so a folder never opens looking empty because of an old filter); the Filter button shows a dot while one is on, and *Clear filter* removes it. Each of the three has a switch in Settings → Advanced → Files and lists (*Use the same sort order in every folder*, *Use the same filter in every folder*, *Use the same view in every folder*), all on by default. With the view switch off, a view picked from the View menu applies only to the folder you are in, and other folders open in the view chosen under Appearance → Folder view.

**Sort and filter after a job.** When copying, moving or deleting finishes, the folders it touched are reloaded and the current sort and filter are applied right away, so new files land in their sorted place. Turn it off under Settings > Files and lists > "Sort and filter when a job finishes".

**Archives.** Drag files out of a zip (or any archive the app can open) straight into another program: the items are unpacked as soon as you start dragging and handed over when you reach the edge of the window. Drag files onto an archive file in a list, or into an archive you have opened, and they are added to it. The app rewrites the archive for you; there is no need to unzip first.

**Jobs run in the order you asked for them.** If you start a copy, move or delete that touches the same folders or file names as one that is still running (delete a folder, then paste its replacement; paste the same files twice), the later one waits in line, shown in the transfers panel, and starts when the earlier one ends. Jobs that do not touch each other still run side by side. When a file is already at the destination with the same name, size, modified time (and contents, for files up to 8 MB), it is kept and skipped without asking; turn that off under Settings > Transfers > "Skip files that are already there".
