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

export const ADVANCED_GROUPS = ["Interface", "Files and lists", "Tabs", "Transfers", "Search", "Notifications", "Navigation pane", "Speed and memory"] as const;

export const ADVANCED: AdvDef[] = [
  // ---- Interface ----
  yn(true, "ui.animations", "Interface", "Animations", "Turn off for instant, motion-free changes."),
  num(100, "ui.scale", "Interface", "Interface zoom", 60, 200, "%", "Makes everything larger or smaller.", 5),
  num(0, "ui.rowHeight", "Interface", "Row height", 0, 60, "px", "0 follows the density setting."),
  yn(false, "ui.zebra", "Interface", "Shade every other row"),
  yn(false, "ui.gridLines", "Interface", "Show lines between rows"),
  yn(true, "ui.focusRing", "Interface", "Show the keyboard focus outline"),
  sel("thin", "ui.scrollbars", "Interface", "Scroll bars", [["thin", "Thin"], ["normal", "Standard"], ["hidden", "Hidden until needed"]]),
  num(24, "ui.selectionOpacity", "Interface", "Selection highlight strength", 5, 80, "%"),
  yn(true, "ui.statusBar", "Interface", "Show the status bar under the file list"),
  yn(true, "ui.dimHidden", "Interface", "Fade hidden files and folders"),

  // ---- Files and lists ----
  yn(true, "files.folderSizes", "Files and lists", "Show folder sizes", "Measured in the background from what is inside each folder, newest folder first, and kept for two minutes."),
  yn(false, "files.singleClickOpen", "Files and lists", "Open items with a single click"),
  yn(false, "files.foldersInNewTab", "Files and lists", "Open folders in a new tab"),
  sel("newTab", "files.middleClick", "Files and lists", "Middle-clicking a folder", [["newTab", "Opens it in a background tab"], ["nothing", "Does nothing"]]),
  sel("decimal", "files.sizeUnits", "Files and lists", "Size units", [["decimal", "KB, MB, GB: 1 MB = 1,000,000 bytes (as on a drive's label)"], ["binary", "KiB, MiB, GiB: 1 MiB = 1,048,576 bytes"], ["windows", "KB, MB, GB, but counted in 1,024s (what Windows Explorer shows)"]]),
  sel("system", "files.dateFormat", "Files and lists", "Dates", [["system", "Match my system"], ["iso", "2026-11-11 19:42"], ["relative", "Today, Yesterday, 3 days ago"]]),
  sel("auto", "files.clock", "Files and lists", "Clock", [["auto", "Match my system"], ["12", "12-hour"], ["24", "24-hour"]]),
  yn(true, "files.naturalSort", "Files and lists", "Sort numbers naturally", "file2 comes before file10."),
  yn(false, "files.caseSensitive", "Files and lists", "Sort capital letters first"),
  num(700, "files.typeAheadMs", "Files and lists", "Type-to-select pause", 200, 3000, "ms", "How long typing keeps extending what you typed."),
  yn(true, "files.wheelZoom", "Files and lists", "Ctrl + scroll changes the view"),
  num(3, "files.confirmDeleteOver", "Files and lists", "Always confirm deleting this many items or more", 0, 1000, "items", "Even when \"Ask before deleting\" is off. 0 never forces it."),
  num(100, "files.recentLimit", "Files and lists", "Recent items to remember", 0, 1000),
  num(10, "files.frequentCount", "Navigation pane", "Frequently used folders to keep in the list", 0, 50, undefined, "The list is built from what you open most."),
  num(6, "sidebar.frequentRows", "Navigation pane", "Frequently used: rows shown before it scrolls", 0, 50, undefined, "0 shows them all. Longer lists get their own scroll bar so the rest of the pane stays in view."),
  num(10, "sidebar.pinnedRows", "Navigation pane", "Pinned: rows shown before it scrolls", 0, 100, undefined, "0 shows them all."),
  num(10, "sidebar.drivesRows", "Navigation pane", "Drives: rows shown before it scrolls", 0, 100, undefined, "0 shows them all."),
  num(20, "files.undoLimit", "Files and lists", "Actions Undo can reverse", 0, 200),

  // ---- Tabs ----
  num(200, "tabs.width", "Tabs", "Tab width", 90, 360, "px", "The width tabs have while there is room."),
  num(90, "tabs.minWidth", "Tabs", "Narrowest a tab gets", 40, 240, "px", "Past this, tabs stop shrinking and the bar scrolls with arrows."),
  num(34, "tabs.height", "Tabs", "Tab bar height", 24, 64, "px"),
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
