// How long the system's right-click entries (the ones other programs add: 7-Zip, Open with, Edit with…) are kept,
// and whether they are fetched before you ask. Building them is the slow part of a right-click, so the answer for a
// selection is remembered; a level with a lot of memory to spare keeps it longer and also gets it ready as soon as
// something is selected.

import type { Settings } from "../api";
import { adv } from "../settings/advanced";

export type ShellCacheLevel = "off" | "short" | "normal" | "long";
export const SHELL_CACHE_LEVELS: ShellCacheLevel[] = ["off", "short", "normal", "long"];
export const SHELL_CACHE_KEY = "perf.shellMenuCache";
export const SHELL_PREFETCH_KEY = "perf.shellMenuPrefetch";

const SECONDS: Record<ShellCacheLevel, number> = { off: 0, short: 60, normal: 300, long: 1800 };

/** "auto" follows the performance level: the bigger the cache it allows, the longer menus are kept. */
export function autoShellLevel(settings: Settings | null | undefined): ShellCacheLevel {
  const mb = settings?.performance.maxCacheMb ?? 256;
  return mb === 0 || mb >= 2048 ? "long" : mb >= 768 ? "normal" : "short";
}

export function shellCacheLevel(settings: Settings | null | undefined): ShellCacheLevel {
  const chosen = adv<string>(settings, SHELL_CACHE_KEY);
  return (SHELL_CACHE_LEVELS as string[]).includes(chosen) ? (chosen as ShellCacheLevel) : autoShellLevel(settings);
}

/** How many seconds a menu is reused for (0: never). */
export const shellCacheSeconds = (settings: Settings | null | undefined) => SECONDS[shellCacheLevel(settings)];

/** Whether the entries for a selection are fetched as soon as it is chosen, before any right-click. */
export function shellPrefetch(settings: Settings | null | undefined): boolean {
  if (shellCacheSeconds(settings) === 0) return false;
  const chosen = adv<string>(settings, SHELL_PREFETCH_KEY);
  if (chosen === "on") return true;
  if (chosen === "off") return false;
  // Automatic: only where the level has memory and time to spare, so a slow computer is not kept busy.
  const level = autoShellLevel(settings);
  return level === "long" || level === "normal";
}
