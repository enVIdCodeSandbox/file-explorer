// What to tell the person while a folder is still being read, and how often the list is redrawn while it fills.

import type { Settings } from "../api";
import { adv } from "../settings/advanced";

export interface LoadProgress {
  path: string;
  /** Items the system has handed over so far (counting ones left out, such as hidden files). */
  done: number;
  /** Items the folder has, when the system said (0 = not known). */
  total: number;
  startedAt: number;
  /** Where `total` came from: the system, or the last time this folder was opened. */
  totalFrom?: "system" | "last-visit";
  /** How long this folder took the last time it was read (ms), when it was. */
  lastMs?: number;
}

// ---- What the last visit to a folder looked like -------------------------------------------------------------------

interface Visit {
  count: number;
  ms: number;
}
const visits = new Map<string, Visit>();
const MAX_VISITS = 300;

/** Note how big a folder was and how long it took, so the next visit can say what to expect. */
export function rememberVisit(path: string, count: number, ms: number): void {
  visits.delete(path);
  visits.set(path, { count, ms });
  if (visits.size > MAX_VISITS) visits.delete(visits.keys().next().value as string);
}
export const recallVisit = (path: string): Visit | undefined => visits.get(path);
export const forgetVisits = () => visits.clear();

// ---- Estimates -------------------------------------------------------------------------------------------------------

/** Seconds until everything is in, from the pace so far; null when it can't be told yet. */
export function etaSeconds(p: Pick<LoadProgress, "done" | "total" | "startedAt" | "lastMs">, now: number): number | null {
  const elapsed = (now - p.startedAt) / 1000;
  if (p.total > 0 && p.done > 0 && p.done < p.total && elapsed >= 0.4) return ((p.total - p.done) * elapsed) / p.done;
  // Nothing has arrived (a phone hands over a whole folder at once): go by how long it took last time.
  if (p.done === 0 && p.lastMs && p.lastMs > 1500) return Math.max(0, p.lastMs / 1000 - elapsed);
  return null;
}

/** One-line wording for the details pane and status bar. */
export function loadingNotice(p: LoadProgress, now: number, fmt: (secs: number) => string): { title: string; detail: string; fraction: number | null } {
  const eta = etaSeconds(p, now);
  const known = p.total > 0;
  const fraction = known && p.done > 0 ? Math.min(1, p.done / p.total) : null;
  if (p.done === 0) {
    const slow = p.lastMs && p.lastMs > 1500 ? ` Last time it took about ${fmt(Math.round(p.lastMs / 1000))}.` : "";
    const over = p.lastMs && p.lastMs > 1500 && now - p.startedAt > p.lastMs;
    const left = eta == null ? "" : over ? " Taking longer than last time." : eta < 1 ? " Should be any moment." : ` About ${fmt(Math.ceil(eta))} left.`;
    return {
      title: "Reading this folder",
      detail: `The device has not sent its list yet. Big folders can take a while.${eta == null || over ? slow + (over ? left : "") : left}`,
      fraction: null,
    };
  }
  const count = known ? `${p.done.toLocaleString()} of ${p.total.toLocaleString()} items${p.totalFrom === "last-visit" ? " (as it was last time)" : ""}` : `${p.done.toLocaleString()} items so far`;
  const when = eta == null ? "" : eta < 1 ? " Almost done." : ` About ${fmt(Math.ceil(eta))} left.`;
  return {
    title: "Still loading this folder",
    detail: `Not every file is shown yet (${count}). The rest appear as they are read.${when}`,
    fraction,
  };
}

// ---- How often the list is redrawn while it fills ------------------------------------------------------------------

export type RefreshLevel = "fast" | "normal" | "gentle";
export const REFRESH_KEY = "perf.loadRefresh";
const MIN_MS: Record<RefreshLevel, number> = { fast: 60, normal: 150, gentle: 400 };
export const REFRESH_MAX_MS = 1500;

/** "auto" follows the performance level: a computer allowed a lot of memory redraws more often. */
export function refreshLevel(settings: Settings | null | undefined): RefreshLevel {
  const chosen = adv<string>(settings, REFRESH_KEY);
  if (chosen === "fast" || chosen === "normal" || chosen === "gentle") return chosen;
  const mb = settings?.performance.maxCacheMb ?? 256;
  return mb === 0 || mb >= 2048 ? "fast" : mb >= 768 ? "normal" : "gentle";
}
export const refreshMinMs = (settings: Settings | null | undefined) => MIN_MS[refreshLevel(settings)];

/** Time to leave between redraws: the level's own pace, stretched when a redraw has been taking long (more than a
 *  third of the time would go on drawing), never beyond REFRESH_MAX_MS. */
export function nextInterval(minMs: number, drawMs: number): number {
  return Math.round(Math.min(REFRESH_MAX_MS, Math.max(minMs, drawMs * 3)));
}

/** Lets a stream of updates through at a steady pace: the first at once, then at most one per interval, always
 *  ending on the latest. `drawMs` reports how long the last redraw took (measured by whoever draws). */
export function createPacer(minMs: number, now: () => number = () => performance.now(), later: (fn: () => void, ms: number) => unknown = setTimeout, cancelLater: (h: unknown) => void = (h) => clearTimeout(h as ReturnType<typeof setTimeout>)) {
  let last = -Infinity;
  let pending: (() => void) | null = null;
  let timer: unknown = null;
  let interval = minMs;
  const fire = () => {
    timer = null;
    const job = pending;
    pending = null;
    if (!job) return;
    last = now();
    job();
  };
  return {
    /** Ask for `job` to run; if another is already waiting, it is replaced (only the latest matters). */
    push(job: () => void) {
      pending = job;
      if (timer != null) return;
      const wait = Math.max(0, last + interval - now());
      if (wait === 0) fire();
      else timer = later(fire, wait);
    },
    /** Run what is waiting now (the listing is complete). */
    flush() {
      if (timer != null) cancelLater(timer);
      timer = null;
      fire();
    },
    cancel() {
      if (timer != null) cancelLater(timer);
      timer = null;
      pending = null;
    },
    /** How long the last redraw took. */
    drew(ms: number) {
      interval = nextInterval(minMs, ms);
    },
    get interval() {
      return interval;
    },
  };
}

/** Shows the "still loading" notice only for a folder that is slow to open, and never once it has stopped: a folder
 *  that opens at once must not have the notice appear after it is already complete. */
export function createNotice(show: () => void, delayMs = 250) {
  let over = false;
  const timer = setTimeout(() => {
    if (!over) show();
  }, delayMs);
  return {
    stop() {
      over = true;
      clearTimeout(timer);
    },
    get stopped() {
      return over;
    },
  };
}
