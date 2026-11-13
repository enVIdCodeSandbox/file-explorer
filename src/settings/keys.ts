// Keyboard shortcuts that can be changed in Settings > Advanced. Others
// (arrow keys, Enter, F2, type-to-select…) belong to the file list itself.

import type { Settings } from "../api";

export interface KeyCommand {
  id: string;
  label: string;
  default: string;
}

export const KEY_COMMANDS: KeyCommand[] = [
  { id: "newTab", label: "New tab", default: "Ctrl+T" },
  { id: "closeTab", label: "Close tab", default: "Ctrl+W" },
  { id: "nextTab", label: "Next tab", default: "Ctrl+Tab" },
  { id: "prevTab", label: "Previous tab", default: "Ctrl+Shift+Tab" },
  { id: "settings", label: "Open settings", default: "Ctrl+," },
  { id: "toggleSidebar", label: "Collapse or expand the navigation pane", default: "Ctrl+B" },
  { id: "toggleDetails", label: "Show or hide the details pane", default: "Alt+P" },
  { id: "editLayout", label: "Edit the window layout", default: "Ctrl+Alt+L" },
  { id: "thisPc", label: "Go to This PC", default: "Ctrl+Shift+C" },
  { id: "home", label: "Go to Home", default: "Alt+Home" },
  { id: "open", label: "Open the selected item", default: "Enter" },
  { id: "rename", label: "Rename", default: "F2" },
  { id: "delete", label: "Delete", default: "Delete" },
  { id: "newFolder", label: "New folder", default: "Ctrl+Shift+N" },
  { id: "properties", label: "Properties", default: "Alt+Enter" },
  { id: "refresh", label: "Refresh", default: "F5" },
  { id: "goUp", label: "Up one level", default: "Alt+ArrowUp" },
  { id: "back", label: "Back", default: "Alt+ArrowLeft" },
  { id: "forward", label: "Forward", default: "Alt+ArrowRight" },
  { id: "focusAddress", label: "Edit the address", default: "Ctrl+L" },
  { id: "focusSearch", label: "Search", default: "Ctrl+F" },
  { id: "toggleHidden", label: "Show or hide hidden files", default: "Ctrl+H" },
];

export function keyFor(settings: Settings | null, id: string): string {
  return settings?.keys?.[id] ?? KEY_COMMANDS.find((k) => k.id === id)?.default ?? "";
}

/** Describe a key press as "Ctrl+Shift+T" (Cmd counts as Ctrl). */
export function comboOf(e: Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey">): string {
  if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return "";
  const parts: string[] = [];
  if (e.ctrlKey || e.metaKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  const k = e.key === " " ? "Space" : e.key.length === 1 ? e.key.toUpperCase() : e.key;
  parts.push(k);
  return parts.join("+");
}

/** The id of the command a key press triggers, if any. */
export function commandFor(settings: Settings | null, e: KeyboardEvent): string | null {
  const combo = comboOf(e);
  if (!combo) return null;
  return KEY_COMMANDS.find((k) => keyFor(settings, k.id).toLowerCase() === combo.toLowerCase())?.id ?? null;
}
