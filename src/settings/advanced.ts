// Advanced options. Each has a default here; only values the person changed
// are saved (settings.advanced). Every key listed is read somewhere in the
// app: the "Used by" note says where, so none of them is decoration.

import type { Settings } from "../api";

export type AdvControl =
  | { type: "toggle" }
  | { type: "number"; min: number; max: number; step?: number; unit?: string }
  | { type: "select"; options: { value: string; label: string }[] };

export interface AdvDef {
  key: string;
  group: string;
  label: string;
  description?: string;
  control: AdvControl;
  default: boolean | number | string;
}

const yn = (def: boolean, key: string, group: string, label: string, description?: string): AdvDef => ({
  key, group, label, description, control: { type: "toggle" }, default: def,
});
const num = (def: number, key: string, group: string, label: string, min: number, max: number, unit?: string, description?: string, step?: number): AdvDef => ({
  key, group, label, description, control: { type: "number", min, max, unit, step }, default: def,
});
const sel = (def: string, key: string, group: string, label: string, options: [string, string][], description?: string): AdvDef => ({
  key, group, label, description, control: { type: "select", options: options.map(([value, l]) => ({ value, label: l })) }, default: def,
});

export const ADVANCED_GROUPS = ["Privacy and caches", "Interface", "Files and lists", "Tabs", "Transfers", "Search", "Notifications", "Navigation pane", "Speed and memory", "Thumbnails"] as const;

export const ADVANCED: AdvDef[] = [
  // ---- Interface ----
  sel("auto", "ui.language", "Interface", "Language", [["auto", "Automatic (system language)"], ["en", "English"], ["es", "Español"], ["fr", "Français"], ["de", "Deutsch"], ["pt", "Português"], ["it", "Italiano"], ["ru", "Русский"], ["zh", "简体中文"], ["ja", "日本語"], ["ko", "한국어"]], "Used by the setup wizard and the labels that are translated so far; anything else stays in English."),
  sel("windows", "ui.chrome", "Interface", "Window arrangement", [["windows", "Windows (File Explorer)"], ["macos", "macOS (Finder)"], ["linux", "Linux (GNOME Files)"]], "How the same tabs, address bar, commands and navigation pane are arranged. The interface style sets this for you."),
  yn(true, "ui.animations", "Interface", "Animations", "Turn off for instant, motion-free changes."),
  num(100, "ui.scale", "Interface", "Interface zoom", 60, 200, "%", "Makes everything larger or smaller.", 5),
  num(0, "ui.rowHeight", "Interface", "Row height", 0, 60, "px", "0 follows the density setting."),
  yn(false, "ui.zebra", "Interface", "Shade every other row"),
  yn(false, "ui.gridLines", "Interface", "Show lines between rows"),
  yn(true, "ui.focusRing", "Interface", "Show the keyboard focus outline"),
  sel("thin", "ui.scrollbars", "Interface", "Scroll bars", [["thin", "Thin"], ["normal", "Standard"], ["hidden", "Hidden until needed"]]),
  num(24, "ui.selectionOpacity", "Interface", "Selection highlight strength", 5, 80, "%"),
  yn(true, "ui.statusBar", "Interface", "Show the status bar under the file list"),
  sel("right", "ui.windowControlsSide", "Interface", "Window buttons (minimize, maximize, close)", [["right", "On the right (Windows, GNOME)"], ["left", "On the left (macOS style)"]], "On macOS the system's own buttons are always on the left."),
  yn(true, "ui.dimHidden", "Interface", "Fade hidden files and folders"),

  // ---- Privacy and caches ----
  num(15, "locks.relockMin", "Privacy and caches", "A passcode stays unlocked for", 0, 1440, "min", "Then it asks again. 0 keeps it unlocked until the app closes. Passcodes only lock folders and drives inside this app; other programs are not affected."),
  yn(true, "cache.thumbnails", "Privacy and caches", "Keep thumbnails on disk", "Off: thumbnails are made each time and never saved."),
  num(512, "cache.thumbnailsMaxMb", "Privacy and caches", "Thumbnail cache size limit", 0, 20000, "MB", "The oldest thumbnails are removed when it grows past this. 0 means no limit."),
  yn(true, "cache.usageSnapshots", "Privacy and caches", "Keep disk-usage results", "The last scan of each analysed folder is shown at once next time. Off: nothing is saved."),
  yn(true, "cache.predictions", "Privacy and caches", "Learn which folders you open next", "Used to prepare folders early. Off: nothing is recorded."),
  yn(true, "cache.hardware", "Privacy and caches", "Save this computer's hardware details", "About opens at once from the saved copy instead of reading the hardware and drives every time."),
  num(24, "cache.hardwareMaxAgeHours", "Privacy and caches", "Saved hardware details count as current for", 0, 8760, "hours", "After this About reads them again. 0 never expires them (use Read again now)."),
  yn(false, "perf.compactWhenIdle", "Speed and memory", "Give back memory when the window is in the background", "Drops cached folder listings and returns free memory to the system two minutes after you switch away."),

  // ---- Files and lists ----
  yn(true, "files.folderSizes", "Files and lists", "Show folder sizes", "Measured in the background from what is inside each folder, newest folder first, and kept for two minutes."),
  yn(true, "files.keepSelectionOnDrag", "Files and lists", "Keep several selected items selected while dragging", "Pressing on one of them no longer narrows the selection to that one until you let go without dragging. Cancelling a drag keeps the selection too."),
  num(20, "search.historyLimit", "Search", "Previous searches to remember", 0, 200, undefined, "Shown under the search box, newest first. 0 turns the history off and forgets the list."),
  yn(true, "files.sizeCache", "Files and lists", "Remember folder sizes", "Saved on disk and shown the moment a folder opens, then checked again in the background. Turn off to measure every time and keep nothing."),
  num(15, "files.sizeCacheFreshMin", "Files and lists", "A remembered folder size counts as current for", 0, 10080, "min", "Older sizes are still shown first, then measured again quietly. 0 measures every time."),
  num(20000, "files.sizeCacheMax", "Files and lists", "Folder sizes to remember", 100, 500000, undefined, "The oldest are forgotten first."),
  sel("chooser", "files.noAppAction", "Files and lists", "When no app is set to open a file", [["chooser", "Ask with this app's own chooser"], ["system", "Ask with the system's chooser (Windows; other systems use this app's)"], ["message", "Only say so"]], "Applies when a file type has no program, or its program is gone. Right-click > Open with always shows the chooser."),
  yn(false, "files.singleClickOpen", "Files and lists", "Open items with a single click"),
  yn(false, "files.foldersInNewTab", "Files and lists", "Open folders in a new tab"),
  sel("newTab", "files.middleClick", "Files and lists", "Middle-clicking a folder", [["newTab", "Opens it in a background tab"], ["nothing", "Does nothing"]]),
  sel("decimal", "files.sizeUnits", "Files and lists", "Size units", [["decimal", "KB, MB, GB: 1 MB = 1,000,000 bytes (as on a drive's label)"], ["binary", "KiB, MiB, GiB: 1 MiB = 1,048,576 bytes"], ["windows", "KB, MB, GB, but counted in 1,024s (what Windows Explorer shows)"]]),
  sel("system", "files.dateFormat", "Files and lists", "Dates", [["system", "Match my system"], ["iso", "2026-11-11 19:42"], ["relative", "Today, Yesterday, 3 days ago"]]),
  sel("auto", "files.clock", "Files and lists", "Clock", [["auto", "Match my system"], ["12", "12-hour"], ["24", "24-hour"]]),
  yn(true, "files.openArchives", "Files and lists", "Open archives like folders", "Double-clicking a zip, 7z, rar, tar or similar file shows what is inside it. Items can be opened or copied out; nothing inside can be changed. Turn off to open archives with another program instead (Open with… and Extract still work)."),
  yn(true, "files.rememberSort", "Files and lists", "Use the same sort order in every folder", "The column and direction you pick (for example newest first) stay when you open another folder, and are still there next time you start the app. Turn off to start every visit sorted by name."),
  sel("name", "files.sortKey", "Files and lists", "Sorted by", [["name", "Name"], ["modified", "Date modified"], ["created", "Date created"], ["type", "Type"], ["size", "Size"], ["extension", "Extension"]], "Changes when you click a column heading or use Sort."),
  yn(true, "files.sortAsc", "Files and lists", "Sort ascending", "A to Z, oldest first, smallest first. Off reverses it."),
  yn(true, "files.keepFilter", "Files and lists", "Use the same filter in every folder", "What is typed or ticked in the Filter menu stays when you open another folder, until you clear it. The Filter button shows a dot while one is on. Turn off to start every folder unfiltered."),
  yn(true, "files.rememberView", "Files and lists", "Use the same view in every folder", "The view you pick in the View menu (Details, List, icons of any size) stays when you open another folder, and is still there next time you start the app. Turn off to have a view you pick apply only to the folder you are in; every other folder opens in the view chosen under Appearance > Folder view."),
  yn(true, "files.sortAfterJobs", "Files and lists", "Sort and filter when a job finishes", "When copying, moving or deleting is done, the folders it touched are reloaded and the current sort and filter are applied straight away, so new files land in their sorted place and hidden ones stay hidden. Turn off to leave folders as they are until you refresh."),
  yn(true, "files.naturalSort", "Files and lists", "Sort numbers naturally", "file2 comes before file10."),
  yn(false, "files.caseSensitive", "Files and lists", "Sort capital letters first"),
  num(700, "files.typeAheadMs", "Files and lists", "Type-to-select pause", 200, 3000, "ms", "How long typing keeps extending what you typed."),
  yn(true, "files.wheelZoom", "Files and lists", "Ctrl + scroll changes the view"),
  num(3, "files.confirmDeleteOver", "Files and lists", "Always confirm deleting this many items or more", 0, 1000, "items", "Even when \"Ask before deleting\" is off. 0 never forces it."),
  num(100, "files.recentLimit", "Files and lists", "Recent items to remember", 0, 1000),
  num(10, "files.frequentCount", "Navigation pane", "Frequently used folders to keep in the list", 0, 50, undefined, "The list is built from what you open most."),
  sel("frequent,pinned,drives", "sidebar.order", "Navigation pane", "Order of the sections", [
    ["frequent,pinned,drives", "Frequently used, Pinned, Drives"],
    ["pinned,frequent,drives", "Pinned, Frequently used, Drives"],
    ["pinned,drives,frequent", "Pinned, Drives, Frequently used"],
    ["frequent,drives,pinned", "Frequently used, Drives, Pinned"],
    ["drives,pinned,frequent", "Drives, Pinned, Frequently used"],
    ["drives,frequent,pinned", "Drives, Frequently used, Pinned"],
  ], "You can also drag a section's heading to move it."),
  sel("always", "sidebar.scrollbar", "Navigation pane", "Scroll bar in the left pane", [["always", "Always show"], ["hover", "Show when the pointer is over it"], ["hidden", "Hide completely (still scrolls)"]]),
  num(6, "sidebar.frequentRows", "Navigation pane", "Frequently used: rows shown before it scrolls", 0, 50, undefined, "0 shows them all. Longer lists get their own scroll bar so the rest of the pane stays in view."),
  num(10, "sidebar.pinnedRows", "Navigation pane", "Pinned: rows shown before it scrolls", 0, 100, undefined, "0 shows them all."),
  num(10, "sidebar.drivesRows", "Navigation pane", "Drives: rows shown before it scrolls", 0, 100, undefined, "0 shows them all."),
  num(20, "files.undoLimit", "Files and lists", "Actions Undo can reverse", 0, 200),

  // ---- Tabs ----
  num(200, "tabs.width", "Tabs", "Tab width", 90, 360, "px", "The width tabs have while there is room."),
  num(120, "tabs.minWidth", "Tabs", "Narrowest a tab gets", 40, 240, "px", "Past this, tabs stop shrinking and the bar scrolls with arrows."),
  num(34, "tabs.height", "Tabs", "Tab bar height", 24, 64, "px"),
  num(25, "tabs.reopenKeep", "Tabs", "Closed tabs that can be brought back", 0, 200, undefined, "Ctrl+Shift+T brings back the last one closed, with its back and forward history. 0 turns it off."),
  num(0, "tabs.max", "Tabs", "Most tabs open at once", 0, 100, undefined, "0 means no limit."),
  sel("end", "tabs.newPosition", "Tabs", "New tabs open", [["end", "At the end"], ["afterCurrent", "Next to the current tab"]]),
  sel("hover", "tabs.closeButton", "Tabs", "Tab close button", [["hover", "On hover and on the active tab"], ["always", "Always"], ["never", "Never (use middle-click)"]]),

  // ---- Notifications ----
  yn(true, "notify.enabled", "Notifications", "Show a notice when an action is done", "Copy, cut, move, delete, rename, create and undo are reported in a corner of the window."),
  sel("bottom-right", "notify.position", "Notifications", "Where notices appear", [["bottom-right", "Bottom right"], ["bottom-left", "Bottom left"], ["top-right", "Top right"], ["top-left", "Top left"]]),
  sel("popup", "notify.mode", "Notifications", "How notices appear", [["popup", "Pop up for a few seconds"], ["bell", "Start hidden: a small bell, open it to read them"]]),
  sel("normal", "notify.size", "Notifications", "Notice size", [["small", "Small"], ["normal", "Normal"], ["large", "Large"], ["xlarge", "Extra large"]]),
  yn(false, "windows.rememberDetached", "General", "Remember detached windows", "Off: Settings or Transfers that you moved into their own window come back inside the main window next time, where they were before. On: they reopen detached."),
  num(5, "notify.seconds", "Notifications", "Pop-ups stay for", 1, 60, "sec"),
  num(30, "notify.keep", "Notifications", "Notices kept in the list", 5, 200),
  yn(false, "notify.sound", "Notifications", "Play a soft beep with each notice"),

  // ---- Transfers ----
  num(4, "transfers.dismissSecs", "Transfers", "Hide a finished transfer after", 0, 120, "sec", "0 keeps it until you close it."),
  yn(false, "transfers.openWindow", "Transfers", "Open the larger transfer window automatically"),

  // ---- Search ----
  num(140, "search.debounceMs", "Search", "Wait after typing before searching", 0, 1500, "ms"),

  // ---- Thumbnails ----
  yn(true, "thumbs.images", "Thumbnails", "Pictures", "Show the picture itself instead of a type icon: JPEG, PNG, GIF, WebP and BMP."),
  yn(true, "thumbs.documents", "Thumbnails", "Documents", "Show the first page of PDFs and Office documents. Windows and macOS use their own previews; on Linux this needs pdftoppm (PDF only)."),
  yn(true, "thumbs.videos", "Thumbnails", "Videos", "Show a frame from the video. Windows and macOS use their own previews; on Linux this needs ffmpegthumbnailer or ffmpeg."),
  yn(false, "thumbs.inDetails", "Thumbnails", "Show in the Details view", "Replaces the small type icon at the start of each row."),
  yn(false, "thumbs.inList", "Thumbnails", "Show in the List view"),
  yn(false, "thumbs.inSmall", "Thumbnails", "Show in the Small icons view"),
  yn(true, "thumbs.inMedium", "Thumbnails", "Show in the Medium icons view"),
  yn(true, "thumbs.inLarge", "Thumbnails", "Show in the Large icons view"),
  yn(true, "thumbs.inXl", "Thumbnails", "Show in the Extra large icons view"),
  yn(true, "thumbs.inPane", "Thumbnails", "Show in the details pane", "The preview at the top of the details pane for the selected file."),
  sel("normal", "thumbs.quality", "Thumbnails", "Sharpness", [["draft", "Draft (smaller, fastest)"], ["normal", "Normal"], ["sharp", "Sharp (twice the pixels, for high-resolution screens)"]], "Sharper thumbnails take longer to make and more space to keep."),
  num(0, "thumbs.parallel", "Thumbnails", "Made at the same time", 0, 8, undefined, "How many thumbnails are made at once. 0 lets the performance level decide."),
  sel("auto", "thumbs.networkDrives", "Thumbnails", "On network drives", [["auto", "Automatic (on when wired)"], ["on", "Always on"], ["off", "Always off"]], "Showing thumbnails of files on network shares means reading many files over the network. Automatic checks the connection: on a cable (Ethernet or fibre) thumbnails are always made, and on Wi-Fi or mobile data they are not."),
  num(64, "thumbs.maxImageMb", "Thumbnails", "Largest picture to preview", 0, 2048, "MB", "Bigger pictures keep their icon. 0 means no limit."),
  num(10, "thumbs.videoPercent", "Thumbnails", "Video frame to show", 0, 95, "%", "How far into the video the frame is taken. With ffmpeg alone (no ffmpegthumbnailer) the frame at two seconds is used."),

  // ---- Drives ----
  yn(true, "drives.watch", "Files and lists", "Notice drives being connected or removed", "Checks every second or two for USB drives, cards, discs and network drives coming and going, and updates This PC and the navigation pane by itself."),
  num(1500, "drives.pollMs", "Files and lists", "How often to check for drives", 250, 60000, "ms", "A quick check that costs almost nothing; the drives are only listed again when it finds a change."),
  num(10, "drives.refreshSecs", "Files and lists", "Refresh free space every", 0, 3600, "s", "How often the used and free figures of the drives are read again. 0 only updates when a drive is connected or removed."),
  yn(true, "drives.notify", "Files and lists", "Show a notice when a drive is connected or removed"),

  yn(true, "devices.show", "Files and lists", "Show phones, cameras and other devices", "Lists connected phones, cameras and other devices in This PC and the navigation pane, next to the drives. Windows: portable devices; macOS and Linux: USB devices."),
  yn(true, "devices.showOther", "Files and lists", "Also show printers, keyboards and other USB devices", "Off: only phones and cameras are listed."),

  // ---- Disk usage ----
  sel("auto", "usage.liveActivity", "Files and lists", "Live drive activity in Disk usage", [["auto", "Automatic (only on computers with power to spare)"], ["on", "Always"], ["off", "Never"]], "While a scan runs, draws how busy the drive is and how many files per second it is finding. Automatic needs 8 or more cores, 8 GB of memory and mains power."),

  // ---- Speed and memory ----
  num(70, "perf.prefetchDelayMs", "Speed and memory", "Pointer rest before preloading a folder", 0, 1000, "ms"),
  num(90, "perf.thumbDelayMs", "Speed and memory", "Wait before making a thumbnail", 0, 1000, "ms", "Scrolling fast skips the work."),
  num(10, "perf.overscanRows", "Speed and memory", "Extra rows drawn beyond the screen", 2, 100),
];

const BY_KEY = new Map(ADVANCED.map((d) => [d.key, d]));

/** An advanced option's current value (the saved one, else its default). */
export function adv<T extends boolean | number | string>(settings: Settings | null | undefined, key: string): T {
  const saved = settings?.advanced?.[key];
  const def = BY_KEY.get(key)?.default as T;
  return (saved === undefined || typeof saved !== typeof def ? def : saved) as T;
}
