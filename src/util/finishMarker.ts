// Where the "Typical finish" mark sits on a transfer's speed graph. Early on it is what similar earlier transfers took;
// as the transfer gets further the mark moves to where this transfer is really heading (now plus the time left), and
// by about three quarters done it sits on that, ending exactly on the finish when the transfer ends.

const smooth = (x: number) => {
  const t = Math.min(1, Math.max(0, x));
  return t * t * (3 - 2 * t);
};

/**
 * @param typical seconds similar earlier transfers took
 * @param now seconds this transfer has run (the last point of the graph)
 * @param etaSecs seconds left by this transfer's own measurements, if known
 * @param fraction how far along it is, 0 to 1
 */
export function finishMark(typical: number, now: number, etaSecs: number | null | undefined, fraction: number): number {
  if (fraction >= 1) return now;
  const live = etaSecs != null && etaSecs >= 0 ? now + etaSecs : fraction > 0.05 ? now / fraction : null;
  if (live == null) return typical;
  const w = smooth((fraction - 0.1) / 0.65);
  return typical * (1 - w) + live * w;
}
