// Split view: the folder view can be cut into up to eight panes, each divider dragged to resize.
//
// A layout is a tree. A leaf is a pane (0 is the normal file list, 1 to 7 are the extra panes); a branch lays its
// children out in a row (side by side) or a column (stacked). A branch has the share of the space each child takes, and
// a divider between neighbours moves share from one to the other, never below a minimum.

export type Dir = "row" | "col";
export type Side = "left" | "right" | "top" | "bottom";
export type Branch = { dir: Dir; kids: Node[]; /** Shares of the space; they add up to 1 (equal if missing). */ shares?: number[] };
export type Node = number | Branch;

export const MAX_PANES = 8;
/** Stands for the pane a dragged tab would become, while it is only shown, not yet made. */
export const GHOST = -1;

export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

export const isBranch = (n: Node): n is Branch => typeof n !== "number";

/** The panes in a tree, in the order they are drawn. */
export function leaves(tree: Node): number[] {
  return isBranch(tree) ? tree.kids.flatMap(leaves) : [tree];
}

/** Shares that add up to 1. */
export function normalize(weights: number[]): number[] {
  const total = weights.reduce((a, b) => a + b, 0);
  return total > 0 ? weights.map((w) => w / total) : weights.map(() => 1 / weights.length);
}

/** The shares of one branch: its own if they fit it, otherwise equal ones. */
export function sharesOf(node: Branch): number[] {
  const s = node.shares;
  if (Array.isArray(s) && s.length === node.kids.length && s.every((x) => typeof x === "number" && x > 0 && Number.isFinite(x))) return normalize(s);
  return node.kids.map(() => 1 / node.kids.length);
}

const sideDir = (side: Side): Dir => (side === "left" || side === "right" ? "row" : "col");

/**
 * Put a new pane beside `target`, on the given side of it. Only the pane it is put beside gives up room (half of its
 * share), the others keep theirs.
 */
export function insertPane(tree: Node, target: number, side: Side, slot: number): Node {
  const dir = sideDir(side);
  const before = side === "left" || side === "top";
  const walk = (n: Node): Node => {
    if (!isBranch(n)) return n === target ? { dir, kids: before ? [slot, target] : [target, slot] } : n;
    const i = n.kids.indexOf(target);
    if (i >= 0 && n.dir === dir) {
      const shares = sharesOf(n);
      const half = shares[i] / 2;
      const at = before ? i : i + 1;
      const kids = [...n.kids];
      const next = [...shares];
      next[i] = half;
      kids.splice(at, 0, slot);
      next.splice(at, 0, half);
      return { dir: n.dir, kids, shares: next };
    }
    return { ...n, kids: n.kids.map(walk) };
  };
  return walk(tree);
}

/** Take a pane out; the space it had goes to the others in proportion, and a branch left with one child dissolves. */
export function removePane(tree: Node, slot: number): Node {
  const walk = (n: Node): Node | null => {
    if (!isBranch(n)) return n === slot ? null : n;
    const shares = sharesOf(n);
    const kids: Node[] = [];
    const kept: number[] = [];
    n.kids.forEach((k, i) => {
      const w = walk(k);
      if (w !== null) {
        kids.push(w);
        kept.push(shares[i]);
      }
    });
    if (!kids.length) return null;
    if (kids.length === 1) return kids[0];
    // A child that runs the same way is folded into this branch.
    const flat: Node[] = [];
    const flatShares: number[] = [];
    kids.forEach((k, i) => {
      if (isBranch(k) && k.dir === n.dir) {
        const inner = sharesOf(k);
        k.kids.forEach((kk, j) => {
          flat.push(kk);
          flatShares.push(kept[i] * inner[j]);
        });
      } else {
        flat.push(k);
        flatShares.push(kept[i]);
      }
    });
    return { dir: n.dir, kids: flat, shares: normalize(flatShares) };
  };
  return walk(tree) ?? 0;
}

/** Every branch of a tree, with the path that names it: "" is the top one, "1" the second child of it, "1.0"… */
export function branchAt(tree: Node, path: string): Branch | null {
  let n: Node = tree;
  if (path) {
    for (const part of path.split(".")) {
      if (!isBranch(n)) return null;
      n = n.kids[Number(part)];
      if (n === undefined) return null;
    }
  }
  return isBranch(n) ? n : null;
}

/** The tree with the shares of one branch replaced. */
export function setShares(tree: Node, path: string, shares: number[]): Node {
  const walk = (n: Node, at: string): Node => {
    if (!isBranch(n)) return n;
    if (at === path) return { ...n, shares };
    return { ...n, kids: n.kids.map((k, i) => walk(k, at ? `${at}.${i}` : String(i))) };
  };
  return walk(tree, "");
}

/** The tree with every branch's shares forgotten, so all are even again. */
export function evenOut(tree: Node): Node {
  return isBranch(tree) ? { dir: tree.dir, kids: tree.kids.map(evenOut) } : tree;
}

// ---- Where everything goes ------------------------------------------------------------------------------

export interface Handle {
  /** The branch it belongs to, and the child before it. */
  path: string;
  index: number;
  dir: Dir;
  box: Box;
  /** The room the children of the branch share (its size less the gaps), for turning a drag into a share. */
  room: number;
  shares: number[];
  kids: number;
}

/** The box of every pane and the handle between neighbours, in a box, with `gap` pixels between panes. */
export function layout(tree: Node, box: Box, gap: number): { panes: Record<number, Box>; handles: Handle[] } {
  const panes: Record<number, Box> = {};
  const handles: Handle[] = [];
  const walk = (n: Node, b: Box, at: string) => {
    if (!isBranch(n)) {
      panes[n] = b;
      return;
    }
    const shares = sharesOf(n);
    const row = n.dir === "row";
    const room = Math.max(0, (row ? b.w : b.h) - gap * (n.kids.length - 1));
    let offset = 0;
    n.kids.forEach((kid, i) => {
      const size = room * shares[i];
      const kb: Box = row ? { x: b.x + offset, y: b.y, w: size, h: b.h } : { x: b.x, y: b.y + offset, w: b.w, h: size };
      walk(kid, kb, at ? `${at}.${i}` : String(i));
      offset += size;
      if (i < n.kids.length - 1) {
        handles.push({
          path: at,
          index: i,
          dir: n.dir,
          box: row ? { x: b.x + offset, y: b.y, w: gap, h: b.h } : { x: b.x, y: b.y + offset, w: b.w, h: gap },
          room,
          shares,
          kids: n.kids.length,
        });
        offset += gap;
      }
    });
  };
  walk(tree, box, "");
  return { panes, handles };
}

/**
 * Drag the divider after child `i` by `delta` (a share of the whole branch). Neither neighbour goes below `min`, so
 * a pane can be made smaller only up to that point, and the others keep their shares.
 */
export function drag(shares: number[], i: number, delta: number, min: number): number[] {
  if (i < 0 || i >= shares.length - 1) return shares;
  const total = shares[i] + shares[i + 1];
  // A pane already below the minimum (the minimum was raised) is not pushed up, only kept from shrinking further.
  const lowA = Math.min(min, shares[i]);
  const lowB = Math.min(min, shares[i + 1]);
  const a = Math.max(lowA, Math.min(total - lowB, shares[i] + delta));
  const out = [...shares];
  out[i] = a;
  out[i + 1] = total - a;
  return out;
}

/** The smallest share allowed: the percentage chosen, but never so large that the panes could not all fit, and at
 * least `minPx` of the `sizePx` there is. */
export function minShare(percent: number, panes: number, sizePx: number, minPx = 120): number {
  const fromPercent = Math.max(0.05, Math.min(0.45, percent / 100));
  const fromPixels = sizePx > 0 ? minPx / sizePx : 0;
  return Math.min(Math.max(fromPercent, fromPixels), 1 / Math.max(2, panes) - 0.001);
}

// ---- Ready-made layouts ---------------------------------------------------------------------------------

export interface Preset {
  id: string;
  label: string;
  panes: number;
  /** The layout with `slots` (the first is the normal file list) in the places of the preset, large ones first. */
  build: (slots: number[]) => Node;
}

const row = (kids: Node[], shares?: number[]): Node => ({ dir: "row", kids, shares });
const col = (kids: Node[], shares?: number[]): Node => ({ dir: "col", kids, shares });
const seq = (from: number, to: number) => Array.from({ length: to - from }, (_, i) => from + i);
const fill = (n: Node, slots: number[]): Node => (isBranch(n) ? { ...n, kids: n.kids.map((k) => fill(k, slots)) } : slots[n]);

/** The shape of a tree without the panes in it: two layouts with the same shape look alike. */
export function shapeKey(n: Node): string {
  return isBranch(n) ? `${n.dir}(${n.kids.map(shapeKey).join(",")})` : "x";
}

/** Every ready-made layout for exactly `n` panes (2 to 8). */
export function presetsFor(n: number): Preset[] {
  if (n < 2 || n > MAX_PANES) return [];
  const shapes: { id: string; label: string; tree: Node }[] = [
    { id: "side", label: "Side by side", tree: row(seq(0, n)) },
    { id: "stack", label: "Stacked", tree: col(seq(0, n)) },
  ];
  if (n >= 3) {
    const rest = seq(1, n);
    shapes.push(
      { id: "big-left", label: "One large on the left", tree: row([0, col(rest)], [0.6, 0.4]) },
      { id: "big-right", label: "One large on the right", tree: row([col(rest), 0], [0.4, 0.6]) },
      { id: "big-top", label: "One large on top", tree: col([0, row(rest)], [0.6, 0.4]) },
      { id: "big-bottom", label: "One large below", tree: col([row(rest), 0], [0.4, 0.6]) },
    );
  }
  for (const c of [2, 3, 4]) {
    if (c >= n) continue;
    const rows = Math.ceil(n / c);
    const last = n - (rows - 1) * c;
    if (rows < 2 || (last !== c && last < 2)) continue;
    const groups = Array.from({ length: rows }, (_, r) => seq(r * c, Math.min(n, (r + 1) * c)));
    shapes.push({ id: `grid-across-${c}`, label: `Grid, ${c} across`, tree: col(groups.map((g) => (g.length === 1 ? g[0] : row(g)))) });
    if (!(last === c && rows === c)) shapes.push({ id: `grid-down-${c}`, label: `Grid, ${c} down`, tree: row(groups.map((g) => (g.length === 1 ? g[0] : col(g)))) });
  }
  const seen = new Set<string>();
  return shapes
    .filter((s) => {
      const k = shapeKey(s.tree);
      if (seen.has(k)) return false;
      seen.add(k);
      return true;
    })
    .map((s) => ({ id: `${n}-${s.id}`, label: s.label, panes: n, build: (slots) => fill(s.tree, slots) }));
}

// ---- Dropping a tab -------------------------------------------------------------------------------------

export type Zone = { target: number; side: Side | "merge" };
export interface Rect {
  left: number;
  top: number;
  width: number;
  height: number;
}

/**
 * Which part of which pane a pointer is over: the middle puts the tab among that pane's tabs, and the rest of the pane
 * is cut by its diagonals into four sides, each of which would put a new pane there.
 */
export function zoneAt(x: number, y: number, rects: Record<number, Rect>, can: { split: (target: number) => boolean; merge: (target: number) => boolean }): Zone | null {
  for (const [key, r] of Object.entries(rects)) {
    if (x < r.left || x > r.left + r.width || y < r.top || y > r.top + r.height) continue;
    const target = Number(key);
    const rx = (x - r.left) / Math.max(1, r.width);
    const ry = (y - r.top) / Math.max(1, r.height);
    const merge: Zone | null = can.merge(target) ? { target, side: "merge" } : null;
    if (rx > 0.28 && rx < 0.72 && ry > 0.28 && ry < 0.72) return merge;
    if (!can.split(target)) return merge;
    const side: Side = Math.abs(rx - 0.5) > Math.abs(ry - 0.5) ? (rx < 0.5 ? "left" : "right") : ry < 0.5 ? "top" : "bottom";
    return { target, side };
  }
  return null;
}

// ---- Saved layouts --------------------------------------------------------------------------------------

/** A layout read back from storage: only if it is sound (panes 0 to 7, each once, with 0 among them). */
export function parseTree(raw: unknown): Node {
  try {
    const data = typeof raw === "string" && raw ? JSON.parse(raw) : raw;
    const ok = (n: unknown): n is Node => {
      if (typeof n === "number") return Number.isInteger(n) && n >= 0 && n < MAX_PANES;
      if (!n || typeof n !== "object") return false;
      const b = n as Branch;
      return (b.dir === "row" || b.dir === "col") && Array.isArray(b.kids) && b.kids.length >= 2 && b.kids.every(ok);
    };
    if (!ok(data)) return 0;
    const all = leaves(data);
    return new Set(all).size === all.length && all.includes(0) ? data : 0;
  } catch {
    return 0;
  }
}
