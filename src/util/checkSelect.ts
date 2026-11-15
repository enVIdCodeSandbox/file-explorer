// Selection with checkboxes: what is ticked stays ticked.

/** A click on an item: ticked becomes unticked and the other way round. */
export function toggled(selected: Set<string>, name: string): Set<string> {
  const next = new Set(selected);
  next.has(name) ? next.delete(name) : next.add(name);
  return next;
}

/** Shift + click from `anchor` to `to`: every item in between is turned over relative to `base`
 *  (what was ticked when the anchor was clicked). The anchor itself keeps the state its own click gave it,
 *  so it is not turned over again. */
export function flipRange(names: string[], base: Set<string>, anchor: number, to: number): Set<string> {
  const next = new Set(base);
  const [lo, hi] = [Math.min(anchor, to), Math.max(anchor, to)];
  for (let k = lo; k <= hi; k++) {
    const name = names[k];
    if (name === undefined || k === anchor) continue;
    next.has(name) ? next.delete(name) : next.add(name);
  }
  return next;
}
