// Which items a selection box touches, worked out from the layout alone so that items scrolled
// out of the rendered window still count. Coordinates are the list's content coordinates.

export interface BandGeometry {
  count: number;
  details: boolean;
  rowHeight: number;
  headerHeight: number;
  /** Details view: the width of a row. */
  rowsWidth: number;
  /** Tile views: tiles per row, and a tile's slot size. */
  columnsPerRow: number;
  tileW: number;
  tileH: number;
}

/** Indexes (in list order) of the items the box `l,t`-`r,b` touches. */
export function touchedIndexes(g: BandGeometry, l: number, t: number, r: number, b: number): number[] {
  const hits: number[] = [];
  const n = g.count;
  if (!n) return hits;
  if (g.details) {
    const left = 8;
    if (r < left || l > left + g.rowsWidth) return hits;
    const from = Math.max(0, Math.floor((t - g.headerHeight) / g.rowHeight));
    const to = Math.min(n - 1, Math.floor((b - g.headerHeight) / g.rowHeight));
    for (let i = from; i <= to; i++) hits.push(i);
    return hits;
  }
  const top = 8; // the space above the first row of tiles
  const rowFrom = Math.max(0, Math.floor((t - top) / g.rowHeight));
  const rowTo = Math.floor((b - top) / g.rowHeight);
  const colFrom = Math.max(0, Math.floor((l - 8) / g.tileW));
  const colTo = Math.min(g.columnsPerRow - 1, Math.floor((r - 8) / g.tileW));
  for (let row = rowFrom; row <= rowTo; row++) {
    const y0 = top + row * g.rowHeight;
    if (b < y0 || t > y0 + g.tileH - 4) continue; // in the gap between rows of tiles
    for (let col = colFrom; col <= colTo; col++) {
      const x0 = 8 + col * g.tileW;
      const i = row * g.columnsPerRow + col;
      if (i >= n || r < x0 || l > x0 + g.tileW) continue;
      hits.push(i);
    }
  }
  return hits;
}
