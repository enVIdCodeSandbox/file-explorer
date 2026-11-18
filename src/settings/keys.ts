// Every keyboard shortcut and mouse button of the app, in one list. Each can be changed, turned off, or put back to
// its default in Settings → Interface → Keyboard and mouse. A binding is written "Ctrl+Shift+T", "F2", "Alt+ArrowLeft"
// or, for the extra mouse buttons, "Mouse4" (the side button that goes back) and "Mouse5" (forward).
//
// Not here, because they only make sense in one place: Escape and Enter inside dialogs and menus, the arrow keys
// inside menus and text boxes, and typing a name to jump to it.

import type { Settings } from "../api";

/** Where a shortcut is handled. Used to tell which ones can really clash. */
export type KeyScope = "app" | "nav" | "list" | "panes";

export interface KeyCommand {
  id: string;
  label: string;
  group: string;
  scope: KeyScope;
  default: string;
  /** Holding Shift with the key does the same thing, extending the selection (the arrow keys) or going further (Delete). */
  shiftVariant?: boolean;
  /** The key also counts with or without Shift (zoom: "=" and "+" are one key). */
  shiftFree?: boolean;
}

const c = (id: string, label: string, group: string, scope: KeyScope, def: string, extra: Partial<KeyCommand> = {}): KeyCommand => ({ id, label, group, scope, default: def, ...extra });

export const KEY_GROUPS = ["Tabs and windows", "Moving between folders", "Moving around a list", "Selecting", "Files and folders", "Search", "View and panels", "Split view", "Mouse buttons"] as const;

export const KEY_COMMANDS: KeyCommand[] = [
  // Tabs and windows
  c("newTab", "New tab", "Tabs and windows", "app", "Ctrl+T"),
  c("closeTab", "Close tab", "Tabs and windows", "app", "Ctrl+W"),
  c("reopenTab", "Bring back the tab you closed", "Tabs and windows", "app", "Ctrl+Shift+T"),
  c("duplicateTab", "Duplicate the tab", "Tabs and windows", "app", ""),
  c("nextTab", "Next tab", "Tabs and windows", "app", "Ctrl+Tab"),
  c("prevTab", "Previous tab", "Tabs and windows", "app", "Ctrl+Shift+Tab"),
  ...[1, 2, 3, 4, 5, 6, 7, 8].map((n) => c(`tab${n}`, `Go to tab ${n}`, "Tabs and windows", "app", `Ctrl+${n}`)),
  c("tabLast", "Go to the last tab", "Tabs and windows", "app", "Ctrl+9"),
  c("settings", "Open settings", "Tabs and windows", "app", "Ctrl+,"),
  c("fullscreen", "Full screen on or off", "Tabs and windows", "app", "F11"),

  // Moving between folders
  c("back", "Back", "Moving between folders", "nav", "Alt+ArrowLeft"),
  c("forward", "Forward", "Moving between folders", "nav", "Alt+ArrowRight"),
  c("goUp", "Up one level", "Moving between folders", "nav", "Alt+ArrowUp"),
  c("goUpList", "Up one level (from the list)", "Moving between folders", "list", "Backspace"),
  c("home", "Go to Home", "Moving between folders", "app", "Alt+Home"),
  c("thisPc", "Go to This PC", "Moving between folders", "app", "Ctrl+Shift+C"),
  c("refresh", "Refresh", "Moving between folders", "app", "F5"),
  c("refreshAlt", "Refresh (second key)", "Moving between folders", "app", "Ctrl+R"),
  c("focusAddress", "Edit the address", "Moving between folders", "nav", "Ctrl+L"),
  c("focusAddressAlt", "Edit the address (second key)", "Moving between folders", "nav", "Alt+D"),
  c("focusAddressF4", "Edit the address (third key)", "Moving between folders", "nav", "F4"),
  c("focusCommandLine", "Go to the command line", "Moving between folders", "app", "Ctrl+Shift+;"),

  // Moving around a list
  c("cursorDown", "Down one item", "Moving around a list", "list", "ArrowDown", { shiftVariant: true }),
  c("cursorUp", "Up one item", "Moving around a list", "list", "ArrowUp", { shiftVariant: true }),
  c("cursorRight", "Right one item", "Moving around a list", "list", "ArrowRight", { shiftVariant: true }),
  c("cursorLeft", "Left one item", "Moving around a list", "list", "ArrowLeft", { shiftVariant: true }),
  c("cursorFirst", "To the first item", "Moving around a list", "list", "Home", { shiftVariant: true }),
  c("cursorLast", "To the last item", "Moving around a list", "list", "End", { shiftVariant: true }),
  c("pageDown", "Down a page", "Moving around a list", "list", "PageDown", { shiftVariant: true }),
  c("pageUp", "Up a page", "Moving around a list", "list", "PageUp", { shiftVariant: true }),

  // Selecting
  c("selectAll", "Select all", "Selecting", "list", "Ctrl+A"),
  c("clearSelection", "Clear the selection", "Selecting", "list", "Escape"),
  c("toggleCheck", "Tick or untick the current item (with check boxes)", "Selecting", "list", "Space"),
  c("selectGroup", "Select by pattern", "Selecting", "list", "Num+"),
  c("deselectGroup", "Unselect by pattern", "Selecting", "list", "Num-"),
  c("invertSelection", "Invert the selection", "Selecting", "list", "Num*"),
  c("restoreSelection", "Bring back the earlier selection", "Selecting", "list", "Num/"),

  // Files and folders
  c("open", "Open the selected item", "Files and folders", "list", "Enter"),
  c("openNewTab", "Open the selected folder in a new tab", "Files and folders", "list", "Ctrl+Enter"),
  c("view", "View the selected file (the file viewer)", "Files and folders", "list", "F3"),
  c("quickLook", "Quick look at the selected file", "Files and folders", "list", "Space"),
  c("rename", "Rename", "Files and folders", "list", "F2"),
  c("delete", "Delete (with Shift: delete for good)", "Files and folders", "list", "Delete", { shiftVariant: true }),
  c("copy", "Copy", "Files and folders", "list", "Ctrl+C"),
  c("cut", "Cut", "Files and folders", "list", "Ctrl+X"),
  c("paste", "Paste", "Files and folders", "list", "Ctrl+V"),
  c("copyPath", "Copy the path of the selected items", "Files and folders", "list", "Ctrl+Alt+C"),
  c("newFolder", "New folder", "Files and folders", "list", "Ctrl+Shift+N"),
  c("newFile", "New file", "Files and folders", "list", ""),
  c("properties", "Properties", "Files and folders", "list", "Alt+Enter"),
  c("undo", "Undo the last file action", "Files and folders", "app", "Ctrl+Z"),
  c("redo", "Redo", "Files and folders", "app", "Ctrl+Y"),
  c("redoAlt", "Redo (second key)", "Files and folders", "app", "Ctrl+Shift+Z"),

  // Search
  c("focusSearch", "Search this folder (just its own level)", "Search", "nav", "Ctrl+F"),
  c("searchGlobal", "Search everywhere", "Search", "nav", "Ctrl+Shift+F"),
  c("searchInFiles", "Search in files (the file tools)", "Search", "app", "Alt+F7"),

  // View and panels
  c("toggleSidebar", "Collapse or expand the navigation pane", "View and panels", "app", "Ctrl+B"),
  c("toggleDetails", "Show or hide the details pane", "View and panels", "app", "Alt+P"),
  c("toggleTerminal", "Show or hide the built-in terminal", "View and panels", "app", "Ctrl+`"),
  c("toggleHidden", "Show or hide hidden files", "View and panels", "app", "Ctrl+H"),
  c("toggleTypeFind", "Turn type-to-find on or off", "View and panels", "app", ""),
  c("editLayout", "Edit the window layout", "View and panels", "app", "Ctrl+Alt+L"),
  c("zoomIn", "Make the list bigger", "View and panels", "app", "Ctrl+=", { shiftFree: true }),
  c("zoomInNum", "Make the list bigger (number pad)", "View and panels", "app", "Ctrl+Num+"),
  c("zoomOut", "Make the list smaller", "View and panels", "app", "Ctrl+-", { shiftFree: true }),
  c("zoomOutNum", "Make the list smaller (number pad)", "View and panels", "app", "Ctrl+Num-"),
  c("zoomReset", "Back to the normal size", "View and panels", "app", "Ctrl+0"),

  // Split view
  c("nextPane", "Next pane", "Split view", "panes", "Tab"),
  c("prevPane", "Previous pane", "Split view", "panes", "Shift+Tab"),
  c("copyToOtherPane", "Copy to the other pane", "Split view", "panes", "F5"),
  c("moveToOtherPane", "Move to the other pane", "Split view", "panes", "F6"),

  // Mouse buttons
  c("backMouse", "Back (side button)", "Mouse buttons", "nav", "Mouse4"),
  c("forwardMouse", "Forward (side button)", "Mouse buttons", "nav", "Mouse5"),
];

export const commandById = (id: string): KeyCommand | undefined => KEY_COMMANDS.find((k) => k.id === id);

// ---- What the person changed: the binding (settings.keys) and whether it is switched off (a list in advanced) ----

export const KEYS_OFF = "keys.off";

/** The ids of shortcuts that are switched off. */
export function offList(settings: Settings | null): Set<string> {
  const raw = settings?.advanced?.[KEYS_OFF];
  return new Set(typeof raw === "string" ? raw.split(",").map((x) => x.trim()).filter(Boolean) : []);
}

export function isOn(settings: Settings | null, id: string): boolean {
  return !offList(settings).has(id);
}

/** The binding of a command, as set (or its default), even when switched off. */
export function keyFor(settings: Settings | null, id: string): string {
  return settings?.keys?.[id] ?? commandById(id)?.default ?? "";
}

/** The binding in effect: "" when switched off or not set. */
export function activeKey(settings: Settings | null, id: string): string {
  return isOn(settings, id) ? keyFor(settings, id) : "";
}

/** Where the physical key says what the character is once Shift has changed it ("Shift+;" gives ":"). */
const BY_CODE: Record<string, string> = {
  Backquote: "`", Minus: "-", Equal: "=", BracketLeft: "[", BracketRight: "]", Backslash: "\\", Semicolon: ";", Quote: "'", Comma: ",", Period: ".", Slash: "/",
  Digit0: "0", Digit1: "1", Digit2: "2", Digit3: "3", Digit4: "4", Digit5: "5", Digit6: "6", Digit7: "7", Digit8: "8", Digit9: "9",
};
const NUMPAD: Record<string, string> = { NumpadAdd: "Num+", NumpadSubtract: "Num-", NumpadMultiply: "Num*", NumpadDivide: "Num/" };

type KeyLike = Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey"> & { code?: string };

/** Describe a key press as "Ctrl+Shift+T" (Cmd counts as Ctrl). */
export function comboOf(e: KeyLike): string {
  if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return "";
  const parts: string[] = [];
  if (e.ctrlKey || e.metaKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  let k: string;
  if (e.code && NUMPAD[e.code]) k = NUMPAD[e.code];
  else if (e.key === " ") k = "Space";
  else if (e.key.length === 1) {
    const shiftedSymbol = e.shiftKey && !/[a-z0-9]/i.test(e.key) && e.code && BY_CODE[e.code];
    const shiftedDigit = e.shiftKey && e.code && /^Digit\d$/.test(e.code) && BY_CODE[e.code];
    k = (shiftedSymbol || shiftedDigit || e.key).toUpperCase();
  } else k = e.key;
  parts.push(k);
  return parts.join("+");
}

/** The extra mouse buttons: 3 is the side button that goes back, 4 the one that goes forward. */
export function mouseComboOf(e: Pick<MouseEvent, "button" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey">): string {
  if (e.button !== 3 && e.button !== 4) return "";
  const parts: string[] = [];
  if (e.ctrlKey || e.metaKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  parts.push(e.button === 3 ? "Mouse4" : "Mouse5");
  return parts.join("+");
}

const same = (a: string, b: string) => a !== "" && a.toLowerCase() === b.toLowerCase();

const withoutShift = (combo: string) => combo.replace(/(^|\+)Shift\+/i, "$1");
const withShift = (combo: string) => {
  const i = combo.lastIndexOf("+", combo.length - 2);
  return i < 0 ? `Shift+${combo}` : `${combo.slice(0, i)}+Shift${combo.slice(i)}`;
};

/** Every command a key press or mouse button triggers (several may share a key, such as Space or F5 in the panes). Switched-off and unset commands never match. */
export function commandsFor(settings: Settings | null, e: KeyboardEvent | MouseEvent): string[] {
  const combo = "key" in e ? comboOf(e) : mouseComboOf(e);
  if (!combo) return [];
  const off = offList(settings);
  const live = KEY_COMMANDS.filter((k) => !off.has(k.id));
  const exact = live.filter((k) => same(keyFor(settings, k.id), combo));
  const out = new Set(exact.map((k) => k.id));
  const hasShift = /(^|\+)shift\+/i.test(combo);
  if (hasShift) {
    // Shift added to the key of a command that takes it (extend the selection, delete for good).
    const bare = withoutShift(combo);
    for (const k of live) if (k.shiftVariant && same(keyFor(settings, k.id), bare)) out.add(k.id);
  }
  // Keys where Shift is part of typing the character (zoom: "=" and "+").
  const flip = hasShift ? withoutShift(combo) : withShift(combo);
  for (const k of live) if (k.shiftFree && same(keyFor(settings, k.id), flip)) out.add(k.id);
  return [...out];
}

/** The first command a key press or mouse button triggers, if any. */
export function commandFor(settings: Settings | null, e: KeyboardEvent | MouseEvent): string | null {
  return commandsFor(settings, e)[0] ?? null;
}

/** Whether two shortcuts can fire on the same key press. */
export function clashes(settings: Settings | null, a: KeyCommand, b: KeyCommand): boolean {
  if (a.id === b.id || !isOn(settings, a.id) || !isOn(settings, b.id)) return false;
  if (!same(keyFor(settings, a.id), keyFor(settings, b.id))) return false;
  // The split view's own keys (F5, Tab) win while a pane is in focus; and a list key and a tab key never meet.
  if (a.scope === "panes" || b.scope === "panes") return a.scope === b.scope;
  // Selecting and opening share Space on purpose: Space ticks a box when there are check boxes and quick-looks otherwise.
  if ((a.id === "toggleCheck" && b.id === "quickLook") || (a.id === "quickLook" && b.id === "toggleCheck")) return false;
  return true;
}

/** The other commands a command clashes with. */
export function clashesOf(settings: Settings | null, id: string): KeyCommand[] {
  const k = commandById(id);
  return k ? KEY_COMMANDS.filter((o) => clashes(settings, k, o)) : [];
}

/** Can a mouse button be one of the keys for this command? Only where going somewhere or acting on the app makes sense. */
export const MOUSE_BINDABLE = (k: KeyCommand) => k.scope === "app" || k.scope === "nav";

// ---- Writing a binding from the pieces the picker gives ----

export const PICKER_KEYS: { group: string; keys: string[] }[] = [
  { group: "Letters", keys: "ABCDEFGHIJKLMNOPQRSTUVWXYZ".split("") },
  { group: "Numbers", keys: "0123456789".split("") },
  { group: "Function keys", keys: Array.from({ length: 12 }, (_, i) => `F${i + 1}`) },
  { group: "Arrows and moving", keys: ["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Home", "End", "PageUp", "PageDown"] },
  { group: "Editing", keys: ["Enter", "Space", "Tab", "Backspace", "Delete", "Insert", "Escape"] },
  { group: "Symbols", keys: [",", ".", "/", ";", "'", "[", "]", "\\", "-", "=", "`"] },
  { group: "Number pad", keys: ["Num+", "Num-", "Num*", "Num/"] },
  { group: "Mouse side buttons", keys: ["Mouse4", "Mouse5"] },
];

export interface Pieces {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  key: string;
}

export function piecesOf(combo: string): Pieces {
  const parts = combo ? combo.split(/\+(?!$)/) : [];
  const key = parts.pop() ?? "";
  const mods = parts.map((p) => p.toLowerCase());
  return { ctrl: mods.includes("ctrl"), alt: mods.includes("alt"), shift: mods.includes("shift"), key: key === "" && combo.endsWith("+") ? "+" : key };
}

export function comboFrom(p: Pieces): string {
  if (!p.key) return "";
  return [p.ctrl && "Ctrl", p.alt && "Alt", p.shift && "Shift", p.key].filter(Boolean).join("+");
}
