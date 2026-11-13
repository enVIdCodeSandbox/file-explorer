// Numbers shown beside a running disk-usage scan: how fast files are being
// found, how fast it is expected to go, how big the whole thing may be.

export interface ScanSample {
  /** Seconds since the scan started. */
  t: number;
  entries: number;
  bytes: number;
}

/** Typical items found per second by media type, used until the scan has measured its own pace. */
export const PRIOR_RATE: Record<string, number> = {
  ssd: 30000,
  hdd: 4000,
  removable: 3000,
  network: 1500,
  unknown: 8000,
};

/** Items per second over the last `window` seconds of samples (0 with too little to go on). */
export function recentRate(samples: ScanSample[], window = 3): number {
  if (samples.length < 2) return 0;
  const last = samples[samples.length - 1];
  const first = samples.find((s) => last.t - s.t <= window) ?? samples[0];
  const dt = last.t - first.t;
  return dt > 0.2 ? Math.max(0, (last.entries - first.entries) / dt) : 0;
}

/**
 * The pace the scan is expected to keep. At the start it is what this kind of
 * drive usually does; as the scan runs, its own average takes over.
 */
export function expectedRate(media: string, entries: number, elapsed: number): number {
  const prior = PRIOR_RATE[media] ?? PRIOR_RATE.unknown;
  if (elapsed <= 0 || entries <= 0) return prior;
  const own = entries / elapsed;
  const weight = Math.min(1, elapsed / 10);
  return prior * (1 - weight) + own * weight;
}

/**
 * How many items the whole drive may hold, from how much space is used and
 * the average size of what has been found so far. Needs a fair sample first.
 */
export function estimateTotal(usedBytes: number, entries: number, bytes: number): number | null {
  if (usedBytes <= 0 || entries < 5000 || bytes <= 0) return null;
  return Math.max(entries, Math.round((usedBytes / bytes) * entries));
}

/** Seconds left at the given pace, or null when it cannot be told. */
export function secondsLeft(total: number | null, entries: number, rate: number): number | null {
  if (total == null || rate <= 0) return null;
  return Math.max(0, (total - entries) / rate);
}

/** An SVG polyline path for the values, scaled to fit width x height (higher value = higher on screen). */
export function sparkPath(values: number[], width: number, height: number, ceiling?: number): string {
  if (values.length < 2) return "";
  const top = Math.max(ceiling ?? 0, ...values, 1e-9);
  const step = width / (values.length - 1);
  return values.map((v, i) => `${i ? "L" : "M"}${(i * step).toFixed(1)},${(height - (Math.max(0, v) / top) * (height - 1) - 0.5).toFixed(1)}`).join(" ");
}

/** Keep only the newest `n` entries. */
export function pushLimited<T>(list: T[], item: T, n: number): T[] {
  const next = [...list, item];
  return next.length > n ? next.slice(next.length - n) : next;
}
