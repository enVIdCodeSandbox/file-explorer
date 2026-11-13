# User guide

## Getting around

**Tabs.** Ctrl+T opens a tab, Ctrl+W closes it, Ctrl+Tab and Ctrl+Shift+Tab switch. Tabs start wide and shrink together as you open more; past the minimum width the strip scrolls. Drag a tab out of the strip to make it its own window. Drag a tab onto another window's tab bar to move it there. A window made from a tab has no Back button. Tabs can be grouped with names and colours; drag a group's coloured label to move the whole group, and click it to collapse. Tabs are reopened at launch (Settings → General).

**Address bar.** Click the path to type one. The parts of the path are clickable and are also drop targets: drop files on a part to move or copy them there.

**Navigation pane (left).** Sections: Pinned, Frequent, Drives and others. Drag a section heading up or down to reorder the sections. Ctrl+B hides or shows the pane.

**Pinning.** Drag any folder onto the Pinned heading, or between pinned entries, to pin it. Drag pinned entries to reorder. Right-click works too. The standard folders (Downloads, Documents, Pictures, Music, Videos, Desktop) have their own icons.

**Home and This PC.** Home shows pins and recent items. This PC (Ctrl+Shift+C) lists drives and network shares with free space.

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

Copy and Cut put the files on the system clipboard, so you can paste into other programs, and files copied in other programs paste here. Cut shows the items dimmed until pasted. A cut that is pasted into another program arrives as a copy; that is an operating-system limit.

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
* **Copy** items (Ctrl+C, then paste or drag into a folder) to unpack them there. Folders keep their contents. Files that already exist are kept, never replaced, and the notice says how many were left alone.
* Right-click empty space inside an archive → **Extract all to a folder next to the archive** or **Extract all here**.
* **Changing an archive** works like a zip folder in Windows, for zip, tar, tar.gz and 7z (7z needs 7-Zip installed): paste or drag files and folders in from anywhere, cut and paste or drag to move things between folders of the same archive, rename, make new folders and files, and delete (deleted items do not go to the Trash). A name that already exists is never overwritten; the new item becomes "name (2)". The archive is rewritten beside itself and swapped in only when finished, so a cancel or an error leaves it as it was. Big archives take a moment to rewrite. rar, tar.bz2, tar.xz, iso and the like can be read but not changed; the app says so.
* **Cut out of an archive** moves the items into the folder you paste in, and removes them from the archive once they have all arrived. Dragging out copies, as in Windows.
* **Other programs:** copying items from inside an archive unpacks them to a temporary place and puts those real files on the system clipboard, so you can paste them into Explorer, Finder, a mail window and so on. Copying from another program's view of a zip is not understood yet when that program does not put real files on the clipboard (Windows Explorer does not for files inside a zip).
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

Right-click a folder or drive → Analyze disk usage. Folders are drawn as frames containing their contents; colours are file types. Click to zoom, hover for sizes. The list beside it shows the largest files, which can be deleted from there.

## Servers (FTP, SFTP, WebDAV)

Click **Connect to server…** at the bottom of the sidebar. Choose SFTP (SSH), FTP or WebDAV, type the server, your user name and password, and optionally a folder to start in. The server opens as a folder you can browse in a tab.

* **Copy, move, delete, rename, new folder** work like on your own disks. Drag files between a server tab and a local tab, or copy and paste. Moves between a server and this computer copy first and delete the original only when everything arrived. Moving inside one server is a rename on the server.
* **Opening a file** downloads it to a temporary place and opens it with your usual program. Changes you make there are not sent back; copy the file to the server again.
* **Deleting from a server cannot be undone** (servers have no Recycle Bin), so you are always asked first. Undo and Redo do not cover servers.
* **Passwords are never saved.** "Keep this address in the sidebar" saves only the server, user name and folder; you type the password once per session. Each address also keeps its own login in memory, so several tabs share one.
* **SFTP asks you to trust a server the first time.** The dialog shows the server's fingerprint; compare it with the one your server's owner gave you. If the key later changes, the app warns you and does not connect until you confirm. You can also log in with a private key file (the password box then holds its passphrase).
* **FTP and plain-HTTP WebDAV are not encrypted**: the dialog warns you. Prefer SFTP, or WebDAV with HTTPS.
* Not available on server folders: thumbnails, folder sizes, checksums, "Compress", "Share", terminal, disk usage, Properties, new empty files. WebDAV needs a server that allows PROPFIND, MKCOL and MOVE (Nextcloud, ownCloud, Synology, Apache and nginx with DAV do).

## Network drives

Mapped drives and shares show under Drives with their drive letter. If you run the app as administrator, drives mapped in your normal session are still listed (they are found through your user's saved mappings).

## Running as administrator

The app does not need it. When a transfer is refused for permissions it offers **Retry as administrator**, which runs only that job elevated.

## First run

The first time the app starts it shows one setup page: the interface style (Windows, macOS or Linux), light or dark, a performance preset, what the app may remember, and whether server addresses are hidden. The recommended answer is marked on each, based on this computer. Choose **Skip** to keep the defaults. Everything on that page is also in Settings.

## Presets and profiles

* **Interface style** (Settings > Presets and profiles) sets the window layout, colours, icons, corner shapes, the side of the window buttons and the keyboard shortcuts to match Windows 11 File Explorer, macOS Finder or a GNOME/KDE file manager. Colours and icons are open-source (Fluent System Icons, MIT); no Windows or Apple files are used.
* **Performance presets**, from Barebones to God-like, set threads, cache size, indexing, thumbnails, folder sizes and what is saved. Each one lists the kind of device it suits, and the one recommended for this computer is marked.
* **Profiles** are saved copies of all your settings, as many as you like. While one is in use, your changes are saved into it automatically.

## What the app uses

Settings > Performance > *What the app is using* shows live processor, memory and disk use, and what is kept for later (caches on disk, caches in memory). *Give back memory now*, *Clear caches that rebuild themselves* and *Smallest footprint* shrink what the app keeps. They do not compress your files. Settings > Storage lists each cache with its own Clear button and an on/off switch (thumbnails, folder sizes, disk-usage results, folder learning, hardware details, search history).

## Locking folders and drives

Right-click a folder or a drive and choose **Lock with passcode…** (the last item). A locked folder cannot be opened, listed, searched, previewed, copied from or deleted inside this app until the passcode is entered; it relocks after the time set in Advanced. Locks are managed under Settings > Privacy and locks, and removing one needs the main password. **This is a privacy screen, not encryption:** other programs and other file managers can still read the files. Use disk or file encryption for that.

## Opening files with no app

If a file type has no program, the app shows a chooser (Default app, Suggested apps, More options, Always / Just once), or the system's own chooser on Windows, or only a message. Choose which in Advanced > *When no app is set to open a file*. *Open with…* always shows the chooser.

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
