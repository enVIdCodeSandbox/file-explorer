// How a transfer's speed graph is scaled, so that what it shows agrees with the numbers beside it.

/**
 * Where the time axis ends, in seconds. While a transfer runs: where it is heading (now plus the time left that the card
 * shows), so the empty part of the graph is the time remaining. Once it is over: its last moment, with no room left.
 * Without an estimate yet: a little room beyond now.
 */
export function graphEnd(now: number, etaSecs: number | null | undefined, finished: boolean, ranFor = 0): number {
  if (finished) return Math.max(now, ranFor, 1);
  if (etaSecs != null && Number.isFinite(etaSecs) && etaSecs >= 0) return Math.max(now + etaSecs, now * 1.02 + 2, 5);
  return Math.max(5, now * 1.15);
}

/** The top of a speed axis: about the 95th percentile of the samples with some headroom, so one start-up burst
 * (a drive's cache taking the first gigabyte at 2 GB/s) does not flatten the real line. Values above it are drawn at the top. */
export function robustMax(values: number[], floor: number): number {
  const v = values.filter((x) => Number.isFinite(x) && x >= 0).sort((a, b) => a - b);
  if (!v.length) return floor;
  const p95 = v[Math.min(v.length - 1, Math.floor(v.length * 0.95))];
  return Math.max(floor, p95 * 1.25);
}

export function niceMax(v: number): number {
  if (v <= 0) return 1;
  const pow = 10 ** Math.floor(Math.log10(v));
  const n = v / pow;
  return (n <= 1 ? 1 : n <= 2 ? 2 : n <= 5 ? 5 : 10) * pow;
}

/** A files-per-second figure for an axis, with the decimals a small axis needs. */
export function filesLabel(v: number, axisMax: number): string {
  if (v === 0) return "0";
  if (axisMax >= 10) return String(Math.round(v));
  if (axisMax >= 1) return v.toFixed(1).replace(/\.0$/, "");
  return v.toFixed(2).replace(/0$/, "");
}

// ---------------------------------------------------------------------------
// Zooming and panning
// ---------------------------------------------------------------------------

/** The closest the graph zooms in: one second across. */
export const MIN_SPAN = 1;

/**
 * Which stretch of time the graph shows. `null`: all of it. Otherwise `span` seconds starting at `start`, or, with
 * `start === null`, the latest `span` seconds, sliding along as new speeds arrive (the size stays, the line moves).
 */
export type View = { span: number; start: number | null } | null;

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** The seconds shown, as `[from, to]`. `now`: the last measured moment; `end`: where the whole graph ends. */
export function viewRange(view: View, now: number, end: number, finished: boolean): [number, number] {
  if (!view) return [0, end];
  const span = clamp(view.span, Math.min(MIN_SPAN, end), end);
  if (view.start === null) {
    const right = finished ? end : Math.min(end, Math.max(now, span));
    return [right - span, right];
  }
  const from = clamp(view.start, 0, Math.max(0, end - span));
  return [from, from + span];
}

/**
 * One zoom step. `factor` below 1 zooms in. `frac`: where along the graph the pointer is, 0 to 1.
 * Zooming in near the latest moment of a running job (or in the empty room after it) follows the latest speeds.
 */
export function zoomView(view: View, factor: number, frac: number, now: number, end: number, finished: boolean): View {
  const [from, to] = viewRange(view, now, end, finished);
  const span = to - from;
  const next = clamp(span * factor, Math.min(MIN_SPAN, end), end);
  if (next >= end * 0.999) return null;
  const at = from + clamp(frac, 0, 1) * span;
  // Near the head (a few seconds back) or in the empty room after it.
  const following = view?.start === null || (!finished && at >= now - Math.min(span * 0.1, 3));
  if (following && !finished) return { span: next, start: null };
  return { span: next, start: clamp(at - clamp(frac, 0, 1) * next, 0, end - next) };
}

/** Slide the view by `dt` seconds (later when positive). Reaching the latest moment of a running job follows it again. */
export function panView(view: View, dt: number, now: number, end: number, finished: boolean): View {
  if (!view) return null;
  const [from, to] = viewRange(view, now, end, finished);
  const span = to - from;
  const start = clamp(from + dt, 0, Math.max(0, end - span));
  if (!finished && start + span >= now - 1e-6 && dt >= 0) return { span, start: null };
  return { span, start };
}

/** A moment on the time axis: tenths of a second when the view is only seconds wide. */
export function timeLabel(t: number, span: number): string {
  if (t < 60) return `${span < 10 ? t.toFixed(1).replace(/\.0$/, "") : Math.round(t)} sec`;
  const m = Math.floor(t / 60);
  const s = Math.round(t % 60);
  return s === 60 ? `${m + 1} min` : s ? `${m} min ${s} sec` : `${m} min`;
}

/** Keeps a long job's graph light: when it holds more than `max` points, the older ones are thinned to every other one
 * while the latest `max / 2` stay as measured (so zooming in on the recent speeds still shows every sample). */
export function thinSeries<T>(points: T[], max = 6000): T[] {
  if (points.length <= max) return points;
  const keep = Math.floor(max / 2);
  const old = points.slice(0, points.length - keep).filter((_, i) => i % 2 === 0);
  return [...old, ...points.slice(points.length - keep)];
}
