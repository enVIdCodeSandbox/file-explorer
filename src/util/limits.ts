// The smallest and largest sizes things that can be resized may have, in one place, so a drag, a saved size and the
// stylesheet all agree and a window made small (or a size saved on a bigger screen) cannot break the layout.

import type { Dock, PanelId } from "./layout";

type Range = readonly [min: number, max: number];

/** [smallest, largest] in pixels: the width of a panel on the left or right, or its height on the top or bottom. */
export const PANEL_LIMITS: Partial<Record<PanelId, { side: Range; edge: Range }>> = {
  // Below 240 wide the labels and their values no longer fit side by side; above 640 it only takes room from the files.
  details: { side: [240, 640], edge: [120, 420] },
  terminal: { side: [300, 960], edge: [110, 640] },
};

export const SIDEBAR_LIMITS = { min: 150, max: 480, collapsed: 52 } as const;

/** The file list always keeps at least this much, whatever the panels around it are set to. */
export const MIN_CONTENT = { w: 280, h: 160 } as const;

/** Narrower than this, panels beside the files (details, terminal) step aside: the navigation pane, such a panel at its smallest and the files' minimum no longer fit. */
export const NARROW_WINDOW = 760;

/** A column is never wider than this. */
export const COLUMN_MAX_WIDTH = 1200;

/** Size limits of a panel that floats above the window. */
export const FLOAT_LIMITS = { w: [200, 1400], h: [60, 1000] } as const satisfies Record<string, Range>;

export const isSideDock = (dock: Dock | string) => dock === "left" || dock === "right";

/** The smallest and largest size of a docked panel (null when it cannot be resized or is not docked on an edge). */
export function panelRange(id: PanelId, dock: Dock | string): Range | null {
  const l = PANEL_LIMITS[id];
  if (!l) return null;
  if (isSideDock(dock)) return l.side;
  return dock === "top" || dock === "bottom" ? l.edge : null;
}

/**
 * A size kept inside what the panel allows. `room` is the room the window gives along that direction: a panel may take
 * up to 60% of it (never less than its own smallest size).
 */
export function clampPanelSize(id: PanelId, dock: Dock | string, size: number, room?: number): number {
  const r = panelRange(id, dock);
  if (!r) return Math.round(size);
  const hi = room && room > 0 ? Math.max(r[0], Math.min(r[1], Math.floor(room * 0.6))) : r[1];
  return Math.round(Math.max(r[0], Math.min(hi, size)));
}

export const clampFloat = (w: number, h: number) => ({
  w: Math.round(Math.max(FLOAT_LIMITS.w[0], Math.min(FLOAT_LIMITS.w[1], w))),
  h: Math.round(Math.max(FLOAT_LIMITS.h[0], Math.min(FLOAT_LIMITS.h[1], h))),
});

/** The navigation pane's width, kept in range (the folded width is its own thing). */
export const clampSidebar = (w: number) => Math.round(Math.max(SIDEBAR_LIMITS.min, Math.min(SIDEBAR_LIMITS.max, w)));
