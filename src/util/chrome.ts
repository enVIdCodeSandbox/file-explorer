// Which arrangement of the window is in use. Every style shows the same parts
// (tabs, address bar, commands, navigation pane, details, status bar); this
// decides how they are arranged and trimmed, like the file manager of that system.

import { adv } from "../settings/advanced";
import { useStore } from "../store";

export type Chrome = "windows" | "macos" | "linux";

export const asChrome = (v: unknown): Chrome => (v === "macos" || v === "linux" ? v : "windows");

export function useChrome(): Chrome {
  return useStore((s) => asChrome(s.settings ? adv<string>(s.settings, "ui.chrome") : "windows"));
}

/** What each system calls the navigation pane's sections. An empty name hides the heading. */
export const SIDE_NAMES: Record<Chrome, Record<string, string>> = {
  windows: {},
  macos: { Navigation: "", "Frequently used": "Recents", Pinned: "Favorites", Drives: "Locations", Servers: "Shared" },
  linux: { Navigation: "", "Frequently used": "Recent", Pinned: "Starred", Drives: "Devices", Servers: "Network" },
};
