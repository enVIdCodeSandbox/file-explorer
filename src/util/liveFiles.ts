// Following files that are being written or copied: their size, how fast they grow, and when they stop.
//
// A file joins when the folder listing shows it new or changed a moment ago, when a transfer of this app is
// writing it, or when it is selected with the details pane open. It is checked every few hundred milliseconds
// (just that file, never the rest of the folder) and leaves again once it has not changed for a few seconds.

import { create } from "zustand";
import { api } from "../api";
import { adv } from "../settings/advanced";
import { useStore } from "../store";
import { baseName, joinPath, parentPath } from "./path";

export interface LiveFile {
  path: string;
  /** When it was first seen changing. */
  since: number;
  /** Its size at that moment. */
  startSize: number;
  size: number;
  modified: number | null;
  /** The last time the size or the modified time moved. */
  lastChange: number;
  /** Bytes per second over the last few seconds. */
  rate: number;
  /** The fastest rate seen. */
  peak: number;
  /** Still changing (moved within the "finished after" seconds). */
  growing: boolean;
  samples: [number, number][];
}

interface LiveState {
  files: Record<string, LiveFile>;
}

const NONE: Record<string, LiveFile> = {};
export const useLive = create<LiveState>(() => ({ files: NONE }));

/** Files being followed because something changed them, by path. Not the selected ones: those are added each round. */
const candidates = new Map<string, number>();
const WINDOW_MS = 5000;

/** The rate over the last few seconds, from (time, size) samples. */
export function rateOf(samples: [number, number][], now: number): number {
  const recent = samples.filter(([t]) => now - t <= WINDOW_MS);
  if (recent.length < 2) return 0;
  const [t0, s0] = recent[0];
  const [t1, s1] = recent[recent.length - 1];
  return t1 > t0 && s1 >= s0 ? ((s1 - s0) * 1000) / (t1 - t0) : 0;
}

/** Marks files as worth following (the listing saw them change). */
export function followFiles(paths: string[]): void {
  const now = Date.now();
  for (const p of paths) candidates.set(p, now);
}

/** What the listing says about a folder now against what it said before: the files to follow. */
export function changedSince(
  dir: string,
  now: { name: string; size: number; modified: number | null; isDir: boolean }[],
  before: Map<string, { size: number; modified: number | null }> | null,
  recentMs: number,
  at = Date.now(),
): string[] {
  const out: string[] = [];
  for (const e of now) {
    if (e.isDir) continue;
    const old = before?.get(e.name);
    const recent = e.modified != null && at - e.modified < recentMs;
    // A file that was not there a moment ago is followed even with an old date (a copy keeps the date it had).
    if (old ? old.size !== e.size || old.modified !== e.modified : before ? true : recent) out.push(joinPath(dir, e.name));
  }
  return out;
}

/** The size of a file as it will be when it stops growing is not known; this says what is. */
export function liveOf(path: string): LiveFile | undefined {
  return useLive.getState().files[path];
}

export function intervalFor(settings: Parameters<typeof adv>[0], tier: string | undefined): number {
  const chosen = adv<string>(settings, "live.interval");
  if (chosen && chosen !== "auto") return Math.max(100, Number(chosen) || 1000);
  return tier === "full" ? 500 : tier === "saver" ? 3000 : tier === "balanced" ? 1000 : 1000;
}

export function maxFollowed(settings: Parameters<typeof adv>[0], tier: string | undefined): number {
  const n = adv<number>(settings, "live.maxFiles");
  return tier === "saver" ? Math.min(n, 4) : tier === "balanced" ? Math.min(n, 12) : n;
}

/** Applies one round of answers to what is followed. Returns the paths that just stopped changing. */
export function applyStats(
  files: Record<string, LiveFile>,
  stats: { path: string; exists: boolean; size: number; modified: number | null }[],
  now: number,
  idleMs: number,
): { files: Record<string, LiveFile>; finished: string[] } {
  const next: Record<string, LiveFile> = { ...files };
  const finished: string[] = [];
  for (const s of stats) {
    const old = next[s.path];
    if (!s.exists) {
      delete next[s.path];
      continue;
    }
    if (!old) {
      next[s.path] = { path: s.path, since: now, startSize: s.size, size: s.size, modified: s.modified, lastChange: now, rate: 0, peak: 0, growing: false, samples: [[now, s.size]] };
      continue;
    }
    const moved = s.size !== old.size || s.modified !== old.modified;
    const samples = [...old.samples, [now, s.size] as [number, number]].filter(([t]) => now - t <= WINDOW_MS * 2).slice(-40);
    const lastChange = moved ? now : old.lastChange;
    // Changing now, or changed a moment ago (a copy can pause between chunks).
    const growing = moved || (old.growing && now - lastChange < idleMs);
    const rate = rateOf(samples, now);
    next[s.path] = { ...old, size: s.size, modified: s.modified, lastChange, rate, peak: Math.max(old.peak, rate), growing, samples };
    if (old.growing && !growing) finished.push(s.path);
  }
  return { files: next, finished };
}

let timer: ReturnType<typeof setTimeout> | undefined;
let running = false;

/** Starts following. Safe to call again: only one loop runs. */
export function startLiveTracker(): () => void {
  if (running) return () => {};
  running = true;
  const loop = async () => {
    const st = useStore.getState();
    const settings = st.settings;
    let wait = 1000;
    if (settings && adv<boolean>(settings, "live.enabled") && !document.hidden) {
      const tier = st.budget?.tier;
      wait = intervalFor(settings, tier);
      const idleMs = Math.max(1, adv<number>(settings, "live.idleSeconds")) * 1000;
      const now = Date.now();
      const want = new Set<string>();
      // The files of this app's own transfers that are being written right now.
      for (const t of Object.values(st.transfers)) {
        if (t.phase !== "running" || !t.current) continue;
        const dest = t.current.includes("/") || t.current.includes("\\") ? joinPath(t.destDir, baseName(t.current)) : joinPath(t.destDir, t.current);
        candidates.set(dest, now);
      }
      for (const [p, at] of candidates) {
        const live = useLive.getState().files[p];
        // Dropped once it has been quiet for a while (and is not showing as growing).
        if (now - at > idleMs * 3 && !(live?.growing)) candidates.delete(p);
        else want.add(p);
      }
      // The selection, when the details pane could show it.
      if (adv<boolean>(settings, "live.details") && settings.appearance.showDetailsPane) for (const p of st.selection.slice(0, 40)) want.add(p);
      const list = [...want].slice(0, Math.max(1, maxFollowed(settings, tier)) + st.selection.length);
      if (list.length) {
        try {
          const stats = await api.statMany(list);
          const cur = useLive.getState().files;
          const { files, finished } = applyStats(cur, stats, Date.now(), idleMs);
          // Forget files that are neither followed nor selected nor still growing.
          for (const p of Object.keys(files)) if (!want.has(p) && !files[p].growing) delete files[p];
          useLive.setState({ files });
          if (finished.length) {
            // One last look at the folder and the details, now that the file is complete.
            const dirs = [...new Set(finished.map((p) => parentPath(p) ?? ""))].filter(Boolean);
            useStore.getState().bumpDirs(dirs);
            for (const p of finished) candidates.delete(p);
          }
        } catch {
          /* try again next round */
        }
      } else if (Object.keys(useLive.getState().files).length) {
        useLive.setState({ files: NONE });
      }
    }
    timer = setTimeout(() => void loop(), wait);
  };
  void loop();
  return () => {
    clearTimeout(timer);
    running = false;
  };
}

/** For the file list and the details pane: is this file being written right now? */
export function isGrowing(path: string): boolean {
  return !!useLive.getState().files[path]?.growing;
}

/** Text that changes whenever one of these paths starts, stops or grows: lets a component redraw only when it matters. */
export function liveKey(files: Record<string, LiveFile>, paths: string[]): string {
  let out = "";
  for (const p of paths) {
    const l = files[p];
    if (l?.growing) out += `${p}:${l.size}:${l.modified ?? ""}|`;
  }
  return out;
}

/** Whether this file is being followed and has changed within the last few seconds (read outside React). */
export function isLiveAt(path: string): boolean {
  return isGrowing(path);
}

/** The numbers shown for a growing file. */
export function liveSummary(l: LiveFile, now: number): { added: number; average: number; seconds: number; sinceChange: number } {
  const seconds = Math.max(0, (now - l.since) / 1000);
  const added = Math.max(0, l.size - l.startSize);
  return { added, average: seconds >= 1 ? added / seconds : 0, seconds, sinceChange: Math.max(0, (now - l.lastChange) / 1000) };
}
