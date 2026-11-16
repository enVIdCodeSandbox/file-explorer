// The last window, kept so the next start can draw it at once.
//
// Everything the first screen needs (settings, tabs, the sidebar's places, the drive list) is saved
// here as the app runs. At the next start it is read before anything is asked of the backend, so the
// whole window appears on the first frame; the real data arrives a moment later and takes over.
// Nothing secret is kept: the update token is left out, and it is only ever read by this app.

import type { AppInfo, Drive, SavedServer, Session, Settings } from "../api";

const KEY = "fast.boot.v1";

export interface Boot {
  settings: Settings;
  session: Session;
  info: AppInfo;
  places: { label: string; path: string }[];
  servers: SavedServer[];
}

/** What is safe to keep: the same settings without the token. */
export function forCache(boot: Boot): Boot {
  return { ...boot, settings: { ...boot.settings, updates: { ...boot.settings.updates, token: "" } } };
}

export function readBoot(): Boot | null {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return null;
    const b = JSON.parse(raw) as Partial<Boot>;
    if (!b || typeof b !== "object" || !b.settings || !b.session || !b.info) return null;
    if (!Array.isArray(b.session.tabs) || b.session.tabs.length === 0 || typeof b.info.os !== "string") return null;
    // A copy written by another build may lack fields this one expects.
    if (b.info.version !== __APP_VERSION__) return null;
    return { places: [], servers: [], ...b } as Boot;
  } catch {
    return null;
  }
}

let timer: ReturnType<typeof setTimeout> | undefined;
let pending: (() => Boot | null) | null = null;

function flush() {
  clearTimeout(timer);
  const make = pending;
  pending = null;
  if (!make) return;
  try {
    const boot = make();
    if (boot) localStorage.setItem(KEY, JSON.stringify(forCache(boot)));
  } catch {
    // Storage may be full or unavailable: the next start is just not instant.
  }
}

/** Save soon, and at most once in a while however often things change. */
export function writeBoot(make: () => Boot | null, delayMs = 1500) {
  pending = make;
  clearTimeout(timer);
  timer = setTimeout(flush, delayMs);
}

/** Save now (the window is closing or going away). */
export function flushBoot() {
  flush();
}

export function forgetBoot() {
  try {
    localStorage.removeItem(KEY);
  } catch {
    // nothing to do
  }
}

export type { Drive };
