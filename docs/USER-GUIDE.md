# User guide

## Getting around

**Tabs.** Ctrl+T opens a tab, Ctrl+W closes it, Ctrl+Tab and Ctrl+Shift+Tab switch. Tabs start wide and shrink together as you open more; past the minimum width the strip scrolls. Drag a tab out of the strip to make it its own window. Drag a tab onto another window's tab bar to move it there. A window made from a tab has no Back button. Tabs can be grouped with names and colours, and are reopened at launch (Settings → General).

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

## Network drives

Mapped drives and shares show under Drives with their drive letter. If you run the app as administrator, drives mapped in your normal session are still listed (they are found through your user's saved mappings).

## Running as administrator

The app does not need it. When a transfer is refused for permissions it offers **Retry as administrator**, which runs only that job elevated.

## Settings

Settings (Ctrl+,) has a search box at the top. Each change applies and saves at once. Sections: General, Appearance, Themes, Layout, Navigation pane, Performance, Storage, File transfers, Search, Notifications, Right-click menu, Terminal, Advanced (keyboard shortcuts, your own right-click actions, maintenance), Documentation and About.

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
