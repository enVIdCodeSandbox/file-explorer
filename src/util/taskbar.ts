// Progress on the app's icon in the taskbar (Windows), the dock (macOS) or the launcher (some Linux desktops) while
// transfers, deletes, archive jobs and automated tasks run: green and filling up, yellow when paused, red when one
// failed, and moving back and forth while the size is not known yet.

export type BarStatus = "none" | "indeterminate" | "normal" | "paused" | "error";
export interface Bar {
  status: BarStatus;
  /** 0 to 100. */
  progress: number;
}
/** One thing that is running: how far it is, in whatever unit it counts in (`total` 0 if not known yet). */
export interface Work {
  done: number;
  total: number;
  paused?: boolean;
}

/** What the icon should show for everything running now (and whether something that just ended failed). */
export function barFor(work: Work[], failed = false): Bar {
  if (!work.length) return failed ? { status: "error", progress: 100 } : { status: "none", progress: 0 };
  const known = work.filter((w) => w.total > 0);
  const progress = known.length ? Math.round((100 * known.reduce((a, w) => a + Math.min(1, Math.max(0, w.done / w.total)), 0)) / known.length) : 0;
  if (work.every((w) => w.paused)) return { status: "paused", progress };
  if (!known.length) return { status: "indeterminate", progress: 0 };
  return { status: failed ? "error" : "normal", progress: Math.min(100, Math.max(0, progress)) };
}

export const sameBar = (a: Bar | null, b: Bar) => !!a && a.status === b.status && a.progress === b.progress;
