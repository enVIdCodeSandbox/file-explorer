// Squarified treemap layout (Bruls, Huizing, van Wijk). Produces rectangles
// whose areas are proportional to sizes and whose aspect ratios stay close to
// square, which is what makes a WinDirStat-style map readable.

import type { SizeNode } from "../api";

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Tile extends Rect {
  node: SizeNode;
  depth: number;
}

function worst(row: number[], side: number): number {
  const sum = row.reduce((a, b) => a + b, 0);
  const max = Math.max(...row);
  const min = Math.min(...row);
  const s2 = side * side;
  return Math.max((s2 * max) / (sum * sum), (sum * sum) / (s2 * min));
}

/** Lay out `sizes` (any order) inside `rect`. Returns one rect per input. */
export function squarify(sizes: number[], rect: Rect): Rect[] {
  const total = sizes.reduce((a, b) => a + b, 0);
  const out: Rect[] = sizes.map(() => ({ x: rect.x, y: rect.y, w: 0, h: 0 }));
  if (total <= 0 || rect.w <= 0 || rect.h <= 0) return out;

  const order = sizes.map((_, i) => i).sort((a, b) => sizes[b] - sizes[a]);
  const scale = (rect.w * rect.h) / total;
  const areas = order.map((i) => sizes[i] * scale);

  let { x, y, w, h } = rect;
  let i = 0;
  while (i < areas.length) {
    const side = Math.min(w, h);
    const row = [areas[i]];
    let j = i + 1;
    while (j < areas.length && areas[j] > 0 && worst([...row, areas[j]], side) <= worst(row, side)) {
      row.push(areas[j]);
      j++;
    }
    const rowSum = row.reduce((a, b) => a + b, 0);
    const thickness = side > 0 ? rowSum / side : 0;
    let offset = 0;
    for (let k = 0; k < row.length; k++) {
      const length = thickness > 0 ? row[k] / thickness : 0;
      out[order[i + k]] =
        w >= h
          ? { x, y: y + offset, w: thickness, h: length }
          : { x: x + offset, y, w: length, h: thickness };
      offset += length;
    }
    if (w >= h) {
      x += thickness;
      w -= thickness;
    } else {
      y += thickness;
      h -= thickness;
    }
    i = j;
  }
  return out;
}

/** Recursively lay out a size tree into leaf tiles. */
export function layoutTree(root: SizeNode, rect: Rect, minPixels = 3): Tile[] {
  const tiles: Tile[] = [];
  const walk = (node: SizeNode, r: Rect, depth: number) => {
    if (!node.children.length || r.w < minPixels * 2 || r.h < minPixels * 2) {
      tiles.push({ ...r, node, depth });
      return;
    }
    const rects = squarify(
      node.children.map((c) => c.size),
      r,
    );
    node.children.forEach((child, i) => {
      const cr = rects[i];
      if (cr.w >= 1 && cr.h >= 1) walk(child, cr, depth + 1);
    });
  };
  walk(root, rect, 0);
  return tiles;
}

/** Stable colour per file type so the same kind of file looks the same everywhere. */
export function colorFor(name: string, isDir: boolean): string {
  if (name === "(other)") return "hsl(0 0% 38%)";
  const dot = name.lastIndexOf(".");
  const key = isDir ? "dir" : dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
  let h = 0;
  for (let i = 0; i < key.length; i++) h = (h * 31 + key.charCodeAt(i)) >>> 0;
  return `hsl(${h % 360} 55% ${isDir ? 42 : 52}%)`;
}
