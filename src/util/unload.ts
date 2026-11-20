// Unloading the interface while nobody is looking at it.
//
// When the window is minimized, on another virtual desktop or covered by other windows, the browser engine reports the
// page as hidden. After a wait that depends on the level, the app takes its heavy interface down (the file list with its
// pictures, the navigation pane, the toolbars) and keeps everything that matters: tabs, selection, settings and running
// jobs live outside it. When the window shows again the interface is put back at once, at the scroll position it had.
// Pictures kept in memory for the file list are let go as well.

import type { Settings } from "../api";
import { adv } from "../settings/advanced";

export type UnloadLevel = "off" | "light" | "balanced" | "aggressive";
export const UNLOAD_LEVELS: UnloadLevel[] = ["off", "light", "balanced", "aggressive"];

/** The setting: a level, or "auto" to follow the performance level. */
export const UNLOAD_KEY = "perf.uiUnload";

/**
 * "auto" follows how much memory the performance level lets the app keep: a level with a big cache is for a machine
 * with room to spare, so nothing is unloaded; a small cache means every megabyte counts. Phones are never "off".
 */
export function autoLevel(settings: Settings | null | undefined, mobile = false): UnloadLevel {
  const mb = settings?.performance.maxCacheMb ?? 256;
  let level: UnloadLevel = mb === 0 || mb >= 2048 ? "off" : mb >= 768 ? "light" : mb >= 128 ? "balanced" : "aggressive";
  if (mobile && level === "off") level = "light";
  return level;
}

export function unloadLevelOf(settings: Settings | null | undefined, mobile = false): UnloadLevel {
  const chosen = adv<string>(settings, UNLOAD_KEY);
  return (UNLOAD_LEVELS as string[]).includes(chosen) ? (chosen as UnloadLevel) : autoLevel(settings, mobile);
}

export interface UnloadPlan {
  /** How long the window must stay out of sight before the interface comes down. Null: never. */
  afterMs: number | null;
  /** Also let go of pictures held for the file list. */
  dropPictures: boolean;
}

export function planFor(level: UnloadLevel): UnloadPlan {
  switch (level) {
    case "off":
      return { afterMs: null, dropPictures: false };
    case "light":
      return { afterMs: 5 * 60_000, dropPictures: false };
    case "balanced":
      return { afterMs: 60_000, dropPictures: true };
    case "aggressive":
      return { afterMs: 10_000, dropPictures: true };
  }
}

// ---- the state, as a tiny store so any component can follow it ----

let suspended = false;
const listeners = new Set<() => void>();
export const isSuspended = () => suspended;
export const onSuspendChange = (fn: () => void) => {
  listeners.add(fn);
  return () => void listeners.delete(fn);
};
function setSuspended(v: boolean) {
  if (v === suspended) return;
  suspended = v;
  listeners.forEach((l) => l());
}

export interface UnloaderHooks {
  /** Current settings (read when the timer is set, so a change applies the next time). */
  settings: () => Settings | null | undefined;
  mobile: () => boolean;
  /** Called just before the interface comes down (to remember scroll positions) and after it is back. */
  before?: () => void;
  after?: () => void;
  dropPictures?: () => void;
}

/** Watch the page's visibility. Returns a function that stops watching. */
export function startUnloader(hooks: UnloaderHooks, doc: Document = document): () => void {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const clear = () => {
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
  };
  const check = () => {
    clear();
    if (doc.visibilityState === "visible") {
      if (suspended) {
        setSuspended(false);
        hooks.after?.();
        setTimeout(() => (resumedSelection = null), 3000);
      }
      return;
    }
    const plan = planFor(unloadLevelOf(hooks.settings(), hooks.mobile()));
    if (plan.afterMs === null) return;
    timer = setTimeout(() => {
      timer = undefined;
      if (doc.visibilityState === "visible") return;
      hooks.before?.();
      if (plan.dropPictures) hooks.dropPictures?.();
      setSuspended(true);
    }, plan.afterMs);
  };
  doc.addEventListener("visibilitychange", check);
  return () => {
    doc.removeEventListener("visibilitychange", check);
    clear();
    setSuspended(false);
  };
}

/** Scroll positions to put back when the interface returns, keyed by the area's name. */
const scrolls = new Map<string, number>();
export function rememberScrolls(doc: Document = document) {
  scrolls.clear();
  doc.querySelectorAll<HTMLElement>("[data-scroll-keep]").forEach((el) => {
    if (el.scrollTop > 0) scrolls.set(el.dataset.scrollKeep!, el.scrollTop);
  });
}
export function restoreScrolls(doc: Document = document) {
  if (!scrolls.size) return;
  let tries = 0;
  const put = () => {
    let left = 0;
    scrolls.forEach((top, key) => {
      const el = doc.querySelector<HTMLElement>(`[data-scroll-keep="${key}"]`);
      if (el && el.scrollHeight - el.clientHeight >= top) {
        el.scrollTop = top;
        scrolls.delete(key);
      } else left++;
    });
    // The list fills in a moment after it is shown; keep trying briefly.
    if (left && ++tries < 20) setTimeout(put, 100);
    else scrolls.clear();
  };
  requestAnimationFrame(put);
}

/** What was selected when the interface came down, for the list to take back up when it returns. */
let resumedSelection: string[] | null = null;
export const keepSelection = (paths: string[]) => void (resumedSelection = paths.length ? [...paths] : null);
/**
 * The names selected in `folder` before the interface came down, or null. It stays available for a few seconds after
 * the interface returns rather than being handed out once, because a list can be built twice in a row.
 */
export function takeSelection(folder: string, parent: (p: string) => string | null, name: (p: string) => string): string[] | null {
  const names = resumedSelection?.filter((p) => parent(p) === folder).map(name) ?? [];
  return names.length ? names : null;
}
