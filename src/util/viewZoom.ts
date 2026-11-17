// Zooming the folder view. Details and List each have a size of their own (a percentage of the usual one). Zooming in
// makes the rows and text bigger a step at a time, and only when the view is as big as it goes does the next zoom
// move on to the next kind of view (Details, then List, then the icon sizes). Zooming out does the same the other way.

/** The sizes a view steps through, in percent. */
export const ZOOM_PCT = [70, 80, 90, 100, 115, 130, 150, 175] as const;
/** The views that have sizes of their own: the first two levels. */
export const SIZED_LEVELS = 2;

export interface ZoomState {
  /** Index into VIEW_LEVELS (0 Details, 1 List, 2 and up the icon views). */
  level: number;
  /** Details and List sizes, in percent. */
  details: number;
  list: number;
}

const FIRST = 0;
const LAST = ZOOM_PCT.length - 1;

/** The step nearest to a percentage (a saved size that is not one of the steps still works). */
export function nearestStep(pct: number): number {
  let best = 0;
  ZOOM_PCT.forEach((p, i) => {
    if (Math.abs(p - pct) < Math.abs(ZOOM_PCT[best] - pct)) best = i;
  });
  return best;
}

const sizeOf = (s: ZoomState, level: number) => (level === 0 ? s.details : s.list);
const withSize = (s: ZoomState, level: number, pct: number): ZoomState => (level === 0 ? { ...s, details: pct } : { ...s, list: pct });

function one(s: ZoomState, dir: 1 | -1, maxLevel: number): ZoomState {
  if (s.level < SIZED_LEVELS) {
    const i = nearestStep(sizeOf(s, s.level));
    const next = i + dir;
    if (next >= FIRST && next <= LAST) return withSize(s, s.level, ZOOM_PCT[next]);
    // As big (or as small) as this view goes: on to the next view, which starts at its own end of the range.
    const level = s.level + dir;
    if (level < 0 || level > maxLevel) return s;
    const moved = { ...s, level };
    return level < SIZED_LEVELS ? withSize(moved, level, dir > 0 ? ZOOM_PCT[FIRST] : ZOOM_PCT[LAST]) : moved;
  }
  const level = s.level + dir;
  if (level < 0 || level > maxLevel) return s;
  const moved = { ...s, level };
  return level < SIZED_LEVELS ? withSize(moved, level, dir > 0 ? ZOOM_PCT[FIRST] : ZOOM_PCT[LAST]) : moved;
}

/** The view and sizes after `steps` notches of zoom (positive: bigger). */
export function zoomBy(state: ZoomState, steps: number, maxLevel: number): ZoomState {
  let s = state;
  for (let n = 0; n < Math.abs(steps); n++) s = one(s, steps > 0 ? 1 : -1, maxLevel);
  return s;
}

/** The usual size (100%) for the view in use. */
export function resetSize(state: ZoomState): ZoomState {
  return state.level < SIZED_LEVELS ? withSize(state, state.level, 100) : state;
}

/** The scale of the view in use as a factor: 1 for the icon views, which size themselves. */
export function scaleOf(state: ZoomState): number {
  return state.level < SIZED_LEVELS ? sizeOf(state, state.level) / 100 : 1;
}
