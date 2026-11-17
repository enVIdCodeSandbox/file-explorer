// Advanced options. Each has a default here; only values the person changed
// are saved (settings.advanced). Every key listed is read somewhere in the
// app: the "Used by" note says where, so none of them is decoration.

import type { Settings } from "../api";
import { TRANSFER_METRICS } from "./transferMetrics";

export type AdvControl =
  | { type: "toggle" }
  | { type: "number"; min: number; max: number; step?: number; unit?: string }
  | { type: "select"; options: { value: string; label: string }[] }
  | { type: "color" }
  | { type: "text"; placeholder?: string }
  | { type: "methods" };

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

const text = (def: string, key: string, group: string, label: string, placeholder?: string, description?: string): AdvDef => ({
  key, group, label, description, control: { type: "text", placeholder }, default: def,
});

const col = (def: string, key: string, group: string, label: string, description?: string): AdvDef => ({
  key, group, label, description, control: { type: "color" }, default: def,
});

export const ADVANCED_GROUPS = ["Privacy and caches", "Interface", "Files and lists", "Tabs", "Transfers", "Copy window", "Search", "Notifications", "Navigation pane", "Speed and memory", "Thumbnails", "Tasks", "Terminal panel"] as const;

export const ADVANCED: AdvDef[] = [
  // ---- Terminal panel (the built-in terminal; the Terminal section is for the outside one) ----
  yn(false, "terminal.popOutOnClose", "Terminal panel", "Pop the terminal out when the app closes", "When you close the app, each running terminal opens in the system's own terminal app (Windows Terminal or the console on Windows, Terminal on a Mac, your terminal program on Linux) and keeps running, with everything it has printed. Off by default: closing the app then ends the terminals, but what they printed is shown again next time."),
  yn(true, "terminal.restore", "Terminal panel", "Bring back the terminal's tabs and what they printed", "When the app starts again, the terminal panel comes back as it was: the same tabs and splits, showing what each printed. The commands are not run again; each tab gets a fresh shell in the folder it was in. Terminals that are still running (popped out, or after a crash) are reconnected instead."),
  text("", "servers.proxy", "Transfers", "Connect to FTP and SFTP servers through a proxy", "Empty = connect directly", "socks5://host:1080, socks5://user:password@host:1080, socks4://host:1080 or http://host:8080 (HTTP CONNECT). Used for the connection to the server and, for FTP, for every file transfer. Applies to FTP and SFTP; WebDAV and phones are not routed through it. Takes effect for the next connection."),
  text("", "terminal.shell", "Terminal panel", "Shell to run", "Default: PowerShell or Command Prompt on Windows, your login shell on Mac and Linux", "For example pwsh, wsl.exe, or \"C:\\Program Files\\Git\\bin\\bash.exe\" --login -i. Leave empty for the default."),
  sel("folder", "terminal.startIn", "Terminal panel", "New terminals start in", [["folder", "The folder being viewed"], ["home", "Your home folder"]]),
  sel("external", "address.shellWords", "Terminal panel", "Typing cmd, powershell… in the address bar", [["external", "Open the real program (Command Prompt, PowerShell, Terminal) in this folder"], ["builtin", "Open a tab in the built-in terminal in this folder"], ["off", "Do nothing special (treat it as a path)"]], "As in Windows Explorer, typing the name of a shell (cmd, powershell, pwsh, wt, wsl; terminal, bash or zsh elsewhere) and pressing Enter opens it in the folder being viewed. The real program is the one chosen in Settings → Terminal."),
  text("", "terminal.fontFamily", "Terminal panel", "Font", "Default: Cascadia Mono, Consolas…", "The name of a fixed-width font installed on this computer, for example Cascadia Code or JetBrains Mono. Use a fixed-width font: the terminal draws every letter in a cell of the same size, so other fonts look uneven. If the font is not installed, the built-in fixed-width fonts are used. Leave empty for the default."),
  num(13, "terminal.fontSize", "Terminal panel", "Text size", 8, 32, "px"),
  num(5000, "terminal.scrollback", "Terminal panel", "Lines kept to scroll back through", 500, 100000, "lines", "In each terminal, on top of what the terminal host keeps for reconnecting (about 2 MB of output each)."),
  sel("block", "terminal.cursor", "Terminal panel", "Cursor", [["block", "Block"], ["bar", "Bar"], ["underline", "Underline"]]),
  yn(true, "terminal.cursorBlink", "Terminal panel", "Blinking cursor"),
  yn(false, "terminal.copyOnSelect", "Terminal panel", "Copy text when it is selected", "Selecting text copies it, as in many Linux terminals. Ctrl+Shift+C and Ctrl+Shift+V always copy and paste."),
  // ---- Tasks (the rest is in the Tasks window) ----
  yn(true, "tasks.enabled", "Tasks", "Run automated tasks", "Turns every automated task on or off. The tasks keep their settings while this is off. Used by the Tasks window and its scheduler."),
  yn(true, "tasks.showButton", "Tasks", "Show the Tasks button in the toolbar", "The button next to Share that opens the Tasks window. The window can also be opened from the right-click menu of a folder."),
  yn(true, "tasks.trainEstimates", "Tasks", "Let tasks teach the time estimates", "Finished copy, move and sync tasks add what they measured (speed, time per file) to the history behind the time estimates of ordinary transfers."),
  yn(true, "tasks.recordHistory", "Tasks", "Keep a record of every run", "What ran, when, how long it took, how much it moved, what failed, and runs that were cut short by a shutdown or sleep. The Tasks window's statistics and time estimates come from it."),
  num(500, "tasks.historyPerTask", "Tasks", "Runs to remember per task", 10, 10000, undefined, "The oldest records of a task are dropped beyond this."),
  num(365, "tasks.historyDays", "Tasks", "Forget runs older than", 0, 3650, "days", "0 keeps them until the limit above is reached."),
  yn(true, "tasks.notify", "Tasks", "Show a message when a task finishes", "Each task decides which results to tell you about (failures by default). Turn this off to see none."),
  yn(true, "tasks.confirmDelete", "Tasks", "Ask before saving a task that deletes for good", "Tasks that delete without using the trash can are easy to get wrong; this asks once when you save one."),
  yn(false, "tasks.startWithComputer", "Tasks", "Start with the computer (minimized)", "Tasks only run while the app is running. This starts it, minimized, when you sign in to the computer."),

  // ---- Interface ----
  sel("auto", "ui.language", "Interface", "Language", [["auto", "Automatic (system language)"], ["en", "English"], ["es", "Español"], ["fr", "Français"], ["de", "Deutsch"], ["pt", "Português"], ["it", "Italiano"], ["ru", "Русский"], ["zh", "简体中文"], ["ja", "日本語"], ["ko", "한국어"]], "Used by the setup wizard and the labels that are translated so far; anything else stays in English."),
  sel("windows", "ui.chrome", "Interface", "Window arrangement", [["windows", "Windows (File Explorer)"], ["macos", "macOS (Finder)"], ["linux", "Linux (GNOME Files)"], ["google", "Phone: Google (Files by Google)"], ["samsung", "Phone: Samsung (My Files)"], ["apple", "Phone: Apple (Files)"]], "How the same parts are arranged. The interface style sets this for you. The three phone arrangements are used by the phone layout."),
  sel("auto", "ui.phoneLayout", "Interface", "Phone layout", [["auto", "Automatic (on phones and tablets)"], ["on", "Always"], ["off", "Never"]], "The touch layout: one column, a bar along the bottom, press and hold to select. \"Always\" shows it on a computer too, which is a quick way to see it; make the window narrow."),
  sel("slider", "ui.perfPresetView", "Interface", "Performance presets are shown as", [["slider", "A slider"], ["cards", "Cards"]], "Settings > Presets and profiles has a button for this too."),
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
  yn(true, "thumbs.onDevices", "Thumbnails", "Thumbnails for phones and cameras on a cable", "Uses the preview picture the device keeps for each photo and video, and follows the other thumbnail settings. Turn off to show plain icons."),
  yn(true, "thumbs.deviceSaveBattery", "Thumbnails", "Pause device thumbnails on low battery", "While this computer runs on battery below 20%, phones and cameras show icons instead of previews, so the cable is not asked for hundreds of pictures. Not every system reports its battery; where it does not, previews keep loading."),
  yn(true, "ui.autoSave", "Interface", "Save changes automatically", "Changes to the layout, themes and other settings are applied and saved the moment you make them. Turn off to try changes first: they still show at once, but stay unsaved until you press Save at the bottom of Settings (or Discard to go back)."),
  sel("tree", "search.defaultMode", "Search", "Typing in the search box searches", [["level", "Only the folder you are in"], ["tree", "The folder and everything under it"], ["global", "Everywhere"]], "Used when you click the search box and type. Ctrl+F always searches the folder's own level, Ctrl+Shift+F everywhere, and Ctrl+R then F the folder with all its subfolders. The shortcuts for the first two can be changed under Keyboard shortcuts."),
  yn(true, "search.chord", "Search", "Ctrl+R, then F searches the folder and its subfolders", "Press Ctrl+R, then F within a second and a half. Turn off to leave Ctrl+R alone."),
  yn(false, "view.dualPane", "Interface", "Show a second pane beside the file list", "A second folder view on the right (Total Commander style). F5 copies and F6 moves the selection of the pane you are in to the folder of the other one; Tab switches panes. The normal file list stays as it is. Also in the View menu."),
  yn(false, "view.miller", "Interface", "Show folders as columns (Miller columns)", "One column for every folder level with a preview of the selected file at the end (OneCommander and Finder style). Click a folder to open it in the next column; double-click or Enter to go there. Also in the View menu."),
  yn(false, "ui.commandLine", "Interface", "Show the command line under the file list", "Type a command and press Enter: it runs in a new tab of the built-in terminal, started in the folder you are viewing. Ctrl+Enter adds the name of the selected item, Ctrl+Shift+Enter its full path. Up and down recall earlier commands."),
  yn(true, "ui.buttonBar", "Interface", "Show the button bar", "Shows the commands you defined (Settings > User commands) as buttons above the file list, and the Commands menu. Hidden when there is nothing to show."),
  sel("builtin", "files.previewEngine", "Files and lists", "Quick look (space bar) uses", [["builtin", "This app's own viewer"], ["quicklook", "QuickLook (Windows, if installed)"], ["seer", "Seer (Windows, if installed)"], ["system", "The system's preview (macOS Quick Look, GNOME Sushi)"]], "If the program you choose is not installed, the built-in viewer opens instead. F3 always opens the built-in viewer."),
  yn(true, "files.progressiveLoad", "Files and lists", "Show files as they load", "Items appear the moment they are read instead of after the whole folder is. The details pane says the folder is still loading, with a time estimate. Turn off to wait and show everything at once."),
  yn(true, "files.folderSizes", "Files and lists", "Show folder sizes", "Measured in the background from what is inside each folder, newest folder first, and kept for two minutes."),
  yn(true, "files.keepSelectionOnDrag", "Files and lists", "Keep several selected items selected while dragging", "Pressing on one of them no longer narrows the selection to that one until you let go without dragging. Cancelling a drag keeps the selection too."),
  num(20, "search.historyLimit", "Search", "Previous searches to remember", 0, 200, undefined, "Shown under the search box, newest first. 0 turns the history off and forgets the list."),
  yn(true, "files.sizeCache", "Files and lists", "Remember folder sizes", "Kept in memory while the app is open and saved on disk for next time. Shown the moment a folder opens, then checked again in the background. Turn off to measure every time and keep nothing."),
  num(15, "files.sizeCacheFreshMin", "Files and lists", "A remembered folder size counts as current for", 0, 10080, "min", "Older sizes are still shown first, then measured again quietly. 0 measures every time."),
  num(20000, "files.sizeCacheMax", "Files and lists", "Folder sizes to remember", 100, 500000, undefined, "The oldest are forgotten first."),
  sel("chooser", "files.noAppAction", "Files and lists", "When no app is set to open a file", [["chooser", "Ask with this app's own chooser"], ["system", "Ask with the system's chooser (Windows; other systems use this app's)"], ["message", "Only say so"]], "Applies when a file type has no program, or its program is gone. Right-click > Open with always shows the chooser."),
  yn(false, "files.singleClickOpen", "Files and lists", "Open items with a single click", "Files and folders. To do it for folders only, leave this off and turn on the next one."),
  yn(false, "files.singleClickFolders", "Files and lists", "Open folders with a single click", "A single click on a folder opens it; files still open with a double click. Selecting folders is then done with the selection checkboxes (below) or Ctrl + click."),
  yn(false, "files.checkboxes", "Files and lists", "Show selection checkboxes", "A checkbox to the left of each item (and a Select all box in the column heading). Clicking an item or its box ticks it; clicking again unticks it. Ticked items stay ticked when you click elsewhere or on empty space. Shift + click turns over every item between the last click and this one: ticked ones become unticked and the others ticked. Space ticks the item the arrow keys are on. Details view and icon views on computers; not on phones."),
  yn(true, "files.perFolderView", "Files and lists", "Remember sort order, filter and columns for each folder", "Every folder keeps the order, filter and columns you gave it (Details > Choose details). Folders you have not changed use the defaults. Turn off to use one sort order, one filter and one set of columns everywhere, as set by the two options below."),
  yn(false, "files.columnsEverywhere", "Files and lists", "Use the same columns in every folder", "One set of columns, in one order and width, for all folders, even while sort order and filters are kept per folder. Also ticked from Details > Choose details > Use these columns in every folder."),
  yn(false, "files.foldersInNewTab", "Files and lists", "Open folders in a new tab"),
  sel("newTab", "files.middleClick", "Files and lists", "Middle-clicking a folder", [["newTab", "Opens it in a background tab"], ["nothing", "Does nothing"]]),
  sel("decimal", "files.sizeUnits", "Files and lists", "Size units", [["decimal", "KB, MB, GB: 1 MB = 1,000,000 bytes (as on a drive's label)"], ["binary", "KiB, MiB, GiB: 1 MiB = 1,048,576 bytes"], ["windows", "KB, MB, GB, but counted in 1,024s (what Windows Explorer shows)"]]),
  sel("system", "files.dateFormat", "Files and lists", "Dates", [["system", "Match my system"], ["iso", "2026-11-11 19:42"], ["relative", "Today, Yesterday, 3 days ago"]]),
  sel("auto", "files.clock", "Files and lists", "Clock", [["auto", "Match my system"], ["12", "12-hour"], ["24", "24-hour"]]),
  yn(true, "files.openArchives", "Files and lists", "Open archives like folders", "Double-clicking a zip, 7z, rar, tar or similar file shows what is inside it. Items can be opened, copied out or dragged into other programs, and files can be dragged in (zip, tar, 7z and most others; the User guide lists which). Turn off to open archives with another program instead (Open with… and Extract still work)."),
  yn(true, "files.rememberSort", "Files and lists", "Use the same sort order in every folder", "Used only when each folder does not keep its own (above). The column and direction you pick (for example newest first) stay when you open another folder, and are still there next time you start the app. Turn off to start every visit sorted by name."),
  sel("name", "files.sortKey", "Files and lists", "Sorted by", [["name", "Name"], ["modified", "Date modified"], ["created", "Date created"], ["type", "Type"], ["size", "Size"], ["extension", "Extension"]], "Changes when you click a column heading or use Sort."),
  yn(true, "files.sortAsc", "Files and lists", "Sort ascending", "A to Z, oldest first, smallest first. Off reverses it."),
  yn(true, "files.keepFilter", "Files and lists", "Use the same filter in every folder", "Used only when each folder does not keep its own (above). What is typed or ticked in the Filter menu stays when you open another folder, until you clear it. The Filter button shows a dot while one is on. Turn off to start every folder unfiltered."),
  yn(true, "files.rememberView", "Files and lists", "Use the same view in every folder", "The view you pick in the View menu (Details, List, icons of any size) stays when you open another folder, and is still there next time you start the app. Turn off to have a view you pick apply only to the folder you are in; every other folder opens in the view chosen under Appearance > Folder view."),
  yn(true, "files.sortAfterJobs", "Files and lists", "Sort and filter when a job finishes", "When copying, moving or deleting is done, the folders it touched are reloaded and the current sort and filter are applied straight away, so new files land in their sorted place and hidden ones stay hidden. Turn off to leave folders as they are until you refresh."),
  yn(true, "files.selectionBox", "Files and lists", "Selection box", "Drag over empty space in a folder or in search results to select everything the box touches, as in Windows Explorer. Turn off and dragging over empty space only clears the selection."),
  yn(true, "band.followAccent", "Files and lists", "Selection box uses the accent colour", "Turn off to choose the fill and border colours below."),
  col("#0078d4", "band.fillColor", "Files and lists", "Selection box fill colour", "Used when the accent colour is not followed."),
  num(28, "band.fillOpacity", "Files and lists", "Selection box fill strength", 0, 100, "%", "0 leaves the inside clear and shows only the border."),
  col("#0078d4", "band.borderColor", "Files and lists", "Selection box border colour", "Used when the accent colour is not followed."),
  num(1, "band.borderWidth", "Files and lists", "Selection box border width", 0, 8, "px", "0 draws no border."),
  sel("solid", "band.borderStyle", "Files and lists", "Selection box border style", [["solid", "Solid"], ["dashed", "Dashed"], ["dotted", "Dotted"]]),
  yn(true, "band.autoScroll", "Files and lists", "Scroll while dragging the selection box", "The list scrolls by itself when the pointer is near or past its top or bottom edge, and the box keeps growing. Turn off to select only what is on screen."),
  num(28, "band.edgeZone", "Files and lists", "Scrolling starts this close to the edge", 4, 120, "px", "How near the pointer must be to the edge of the list for it to scroll."),
  num(48, "band.scrollSpeed", "Files and lists", "Fastest selection-box scrolling", 4, 200, "px/frame", "How fast the list scrolls when the pointer is far outside it. It is slower closer to the edge."),
  yn(true, "files.naturalSort", "Files and lists", "Sort numbers naturally", "file2 comes before file10."),
  yn(false, "files.caseSensitive", "Files and lists", "Sort capital letters first"),
  yn(false, "files.typeFind", "Files and lists", "Type to find", "With a folder focused, typing picks the closest name alphabetically (a name starting with what you typed, otherwise the next one after it). Backspace edits what you typed. The typed text is forgotten after the pause below, or press Escape. Off: typing only jumps to names that start with the letters, with a short pause. You can also turn it on and off with a shortcut (Settings > Advanced > Keyboard shortcuts)."),
  num(10, "files.typeFindResetSec", "Files and lists", "Type to find forgets after", 1, 120, "sec", "Seconds without typing before the typed text starts over."),
  num(700, "files.typeAheadMs", "Files and lists", "Type-to-select pause", 200, 3000, "ms", "How long typing keeps extending what you typed."),
  yn(true, "files.wheelZoom", "Files and lists", "Ctrl + scroll changes the view"),
  num(3, "files.confirmDeleteOver", "Files and lists", "Always confirm deleting this many items or more", 0, 1000, "items", "Even when \"Ask before deleting\" is off. 0 never forces it."),
  num(100, "files.recentLimit", "Files and lists", "Recent items to remember", 0, 1000),
  num(10, "files.frequentCount", "Navigation pane", "Frequently used folders to keep in the list", 0, 50, undefined, "The list is built from what you open most."),
  num(300, "frequent.window", "Navigation pane", "Frequently used: navigations looked back on", 0, 3000, undefined, "Only this many of your latest navigations count. A folder you stopped opening drops off once it is no longer within them. 0 uses everything that is remembered."),
  num(2, "frequent.minVisits", "Navigation pane", "Frequently used: opens needed to be listed", 1, 50, undefined, "A folder must have been opened this many times within the range above."),
  num(25, "frequent.timeWeight", "Navigation pane", "Frequently used: weight of time spent open", 0, 100, "%", "0 ranks only by how many times a folder was opened; 100 only by how long it stayed open (a folder left open counts for at most 15 minutes each time)."),
  num(30, "frequent.recencyBias", "Navigation pane", "Frequently used: favour recent navigations", 0, 100, "%", "0 counts every navigation in the range the same; 100 makes the newest count twice as much as the oldest."),
  num(3, "frequent.dragCount", "Navigation pane", "Frequently used: default drag frequency", 0, 50, "opens", "A folder dragged into the section starts out counting as this many opens, so it fades away if you do not open it. Folders with the same frequency are sorted by name."),
  yn(true, "recents.show", "Navigation pane", "Show Recents", "The folders or files you opened last. Off hides the entries and the heading text as well."),
  sel("folders", "recents.kind", "Navigation pane", "Recents: what to list", [["folders", "Folders"], ["files", "Files"], ["both", "Folders and files"]]),
  num(8, "recents.count", "Navigation pane", "Recents: entries kept", 1, 100, undefined, "The oldest is removed when a new one comes in. Opening one again moves it to the top."),
  num(6, "sidebar.recentsRows", "Navigation pane", "Recents: rows shown before it scrolls", 0, 100, undefined, "0 shows them all."),
  yn(true, "sidebar.showTrash", "Navigation pane", "Show Recycle Bin"),
  yn(true, "sidebar.showAnalyze", "Navigation pane", "Show Disk usage"),
  yn(true, "sidebar.showClear", "Navigation pane", "Show Smart Clear"),
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
  { key: "transfers.methods", group: "Transfers", label: "Transfer methods in use", description: "How every kind of job is being carried out right now: copies and moves (the built-in engine, Robocopy, rsync, rclone), deletes, compressing and unpacking, copies into archives, servers and phones (SFTP over SSH, FTP, WebDAV, MTP) and update downloads. Also what this computer can use and how each has done. The copy method itself is chosen under File transfers > Copy method.", control: { type: "methods" }, default: false },
  yn(true, "transfers.skipIdentical", "Transfers", "Skip files that are already there", "A file at the destination with the same name, size and modified time (and, for files up to 8 MB, the same contents) is the same file: it is kept as it is and skipped, without asking, whatever \"When a file already exists\" says. Turn off to treat it like any other clash."),
  yn(false, "transfers.openWindow", "Transfers", "Open the larger transfer window automatically"),

  // ---- Search ----
  num(140, "search.debounceMs", "Search", "Wait after typing before searching", 0, 1500, "ms"),

  // ---- Thumbnails ----
  yn(true, "thumbs.images", "Thumbnails", "Pictures", "Show the picture itself instead of a type icon: JPEG, PNG, GIF, WebP and BMP."),
  yn(true, "thumbs.documents", "Thumbnails", "Documents", "Show the first page of PDFs and Office documents. Windows and macOS use their own previews; on Linux this needs pdftoppm (PDF only)."),
  yn(true, "thumbs.videos", "Thumbnails", "Videos", "Show a frame from the video. Windows and macOS use their own previews; on Linux this needs ffmpegthumbnailer or ffmpeg."),
  yn(true, "thumbs.highPerfAlways", "Thumbnails", "Always show thumbnails in Medium, Large and Extra large views on a powerful computer", "When the performance level is Full or Manual, those three views show thumbnails whatever the per-view switches below say. On a weaker computer or on battery the switches decide."),
  sel("auto", "thumbs.videoPrewarm", "Thumbnails", "Prepare video thumbnails in the background", [["auto", "Automatic (on at the Full and Manual performance levels)"], ["on", "Always"], ["off", "Never"]], "When a folder opens, the pictures of its videos (and documents) are made ahead and kept in the thumbnail cache, so they are ready when you scroll to them. Needs \"Keep thumbnails on disk\" (Privacy and caches) to be on. Only for files on this computer."),
  sel("direct", "drag.network", "Files and lists", "Dragging files from a network location out of the app", [["direct", "Offer just the file's path (instant, the other program reads it)"], ["copy", "Copy it to this computer first, then drag the copy"], ["refuse", "Do not allow (use Copy and Paste)"]], "Windows looks inside a dragged network folder on the app's own thread, which can freeze the app for as long as the network takes. The first choice avoids that by giving the other program only the path; it does not show a picture while dragging. Copying first works with any program but takes as long as the copy; its limit is below."),
  num(1024, "drag.networkCopyMb", "Files and lists", "Largest network drag that is copied first", 16, 20000, "MB", "Bigger drags are refused with a message when \"Copy it to this computer first\" is chosen."),
  sel("auto", "transfer.smallFilesMethod", "Speed and memory", "Copy method for many small files", [["auto", "Automatic (rclone, then Robocopy, then rsync, whichever is installed)"], ["native", "Built-in engine"], ["robocopy", "Robocopy (Windows)"], ["rsync", "rsync (macOS / Linux)"], ["rclone", "rclone"], ["smb", "SMB-tuned"], ["nfs", "NFS-tuned"]], "Used when there is no history to learn from yet, and always when you pick one here. A tool that is not installed, or cannot run the job, falls back to the built-in engine."),
  sel("auto", "transfer.largeFilesMethod", "Speed and memory", "Copy method for large files", [["auto", "Automatic (built-in engine: the system's own file copy, native SMB over shares)"], ["native", "Built-in engine"], ["smb", "SMB-tuned (Robocopy with compression)"], ["robocopy", "Robocopy (Windows)"], ["rsync", "rsync (macOS / Linux)"], ["rclone", "rclone"], ["nfs", "NFS-tuned"]], "Used for jobs of fewer, bigger files. Over an SMB share the built-in engine lets Windows copy the file the native way."),
  num(1024, "transfer.smallFileKb", "Speed and memory", "Many small files means an average under", 4, 65536, "KB", "A job is treated as many small files when its average file is smaller than this."),
  num(50, "transfer.smallFileMinCount", "Speed and memory", "...and at least this many files", 2, 100000, "files", "A few files are never worth starting a tool for."),
  num(500, "transfer.earlyStartMs", "Speed and memory", "Start copying while still counting, after", 0, 10000, "ms", "Counting the files of a big folder can take minutes over a network. Once it has taken this long, files are copied as they are found, as Windows does, and the total (and the time estimate) fill in when the count ends. Small jobs are done counting before this and never notice. A copy that needs a tool (Robocopy, rsync, rclone) finishes counting first. 0 always counts everything first. Takes effect on the next transfer."),
  yn(true, "startup.instant", "Speed and memory", "Draw the window at once from how it was last time", "The window, tabs and sidebar appear on the first frame from a copy saved while the app ran, and the real data takes over a moment later. Turn off to show nothing until everything has loaded (no copy is kept). Takes effect on the next start."),
  num(700, "transfer.cancelWaitMs", "Speed and memory", "Let go of a cancelled copy that does not answer after", 0, 10000, "ms", "A copy that is stuck waiting for a drive cannot hear Cancel. After this long it is stopped for you, as Force stop would. What was already copied stays. 0 leaves the Force stop button instead."),
  sel("never", "files.renameByCopy", "Files and lists", "When a server will not rename a file", [["never", "Off: just show the error"], ["ask", "Offer to rename it by copying"], ["always", "Rename by copying it, without asking"]], "Off by default. Some shares (a pool that combines a fast cache and a big disk, say) refuse to rename a file that is plainly there. Turning this on renames it by copying it under the new name and removing the original, which copies the whole file."),
  ...TRANSFER_METRICS.map((m) => yn(m.on, `transfer.show.${m.id}`, "Copy window", m.label, m.description)),
  num(4, "transfer.activeFilesCount", "Copy window", "Files listed as being copied right now", 1, 16, "files", "How many of the files in flight are listed (one for each stream, at most). Takes effect on the next transfer."),
  sel("auto", "transfer.activeFilesRefreshMs", "Copy window", "How often the file list refreshes", [["auto", "Automatic (follows the performance preset)"], ["100", "10 times a second"], ["250", "4 times a second"], ["500", "Twice a second"], ["1000", "Every second"], ["2000", "Every 2 seconds"]], "Automatic: about 5 times a second on Full, 2 to 3 on Balanced, once a second on Saver."),
  yn(false, "transfers.speedTest", "Transfers", "Test the drives' speed before a big copy", "A short test that writes a few megabytes next to the destination, to start the time estimate better informed. It delays the start of the copy, so it is off: earlier transfers on the same route and the first seconds of the copy tell nearly as much."),
  yn(true, "ops.checkAccess", "Transfers", "Check permissions before starting", "Looks quickly whether the job needs administrator rights, so you are asked before anything happens. Network folders are not checked either way (a refusal there is offered as a retry as administrator). Off, jobs start at once and a refusal is offered as a retry afterwards."),
  yn(true, "live.enabled", "Files and lists", "Follow files that are being written or copied", "A file that is growing (a download, a recording, a copy) is checked again and again, just that file, so its size and speed stay up to date. Nothing else in the folder is checked."),
  yn(true, "live.details", "Files and lists", "Keep the details pane up to date for them", "Shows how fast the file is growing and how much has been written, for the selected files (all of them at once when several are selected), and keeps it from flashing."),
  yn(true, "live.sizes", "Files and lists", "Keep their size in the list up to date (Details view only)", "Only the files that are growing change in the list; the other rows are left alone."),
  yn(true, "live.blockHash", "Files and lists", "Do not compute a checksum while a file is still being written", "The checksum button waits until the file stops growing."),
  sel("auto", "live.interval", "Speed and memory", "How often a growing file is checked", [["auto", "Automatic (follows the performance preset)"], ["250", "4 times a second"], ["500", "Twice a second"], ["1000", "Every second"], ["2000", "Every 2 seconds"], ["5000", "Every 5 seconds"]], "Automatic: twice a second on Full, every second on Balanced, every 3 seconds on Saver. Pick one to use it whatever the preset."),
  num(4, "live.idleSeconds", "Files and lists", "A file counts as finished after this many quiet seconds", 1, 120, "s", "A copy can pause between pieces, so a file is only called finished after it has not changed for this long."),
  num(12, "live.maxFiles", "Speed and memory", "Most files followed at once", 1, 64, "files", "Saver allows at most 4 and Balanced at most 12, whatever is set here."),
  num(15, "live.recentSeconds", "Files and lists", "A file changed this recently when the folder opens is followed", 1, 600, "s", "Catches a file that is being written when you open the folder."),
  yn(false, "crash.github", "Crash logs", "Send crash reports to the project's GitHub issues", "Off unless you turn it on. When the app stops unexpectedly, the next start posts the crash report (computer information, what the app was doing, error details; your user name and e-mail addresses are removed) as an issue. Only builds that carry the project's reporting key can send; others open a ready-made issue page in your browser from Settings > Storage instead."),
  yn(true, "crash.closeWhenFrozen", "Crash logs", "Save a report and close when the window stops responding", "If the window does not react for the time below while you are looking at it, the app writes a crash report and closes straight away instead of hanging. Unsaved work is not at stake: the app does not hold documents. Off: the report is still written, but the app stays open."),
  num(15, "crash.freezeSeconds", "Crash logs", "Seconds without a response that count as frozen", 5, 600, "s"),
  yn(true, "crash.trail", "Crash logs", "Keep a trail of what you do in the app", "Clicks (by button name, never file names in text), shortcut keys and the commands that run, kept on this computer so a crash report can say what led to it. No typed text is kept."),
  yn(false, "thumbs.inDetails", "Thumbnails", "Show in the Details view", "Replaces the small type icon at the start of each row."),
  yn(false, "thumbs.inList", "Thumbnails", "Show in the List view"),
  yn(false, "thumbs.inSmall", "Thumbnails", "Show in the Small icons view"),
  yn(true, "thumbs.inMedium", "Thumbnails", "Show in the Medium icons view"),
  yn(true, "thumbs.inLarge", "Thumbnails", "Show in the Large icons view"),
  yn(true, "thumbs.inXl", "Thumbnails", "Show in the Extra large icons view"),
  yn(true, "thumbs.inPane", "Thumbnails", "Show in the details pane", "The preview at the top of the details pane for the selected file."),
  sel("normal", "thumbs.quality", "Thumbnails", "Sharpness", [["draft", "Draft (smaller, fastest)"], ["normal", "Normal"], ["sharp", "Sharp (twice the pixels, for high-resolution screens)"]], "Sharper thumbnails take longer to make and more space to keep."),
  num(0, "thumbs.parallel", "Thumbnails", "Made at the same time", 0, 8, undefined, "How many thumbnails are made at once. 0 lets the performance level decide."),
  sel("3", "thumbs.priority", "Thumbnails", "Thumbnail priority", [["1", "1 - Lowest: the app stays quickest, thumbnails fill in slowly"], ["2", "2 - Low"], ["3", "3 - Balanced"], ["4", "4 - High"], ["5", "5 - Highest: thumbnails first, the app may feel busier"]], "How much effort goes into loading thumbnails compared with everything else. Lower waits longer before starting and makes fewer at once; higher starts at once and makes more at once. \"Made at the same time\" below, when set, still decides the number."),
  num(200, "thumbs.networkCopyMb", "Thumbnails", "Largest network video to copy for a preview", 0, 4096, "MB", "Windows often will not preview a video straight from a network drive, and a server has no preview to ask for. The app then copies the start of the file (all of it, up to this size) to this computer to make the picture. 0 never copies."),
  sel("auto", "thumbs.networkDrives", "Thumbnails", "On network drives", [["auto", "Automatic (on when wired)"], ["on", "Always on"], ["off", "Always off"]], "Showing thumbnails of files on network shares means reading many files over the network. Automatic checks the connection: on a cable (Ethernet or fibre) thumbnails are always made, and on Wi-Fi or mobile data they are not."),
  num(64, "thumbs.maxImageMb", "Thumbnails", "Largest picture to preview", 0, 2048, "MB", "Bigger pictures keep their icon. 0 means no limit."),
  num(10, "thumbs.videoPercent", "Thumbnails", "Video frame to show", 0, 95, "%", "How far into the video the frame is taken. With ffmpeg alone (no ffmpegthumbnailer) the frame at two seconds is used."),

  // ---- Drives ----
  sel("files", "files.folderFill", "Files and lists", "Show which folders have something in them", [["files", "Papers on folders that hold files (looking down through subfolders)"], ["anything", "Papers on folders that hold anything, even an empty folder"], ["off", "All folders look the same"]], "Like Windows: a folder with files in it is drawn with papers showing, an empty one is drawn plain. A folder that only holds empty folders counts as empty unless you choose the second option."),
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

/** The rules for working out the Frequently used list, as the settings give them. */
export function frequentPolicy(settings: Settings | null | undefined) {
  return {
    window: adv<number>(settings, "frequent.window"),
    timeWeight: adv<number>(settings, "frequent.timeWeight"),
    minVisits: adv<number>(settings, "frequent.minVisits"),
    recencyBias: adv<number>(settings, "frequent.recencyBias"),
  };
}
