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
* **Selection checkboxes** (Settings > General > Show selection checkboxes, off by default) put a box to the left of every item and a Select all box in the column heading. A click on an item or its box ticks it, another click unticks it, and ticked items stay ticked when you click elsewhere, on empty space, or drag a selection box (the box adds to what is ticked). Shift+click turns over every item between the last click and this one: ticked ones become unticked and the others ticked. Space ticks the item the arrow keys are on. Checkboxes are for computers; phones keep their own select mode.
* **Folders open with a single click** if you turn on Settings > General > Open folders with a single click. Files still open with a double click. (The older "Open items with a single click" does the same for files too.)

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
* **Deleting, compressing and unpacking are measured like copies.** Their cards show the same speed, time left (with the same *rough estimate → settled* note) and, in the expanded window, the same graph (items per second, plus bytes per second when the job knows how many bytes there are), typical time, average and peak speed. Moving to the Recycle Bin / Trash counts items only. Each finished job is remembered with the others (unless *Remember how fast my drives are* is off in Settings → File transfers), kept apart by kind: a delete is judged by earlier deletes on the same drive, not by copies. Copying and moving between folders run through the transfer engine and have always had all of this. A move inside one drive is instant but is still shown and remembered: its graph is a short rise to its speed, and it is kept in the history without ever shaping an estimate.
* **Which method is being used.** Settings → Advanced → Transfers → *Transfer methods in use* shows how every kind of job is being carried out right now: copies and moves (built-in engine, Robocopy, rsync, rclone, SMB-tuned or NFS-tuned), deletes (direct removal or the Recycle Bin / Trash), compressing and unpacking, copies into and out of archives, servers and phones (the connection, such as SFTP over SSH, FTP, WebDAV or MTP) and update downloads. It also shows the latest method of each kind, which copy methods this computer can use, and for each method and kind how many remembered jobs used it and their average speed. The expanded Transfers window names the method of each transfer under *Algorithm* too.
* **Copies made by Robocopy** (Windows, chosen automatically for some copies between drives, or in Settings → File transfers) used to be counted whole the moment each file began, so the graph spiked and the speed and time left were wrong. The file being written is now followed on disk, so the graph shows the real speed.
* **Algorithm.** *Automatic* uses the built-in engine and learns, from your own transfers, whether a system tool is faster for a given pair of drives: Robocopy on Windows, rsync on macOS and Linux, rclone if installed, and settings tuned for SMB and NFS shares when a share is involved. Small jobs always use the built-in engine. You can force one in Settings → File transfers.
* **Speed, priority and the queue.** In the expanded Transfers window every running transfer has its own **priority** (high goes first when others are waiting; low goes last and gives way to other programs), **speed limit** (KB/s, 0 = none) and **time limit** (minutes, then pause or stop). Settings → File transfers sets the defaults: a *maximum total speed* for all transfers together, a *target speed* for each new transfer, the *longest a transfer may run*, the *priority of new transfers*, and how many transfers work *at the same time* (the rest show as *waiting for its turn*, and **Start now** skips the queue). A transfer held to a speed limit uses the built-in engine, because copy tools such as Robocopy and rsync cannot be held to one here. Instant copies (copy-on-write) and same-drive moves move no data, so a limit does not apply to them.
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

Pasting into a folder that a delete is still working on does not race it: the paste waits in line (shown at the bottom with the other jobs, with a ✕ to take it out) and starts by itself when the delete is over, so the files that were deleted are replaced by the pasted ones. Pastes that overlap wait for each other in the order you made them, and pasting the very same thing again while it waits does nothing new.

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

### Columns: choose what Details shows

View > Choose details… (or right-click a column heading, then More…) opens a list like File Explorer's Choose Details, with search and 140+ columns in groups. Tick the columns you want, Move Up and Move Down to reorder, and set a width in pixels. Name always stays first.

- **Basic, Name and Dates:** date modified and created, extension, kind, name without extension, name length, first letter, and the date, time of day, weekday, month, year and "how long ago" of the modified and created dates, plus date accessed and date changed. These need nothing from the disk, so they also work on servers, phones and archives.
- **Size:** size on disk, exact bytes, KB and MB, items in a folder, files, folders and hidden items inside.
- **Kind and flags, File system, Permissions:** hidden, link, empty; owner and group names, user and group IDs, inode, hard links, content type, link target; permissions as `-rwxr-xr-x` or `755`, read-only, executable and the Windows flags (system, archive, compressed, encrypted, offline, temporary, sparse, reparse point).
- **Content:** what the file really is by its first bytes (PNG, PDF, ELF…), text or binary, text encoding, line endings, line and word counts (text up to 8 MB), script interpreter.
- **Checksums:** SHA-256, SHA-1, MD5 and CRC-32, for files up to 64 MB. Showing one reads every file in view, so use it in the folders where you need it.
- **Documents:** title, author, last saved by, subject, keywords, comments, category, company, program, pages, words, characters, slides, created and saved dates, for Word, Excel, PowerPoint, OpenDocument and PDF.
- **Programs:** type, architecture, subsystem, build date, product, publisher, description, version and copyright of Windows programs and libraries; type and architecture of Linux and macOS programs.
- **Archives:** files in a zip (also jar, apk, epub and similar), unpacked size and how far it is packed.
- **Pictures, Music and video:** dimensions, width, height, megapixels, aspect ratio, landscape or portrait, date taken, camera, maker, lens, ISO, aperture, exposure, focal length, flash, white balance, metering, orientation, GPS position, photographer, copyright; length (also in seconds), title, artist, album artist, album, track, disc, composer, genre, year, BPM, bit rate, sample rate, bit depth, channels, frame size and rate, codecs.

Columns that have to read the file (everything marked as read from the file in the list) stay blank on servers, phones and archives. A value a file does not have is left blank, and blanks sort last.

**Working with a column.** Click a heading to sort by it. Right-click a heading (or use the small funnel that appears on it) for that column's menu: sort either way, *Filter by…*, size to fit, reset width, move left or right, hide, and the quick list of columns. *Filter by…* lists the values the column has in this folder with counts, and dates and sizes as ranges (Today, Earlier this month… Tiny, Large…); untick the ones to hide. Filtered columns are shown in the accent colour, and Clear in the Filter menu removes them too. Drag a heading to another place to reorder; drag its edge to resize; double-click the edge to fit the widest value.

The list is the same on a computer and a phone (the phone layout shows the first three chosen columns under each name). Details that come from inside the file need the file on this computer's drives, so on servers, phones and in archives those columns are greyed in the list and stay blank. They are read only for the rows on screen, a moment after you scroll, so a big folder stays quick; sorting by one reads every item once.

### Sort order and filters across folders

By default each folder remembers its own sort order, filter and columns (Settings > General > Remember sort order, filter and columns for each folder). A folder you have not changed starts as the defaults say. Turn the option off to use one order, one filter and one set of columns everywhere, which is what the older options below then control. In the Choose details window, "Use these columns in every folder, and keep doing so" switches to one set of columns for all folders (what you set there, and every later change from a heading or the window, applies everywhere) while sort order and filters stay per folder. It is also in Settings > Advanced as "Use the same columns in every folder". Untick it to give folders their own columns again.


With per-folder views turned off, the sort order you choose (for example Date modified, newest first) stays for every folder, and is still there when you start the app again. Change it from the column headings, Sort, or Settings > Advanced > Files and lists. Turn off "Use the same sort order in every folder" to start each folder sorted by name. A filter is cleared when you open another folder unless you turn on "Keep the filter when opening another folder".

## Folders refresh by themselves

A folder that is open updates when another program adds, renames or deletes files in it, a moment after the change. Only open folders are watched, not their subfolders. Turn it off in Settings → General ("Refresh folders when they change"); F5 always works.

## Sharing

Right-click → Share:

* **Email link / copy link:** starts a temporary download link for one file. Anyone on the same network who has the address can download it until it expires (1 minute to 7 days) or reaches the number of downloads you set. The address contains a random code nobody can guess.
* This is plain HTTP on your local network, and works only while the app is running. It is not encrypted; do not use it for secrets, and do not expect it to work from outside your network. Windows may ask for firewall permission the first time. Folders must be archived first.
* **Outside your network:** choose "Anyone with the link (internet)". This needs the free `cloudflared` program (Cloudflare's quick tunnels: no account, nothing to change on your router, https). The file stays on your computer and is not uploaded, but the download passes through Cloudflare's network. Anyone who has the link can download until it expires, reaches its limit, or you stop it; it works only while this app is open, and it is closed when you quit. Prefer a short time and a limit of 1 download for anything private. If `cloudflared` is not installed the dialog says so.
* Stop a link at any time from the share dialog.

## Tasks (automatic jobs)

**Tasks**, next to Share in the toolbar, opens a window where you set up jobs that run by themselves, like a cron job: back up a folder every night, clear downloads older than 30 days, move photos off a card when it is plugged in, mirror a folder to a NAS, run a script when something is dropped in a folder. Right-click a file or folder and choose **Automate with a task…** to start one already filled in.

**When it runs.** A task can have several triggers: every day (or on chosen weekdays) at a time, every so many minutes, hours or days, once a month on a day (a short month uses its last day), once at a set time, when the app starts, when a folder changes (after it has been quiet for a few seconds), when a drive whose name contains some text is connected, or only when you press **Run now**.

**What it does.** Steps run one after the other: copy, move, delete (to the trash, or for good), compress to ZIP (the name can use `{date}`, `{time}`, `{datetime}`, `{year}`, `{month}`, `{day}` and `{task}`), extract an archive, sync or mirror a folder, or run a program. Copy, move, delete, compress and sync can be limited by name patterns (`*.jpg; report*`), things to leave out, older or newer than some days, smallest and largest size, hidden files and subfolders. Copies are written to a temporary file and renamed when whole, and a file that is already there with the same size and date is skipped, so a task that was cut short simply carries on. If a file already exists you can replace it, replace it only if the new one is newer, leave it, or keep both. **Preview** lists exactly what a task would do without doing anything.

**Options.** Per task: stop at the first failure or keep going; retry a failed file a number of times with a pause; stop after a time limit; run once when the computer is back if a run was missed while it was off or asleep; skip a run while the last is still going; only while plugged in; run quietly (low priority); always run as administrator; and whether to be told when it finishes (only on failure by default).

**Administrator rights.** When a run is refused for lack of permission, the task shows **Run as administrator** and your system asks you to approve it (the Windows prompt, macOS password box or Linux password prompt). A task set to always run as administrator asks every time it runs; the app itself never runs elevated.

**Sleep, shutdown and restarts.** Tasks run while the app is running. Turn on **Start with the computer** under Advanced > Tasks in Settings to have the app start, minimized, when you sign in. When the computer wakes from sleep during a run, the task either carries on (files that were not finished are done again) or cancels that run and waits for its next turn, as chosen per task. When the app is closed or the computer shuts down during a run, a task set to carry on is picked up again at the next start; one set to cancel waits for its next scheduled run. If the app was killed without warning (a power cut), the run is recorded as interrupted and handled the same way. A run missed while the computer was off is either done once at the next start or noted as missed, per task.

**Statistics.** Every run is recorded: when, how long, how many items and how much data, the speed, how it ended, why it ran and what went wrong. Each task shows its runs, successes, failures and success rate, average, median, slowest-10% and fastest and slowest run time, average speed (and whether it is getting faster or slower), the average amount moved, totals, the last success and failure with its error, and how long the next run should take. That estimate follows recent runs more than old ones, and is based on speed and the amount of data the task would handle now. Finished copy, move and sync tasks also feed the history behind the time estimates for ordinary transfers, so both learn from each other. The history window lets you clear one task's records, and tasks can be exported to a file and imported again (imported tasks start turned off).

**Advanced > Tasks (in Settings):** turn all tasks off, hide the toolbar button, let tasks teach the time estimates, keep a record of every run (this also holds the log of interrupted and missed runs), how many runs to keep per task and for how many days, messages when a task finishes, the confirmation for tasks that delete for good, and starting with the computer.

## Details pane (Alt+P)

One item: type, size, size on disk, dates, permissions, SHA-256 and more. Several items: how many files and folders, total size, the largest file, where they are, the oldest and newest change, a breakdown by type, and a **Measure folders** button that adds up everything inside the selected folders.

## Searching

Type in the search box. Results come from an index that is built in the background, so search is immediate. Filters: `ext:png`, `type:image`, `size:>10mb`, and wildcards like `report*.pdf`.

**Where to look** is chosen by the shortcut as well as by the box next to the search field: **Ctrl+F** searches only the folder you are in (the items on that level, nothing deeper), **Ctrl+Shift+F** searches everywhere, and **Ctrl+R then F** searches the folder you are in and every folder inside it. Searching starts as you type, with no need to press Enter. The filters you set (type, date, size) are remembered when you leave or restart (Advanced > Files and lists > "Use the same filter in every folder", on by default). What the search box searches by default and whether Ctrl+R, F works are under Advanced > Search, and every shortcut can be changed under Advanced > Keyboard shortcuts. Right-clicking a search result gives the same menu as in a folder, including Pin to the navigation pane.

## Type to find

Off by default (Advanced > Files and lists > "Type to find", or give it a shortcut under Advanced > Keyboard shortcuts). With the file list focused, start typing and the selection jumps to the closest name alphabetically; after about 10 seconds without typing, the text resets (the time can be changed).

## Disk usage

Right-click a folder or drive → Analyze disk usage. Folders are drawn as frames containing their contents; colors are file types. Click to zoom, hover for sizes. The list beside it shows the largest files, which can be deleted from there.

On a computer with power to spare (8 or more cores, 8 GB of memory, on mains power), the banner shown during a scan also draws two small graphs: how busy the drive is (bytes per second, against the busiest it has been during this scan) and how many files per second are being found, with the pace expected for that kind of drive. When the whole drive is being scanned it also estimates how many items there are in all and how long is left. This is an estimate from used space and the average size of what has been found so far, so it can jump around early on. Turn it on or off in Settings > Advanced > *Live drive activity in Disk usage* (Automatic, Always, Never).

## Smart Clear

Frees up space. Open it from the navigation pane, or right-click a drive → Smart Clear. Pick one drive or the whole device at the top.

It looks for things that can probably go and sorts them into groups, safest first:

| Group | What it is | Ticked to begin with |
| --- | --- | --- |
| Temporary files | Leftovers in the system's temporary folders, and files named like temporary files | Yes |
| App caches | What apps keep to start faster; rebuilt when needed | Yes |
| Picture previews | Thumbnail folders | Yes |
| Trash | What you already deleted | Yes |
| Unfinished downloads | `.crdownload`, `.part` and similar, more than two days old | Yes |
| Old logs and crash reports | Logs over 1 MB not touched for a month; crash dumps | No |
| Installers in Downloads | Setup files and disk images more than a month old | No |
| Project build folders | `node_modules`, `__pycache__`, Rust's `target`, in projects not touched for six months | No |
| Exact copies | Files with the same contents as another file; one copy is always kept | No |
| Old downloads | In Downloads, not changed or opened for six months | No |
| Large files not used for a long time | 200 MB or more, not changed or opened for six months | No |

The large figure at the top is how much space clearing the ticked items gives back. Change what is ticked in any of these ways:

* **The slider** clears a share of what was found, starting with the safest groups and, within a group, with what has been left alone the longest. Your own files come last.
* **Filters** narrow the list by age, size, or part of a name or folder. Only what matches is cleared.
* **Tick boxes** on a group or on single items. Open a group to see what is in it; the folder button shows an item where it lives.

Clearing deletes for good. Items do not go to the Trash, because that would free no space. The app asks first and says how many of the items are your own files rather than leftovers.

What it never touches: system folders, other people's files, folders locked with a passcode, and the app's own data (that has its own page under Settings > Storage). "Not opened for a long time" uses the later of the changed and opened dates; many systems do not keep the opened date current, so treat the last three groups as suggestions and look before clearing. Looking for exact copies reads the contents of same-sized files and is the slow part; untick it to skip it.

On a phone it looks through shared storage only. Android does not let any app clear another app's cache; the system's own storage page does that.

## Default file manager

Settings > General > Default file manager makes folders that other apps open come here instead of the system's file manager. The same button puts things back. It changes settings for the signed-in person only and needs no administrator rights.

| System | What opens here | What stays with the system |
| --- | --- | --- |
| Linux | Folders opened by other apps, and "Show in folder" in browsers and other apps (the file is highlighted) | A file manager that is already running keeps answering "Show in folder" until it is closed or you sign in again |
| macOS | Folders opened by other apps; "Reveal in Finder" in apps that honour the system setting | Finder keeps the desktop, and some apps ask Finder directly. The app must be installed in Applications |
| Windows | Folders and drives opened from other apps, the desktop and shortcuts; `.zip` files (a download clicked in a browser's list opens the zip like a folder) | "Show in folder" in browsers and many apps asks File Explorer directly. Win+E, the taskbar's folder button, Control Panel and other special folders stay with File Explorer |

On Windows, turn it off before removing the app. The `.exe` installer's uninstaller does this by itself; the `.msi` one does not, and folders would be left pointing at a program that is gone. If that happens, install the app again and turn the setting off.

Starting the app while it is already running brings its window forward instead of starting a second copy. From a command line:

```
fast-file-explorer <folder>            opens the folder in a new tab
fast-file-explorer <archive>           opens the archive (zip, 7z, tar, ...) like a folder
fast-file-explorer <other file>        opens the file's folder with the file highlighted
fast-file-explorer --select <path>     highlights the item in its folder, also when it is a folder
```

`file://` addresses work wherever a path does. On Windows other archive types (7z, rar, tar...) keep their usual program but list this app under "Open with". If a `.zip` still opens in File Explorer, you once chose a program for it in Windows' own settings; change it under Settings > Apps > Default apps > .zip.

**On a phone** there is no single switch. Android offers the app whenever another app opens a folder, taps "Open downloads", or opens a file with "Open with"; choose it there and tap "Always". The settings page has a button to the system page where that choice is cleared. A file opened this way is shown in the app's own viewer, with its folder behind it. iPhone and iPad do not let any app replace Files; the app is listed under Share > "Open in".

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

## Working in the background

Settings > Performance > *When minimized or in the background* decides how much the app does while you are not looking at it. **Aggressive** (the default) drops to the minimum, one thread, no thumbnails, indexing or preloading, a small cache, slow progress updates, once the window is minimized or hidden, or has been behind other windows for a few seconds; animations and timers stop too. **Gentle** halves the work and only acts while the window is minimized or hidden. **Off** keeps working as usual. File transfers that are running are never slowed; only how often they report changes. When you come back, everything returns at once.

Performance presets set this too (the heaviest ones use Gentle, the rest Aggressive). As soon as you change it yourself, *Keep my background setting when I apply a preset* turns on and presets leave it alone. Turn that off to let presets set it again.

## Updates

Settings → Updates (also asked on first run, everything on by default). The app looks in two places and offers the newer of what it finds: the repository's **Releases** page (the installer for your kind of computer, no sign-in needed) and the builds of its **GitHub Actions** workflow. Releases are compared by commit when the release names one, otherwise by version number; workflow builds by the commit they were built from. When both have the same build, the release is used because it needs no token.

* **Checking.** When the app opens and then every few hours while it stays open (not while it is minimized with the background saver on). *Check now* checks at once.
* **Telling you.** A short note appears in the notifications once for each new build, then never again until the next one.
* **Downloading.** With *Download new builds automatically* the build downloads as a job with your other transfers (cancel it there like any transfer). Otherwise press *Update now*.
* **Installing.** No installer screens: on Windows the setup program runs silently after the app closes and the app starts again; an AppImage is replaced in place and restarted. For `.deb`, `.rpm` and macOS `.dmg` the system's own installer opens, because a program cannot install those silently without administrator rights. When a download finishes you are asked to **restart now** or **later**; if you wait, *Restart to finish updating* stays in Settings → Updates and you are asked again next time the app opens.
* **What counts as newer.** A release is newer when its version number is higher (and it was not published before the running build was made). Any other build is newer when it is a different build **and** dated after the moment the running build was made (the date is built into the app). An older release or an old kept workflow run is never offered, which stops the app from "updating" back and forth between two builds.
* **Releases only by default.** The app looks only at the repository's Releases, which download straight from GitHub. Builds from the automated runs ("unreleased", fetched through nightly.link unless you give a token) are used only when *Also offer unreleased builds* is on.
* **GitHub's rule.** Releases download without signing in. GitHub lets only signed-in accounts download a build from its Actions page, even for a public repository, so for those either paste an access token that may read Actions into *GitHub access token* (it stays on this computer and is never part of exports or profiles), or install [nightly.link](https://nightly.link) on the repository, which the app uses when no token is set. A build made by hand (not by the workflow) does not know which commit it came from, so it offers the newest build without comparing.

## Presets and profiles

* **Interface style** (Settings > Presets and profiles) rearranges the window the way that system's file manager is arranged, using the same parts: for macOS a Finder-style toolbar with the window buttons, back and forward, the folder name, view, sort and share buttons and search along the top, a Favorites and Locations sidebar, no tab strip until there are two tabs, and a path bar at the bottom; for Linux a header bar with joined back and forward buttons, a pill-shaped path bar, view and menu buttons and search, and a Places sidebar. It also sets the layout, colors, icons, corner shapes, the side of the window buttons and the keyboard shortcuts to match Windows 11 File Explorer, macOS Finder or a GNOME/KDE file manager. Colors and icons are open-source (Fluent System Icons, MIT); no Windows or Apple files are used.
* **Performance presets**, from Barebones to God-like, set threads, cache size, indexing, thumbnails, folder sizes and what is saved. Each one lists the kind of device it suits, and the one recommended for this computer is marked. The page shows them as a **slider** (drag from Light to Heavy, read what the step under the thumb does, then choose **Use**) or as **cards**; the Slider / Cards switch is at the top right of the list. On a phone or tablet the list is a separate, much lighter set of six.
* **On a phone or tablet** the interface styles are Google, Samsung and Apple, each arranged like that maker's own file manager. See [MOBILE.md](MOBILE.md).
* **Profiles** are saved copies of all your settings, as many as you like. While one is in use, your changes are saved into it automatically.

## What the app uses

Settings > Performance > *What the app is using* shows live processor, memory and disk use, and what is kept for later (caches on disk, caches in memory). *Give back memory now*, *Clear caches that rebuild themselves* and *Smallest footprint* shrink what the app keeps. They do not compress your files. Settings > Storage lists each cache with its own Clear button and an on/off switch (thumbnails, folder sizes, disk-usage results, folder learning, hardware details, search history). Below the table, **Folders and drives** clears what is saved about one folder or drive, and can switch saving off for it: pick a drive, choose a folder, or type a path, then *Clear what is saved here* or *Stop saving for this place*. A folder covers everything inside it. Nothing is then kept about those places (listings, folder sizes, thumbnails, disk-usage results, folder learning, search entries), and what was kept is cleared at once. Search still works there, but reads the folder each time. *Save again* removes the rule.

Each performance level also sets a **processor limit** and a **memory limit** (Settings > Performance), as a share of all cores and of installed memory. The processor limit caps how many worker threads run at once, and the memory limit caps the caches. Both can be changed by hand, and 0 means no limit. God-like sets no limit at all. These are caps on the app's threads and caches, not a hard limit on processor time: a single thread can still use a full core.

## Locking folders and drives

Right-click a folder or a drive and choose **Lock with passcode…** (the last item). A locked folder cannot be opened, listed, searched, previewed, copied from or deleted inside this app until the passcode is entered; it relocks after the time set in Advanced. Locks are managed under Settings > Privacy and locks, and removing one needs the main password. **This is a privacy screen, not encryption:** other programs and other file managers can still read the files. Use disk or file encryption for that.

## Thumbnails

In the larger icon views (Medium, Large, Extra large) files show a preview of their contents: pictures, the first page of PDFs and Office documents, and a frame from videos. Details, List and Small icons keep the plain icon, and the details pane always shows a preview of the selected file. All of that is in Settings > Thumbnails: each kind of file, each view, sharpness, how many are made at once, network drives (**Automatic** by default: thumbnails are made while the computer is on a cable, Ethernet or fibre, and not on Wi-Fi or mobile data; or Always on / Always off), the largest picture to preview and which frame of a video to show. Windows uses the same preview handlers as Explorer and macOS uses Quick Look. On Linux, PDFs need `pdftoppm` (poppler-utils) and videos need `ffmpegthumbnailer` or `ffmpeg`; without them those files keep their icons. Office documents are previewed only on Windows and macOS. Every performance level that has thumbnails shows them for pictures, documents and videos; videos and documents can be switched off separately in Settings > Thumbnails. Photos taken in portrait are turned the right way up from their camera's orientation tag.

## Built-in terminal (Ctrl+`)

The **Terminal** button on the left, under Servers, shows or hides a terminal inside the window, at the bottom by default. It works like the one in VS Code:

- **Tabs and split panes.** `+` opens a new terminal in the folder you are viewing (or your home folder; Settings > Terminal). The split button puts a second shell beside the first (below it when the terminal is docked left or right). Middle-click or the `x` on a tab closes it.
- **Administrator.** The small arrow next to `+` has *New terminal as administrator*. On Linux and macOS that starts the shell with `sudo` and asks for your password in the terminal. On Windows a window cannot give a terminal inside it more rights than it has itself, so this works inside the window only when the app was started with *Run as administrator*; otherwise the app offers to open a separate administrator terminal window.
- **Clear, close, new.** The broom clears the screen and the kept output. The bin ends the shell and closes it; open a new one with `+`. When a shell ends, its tab stays with a note; press Enter in it to start a new shell.
- **Copy and paste.** Select to copy if you turn that on in Settings > Terminal; Ctrl+Shift+C / Ctrl+Shift+V (Cmd+C / Cmd+V on a Mac) and the right-click menu always work. Ctrl+C copies when text is selected and interrupts the program otherwise. Links open in your browser.
- **Position.** The gear button, and Settings > Layout, move the terminal to the top, bottom, left, right, a floating window or a pop-out. Drag its inner edge to resize it; double-click the edge to reset.
- **It survives closing the app.** Shells run in a small background helper, so they keep running when you close the window (it quits by itself after 20 seconds if nothing is running and nothing is connected). Reopen the app and the same shells are there with their full output. If the helper was stopped (restart, logout), each tab shows what it printed last time, marked *Restored from last time*, and a fresh shell starts in the same folder. Old commands are never run again. Whether the terminal was shown or hidden is remembered too. Turn the restoring off in Settings > Terminal.
- **Pop out when the app closes (off by default).** Settings > Terminal > *When the app closes, open the terminal in its own window* opens each running shell in the computer's own terminal app (Windows console window, Terminal on macOS, the first terminal found on Linux) and leaves it running. The shell is the same one, so a running command continues. The pop-out button on the terminal does the same on demand.

Settings > Terminal also has the shell to use (blank = your default), font size, scrollback, cursor and blinking.

## File viewer (F3) and quick look (Space)

**F3** opens the selected file in the viewer, **Space** opens a quick look at it (turn off "Select with checkboxes" if Space ticks items instead; both keys can be changed under Keyboard shortcuts). The viewer shows any size of file: only the part on screen is read. Modes: **Text** (T), **Hex** (H), **Binary** (B) and **Preview** (I) for pictures, audio, video, PDF and HTML pages. Choose the encoding (automatic, UTF-8, Windows ANSI, DOS/OEM, UTF-16, Cyrillic, Japanese, Chinese, Korean…), the bytes per row (the "line width" of hex and binary), the text size (+ and −) and whether lines wrap. **Ctrl+F** searches (F3 and Shift+F3 for next and previous, also in hex and binary), **Ctrl+G** goes to an offset. N and P (or the arrow keys in a quick look) go to the next and previous file of the folder. Files inside archives open too; files on servers must be downloaded first. In hex, click a byte to see its value.

Settings > Advanced > Files and lists > "Quick look (space bar) uses" can hand the quick look to QuickLook or Seer (Windows) or the system preview (macOS Quick Look, GNOME Sushi). If the program you choose is not installed the built-in viewer opens. F3 always uses the built-in viewer.

## Select group, restore selection

On the number pad, **+** selects a group of items, **−** unselects a group, **\*** inverts the selection and **/** restores the selection you had before it was last cleared or replaced. The group dialog takes wildcards (`*.jpg; img_??.png | thumb*` where names after `|` are left out), or a regular expression, and can limit by size, date and text inside the files. The same entries are in the **…** menu.

## Command line and your own commands

Turn on **Show the command line under the file list** (Settings > Advanced > Interface), or press Ctrl+Shift+;. Type a command and press Enter: it is typed into a new tab of the built-in terminal, started in the folder you are viewing. **Ctrl+Enter** adds the name of the selected item to the line, **Ctrl+Shift+Enter** its full path; the up and down arrows recall earlier lines.

Settings > Advanced > **User commands** defines commands for a button bar above the file list and a **Commands** menu. In the program, parameters and start folder you can use: `%P` the folder you are viewing (with a closing slash), `%p` without it, `%N` the item under the cursor, `%n` its name without extension, `%E` its extension, `%F` its full path, `%S` the names of the selected items, `%L` their full paths, `%D` the folder's name, `%%` a percent sign. Names with spaces are quoted for you. Programs are started directly, never through a shell; choose "In a new terminal tab" for console programs. (The older "Your own right-click actions" are unchanged.)

## File tools

The **…** menu and the right-click menu have a **File tools** entry (Alt+F7 opens the search):

- **Compare files**: side-by-side lines with changes, additions and removals marked; options to ignore white space and case; binary files report the first byte that differs.
- **Compare / synchronize folders**: either side can be a zip (or 7z, rar, tar…) file: it is unpacked to a temporary folder and compared like a folder (zip files keep the dates stored inside; other archive kinds compare by contents). Select a folder and an archive, or an archive alone, and open the tool. Synchronizing out of an archive copies the unpacked files; synchronizing into one adds them to the archive. It lists what is only on one side, newer on one side or different (optionally by contents) and copies the ticked items left to right, right to left or both ways. Copying uses the normal transfers (conflicts and the queue work as usual).
- **Duplicates**: by content (checked by size, then the first 64 KB, then a SHA-256 of the whole file), by name and size, or by size; tick all but the newest, oldest or first and delete the rest (the Recycle Bin settings apply).
- **Split / combine**: parts named `name.001`, `name.002`… plus `name.crc`, which checks them when combining.
- **Encode / decode**: UUE, XXE and MIME (base64). Decoding finds the encoded files inside mail text.
- **Convert pictures**: JPEG, PNG, WebP, BMP, GIF; quality, scale by percent or fit to a size, name suffix, other folder. It never replaces the original.
- **Search in files**: text or regular expression, file name masks, several places at once (`C:\ ; D:\`), and inside zip, docx, xlsx, pptx, odt, epub, jar and apk files. Double-click a match to open it.

These work on files on this computer, not on servers.

## Second pane, Miller columns, to-do lists

In the View menu: **Second pane** shows another folder beside the file list (F5 copies and F6 moves the selection of the pane you are in to the other pane's folder, Tab switches panes, F3 views), and **Miller columns** shows one column per folder level with a preview of the selected file at the end (arrow keys move, Enter goes there). Both are off by default and the normal file list is unchanged. **To-do for this folder** in the **…** menu keeps a checklist and notes for a folder on this computer. The "already exists" question now shows both files with a picture, size and date and says which is newer or larger.

## Servers through a proxy, and resuming FTP downloads

Settings > Advanced > Transfers > "Connect to FTP and SFTP servers through a proxy" takes `socks5://host:1080`, `socks5://user:password@host:1080`, `socks4://host:1080` or `http://host:8080` (HTTP CONNECT). FTP sends the control and every data connection through it. An FTP download that is cut off by the connection leaves `name.<size>.fastpart` next to the target and the next download of the file continues from there; stopping a download yourself removes the part. FTPS and server-to-server (FXP) copies are not supported; copy through a folder on this computer.

## Terminal

**Inside a zip or other archive** there is no folder a terminal could open in, so the terminal menu changes: *Unpack next to the archive, then open terminal*, *Unpack into this folder, then open terminal* and *Unpack, then open as administrator*. The archive is unpacked as a job with your other transfers; when it is done the terminal opens in the unpacked copy of the folder you were looking at. The right-click menu says the same ("Unpack, then open terminal here"), and an archive file itself has *Unpack, then open terminal in it*. On a server, where a terminal cannot open either, the terminal entries are left out.

Settings > Terminal chooses the terminal (on Windows Command Prompt, PowerShell, PowerShell 7, Windows Terminal, WSL or Git Bash), the Windows Terminal profile, a command to run when it opens (for example `git status`), whether to open a tab in the window that is already open, which folder the Open terminal button uses (the one being viewed or your home folder), whether a single selected folder is used, the shell to run on Linux and macOS, and a custom command with `{path}` for the folder. A custom command replaces the other choices. The startup command is not used for administrator terminals. On macOS the startup command works with Terminal and iTerm only.

## Opening files with no app

Opening a file goes in this order: the app set for that kind of file, then whatever the system itself does with it (the system's own choice is trusted, including Store apps such as Photos), and only if neither works the app picks the most likely installed app for the file type, counting programs that say they can open it (an image viewer for a picture, a media player for a video, and so on for the common types). Only when none is found does it show a chooser (Default app, Suggested apps, More options, Always / Just once), or the system's own chooser on Windows, or only a message. Choose which in Advanced > *When no app is set to open a file*. *Open with…* always shows the chooser.

## Settings

Settings (Ctrl+,) has a search box at the top. Each change applies and saves at once, unless you turn that off (Advanced > Interface > "Save changes automatically"): then a **Save** bar appears at the bottom with Save and Discard, so a set of changes can be made and applied together. Sections: General, Presets and profiles, Appearance, Themes, Layout, Navigation pane, Performance, Storage, Privacy and locks, File transfers, Search, Notifications, Right-click menu, Terminal, Advanced (keyboard shortcuts, your own right-click actions, maintenance), Documentation and About.

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

**Selection box.** Press and drag on empty space in a folder (beside the items in Details view, between the tiles in the icon views) to draw a box; everything it touches is selected, as in Windows Explorer. The list scrolls by itself when you hold the pointer near or past its top or bottom edge, and the box keeps growing. Ctrl toggles what the box touches, Shift adds to what was already selected, and Escape cancels.

The selection box also works in search results. Under Settings > Files and lists you can turn it off, choose whether it follows the accent colour or use your own fill colour, fill strength, border colour, width (0 for none) and style, turn the automatic scrolling off, and set how close to the edge scrolling starts and how fast it goes.


## No flashing console windows (Windows)

Everything the app starts for itself (registry lookups, PowerShell, ffmpeg, winget, the sharing tunnel, administrator helpers) runs with its console window hidden, so no command prompt flashes while files and folders open.

## Files appear as they load

Folders no longer make you wait for the last item. Each batch of files and folders is shown the moment it is read, and while the rest are still coming the details pane says **Still loading this folder**, how many items are in so far, and a time estimate for the rest. The status bar says the count is partial until it finishes. This is on for every folder (including phones, servers and drives); turn it off under Settings > Files and lists > "Show files as they load" to see everything at once instead.

## Thumbnails: priority, servers and drives

Advanced > Thumbnails has a **priority** from 1 (stay out of the way) to 5 (load them as fast as possible) that decides how many are made at once and how long the app waits between them. Videos on a network drive are previewed by making a local copy of the start of the file when the network is fast enough (the size limit is a setting); on FTP, SFTP and WebDAV servers the first part of the file is fetched and a frame taken from it. On Windows, a video that Explorer cannot preview falls back to ffmpeg when it is installed.

## Drive health

If the drive-health card cannot read a drive because smartmontools is missing, it says so and offers an **Install** button (winget on Windows, Homebrew on macOS, your package manager on Linux, each through the system's own prompt), then reads the drive.

## Thumbnails on phones and cameras

Photos and videos on a phone or camera connected by cable show the preview picture the device keeps for them, and follow the same Thumbnails settings as any folder (per kind, per view, size, sharpness). Two things pause them: the switch "Thumbnails for phones and cameras on a cable" (Settings > Thumbnails) and, by default, low battery (computer on battery below 20%, switchable). Where the system does not report battery state, previews keep loading. Files for which the device has no preview keep their plain icon.

## Folder icons, thumbnails and speed (0.9.0)

- **Full folders look full.** A folder with files in it (looked for a few levels down) shows papers sticking out, like Windows; an empty one keeps the plain icon. Settings → Advanced → "Show which folders have files in them" chooses: only real files (default), a folder holding just another folder counts as full, or off.
- **Thumbnails in high performance.** With the resource budget on Full (or Manual), medium, large and extra-large views always show thumbnails, whatever the per-view switches say ("Always show thumbnails in high performance"). Tiles that are on screen are no longer dropped from memory, and a thumbnail that failed is retried after a short while.
- **Video and document thumbnails** are made once at a few fixed sizes, saved in the thumbnail cache (needs "Keep thumbnails on disk"), and made in their own lane so they do not hold up pictures. "Prepare video and document thumbnails ahead" (Auto = on for Full/Manual, On, Off) makes the ones in the open folder in the background.
- **Opening files** asks the registry directly for the program that opens them instead of starting helper programs, so it is immediate.

## Crash logs (0.10.0)

If the app stops unexpectedly, it writes a plain text report: the computer (system, processor, memory, WebView version), what the window looked like (tabs, folder, view, selection, running transfers), commands that were still running, and a trail of the last ~250 events (button clicks by name, shortcut keys, every command that ran and how long it took, drags). A panic also records the error, where it happened and a backtrace. A crash the app cannot catch itself (a driver or system library failing, the program being killed) is found at the next start, because the "running" marker was not removed, and the report is made from the saved trail. Your user name and e-mail addresses are removed; the computer's name is never written.

Find them in Settings > Storage > Crash logs: Copy, Save as text file, or Send to GitHub issues. The Clear button on the Crash logs row deletes them (Clear all leaves them alone). Settings > Advanced > Crash logs: send reports automatically at the next start (off by default), and keep the trail of clicks and keys (on). Files are in the `crash-logs` folder next to settings.json.

**Fast stop (0.10.5).** A panic writes its report first and only then waits up to 1.5 seconds for the backtrace names. A crash inside Windows itself is caught by an exception handler that writes the report (exception code, address, last events) and ends the process at once, skipping Windows' slow "has stopped working" dialog. A frozen window is found by a watchdog: the page reports in every second, and if it is silent for 15 seconds while the window is visible, the report is saved and the app closes (Settings > Advanced > Crash logs: turn that off, or change the seconds). Computer information is gathered once in the background so a report does not wait for it. Dragging files out of the window first asks Windows whether it can describe the item (using the same real path the drag will use, which for a mapped network drive is the share behind it) and says so instead of crashing when it cannot.
