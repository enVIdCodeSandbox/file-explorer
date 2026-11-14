// Which arrangement of the window is in use. Every style shows the same parts
// (tabs, address bar, commands, navigation pane, details, status bar); this
// decides how they are arranged and trimmed, like the file manager of that system.

import { adv } from "../settings/advanced";
import { useStore } from "../store";

export type Chrome = "windows" | "macos" | "linux";
/** The phone arrangements, each like the file manager that comes with that maker's phones. */
export type PhoneChrome = "google" | "samsung" | "apple";

/** The computer arrangement to use. A phone look saved in the settings shows as the nearest computer one. */
export const asChrome = (v: unknown): Chrome => (v === "macos" || v === "apple" ? "macos" : v === "linux" ? "linux" : "windows");

/** The phone arrangement to use. A computer look saved in the settings shows as the nearest phone one. */
export const asPhoneChrome = (v: unknown): PhoneChrome => (v === "samsung" ? "samsung" : v === "apple" || v === "macos" ? "apple" : "google");

export function usePhoneChrome(): PhoneChrome {
  return useStore((s) => asPhoneChrome(s.settings ? adv<string>(s.settings, "ui.chrome") : "google"));
}

export function useChrome(): Chrome {
  return useStore((s) => asChrome(s.settings ? adv<string>(s.settings, "ui.chrome") : "windows"));
}

/** What each system calls the navigation pane's sections. An empty name hides the heading. */
export const SIDE_NAMES: Record<Chrome, Record<string, string>> = {
  windows: {},
  macos: { Navigation: "", "Frequently used": "Recents", Pinned: "Favorites", Drives: "Locations", Servers: "Shared" },
  linux: { Navigation: "", "Frequently used": "Recent", Pinned: "Starred", Drives: "Devices", Servers: "Network" },
};
