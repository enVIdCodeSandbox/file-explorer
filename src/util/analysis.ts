// Pure helpers for the disk usage view: file types, isolating a type,
// hiding small files, removing deleted items, and grouping types.

import type { Analysis, SizeNode } from "../api";
import { kindOf } from "./format";
import { isInside } from "./path";

export const NO_EXT = "(no extension)";
const OTHER = "(other)";

/** The file type of a name, matching the scanner: lower case, no dot. */
export function extOf(name: string): string {
  const i = name.lastIndexOf(".");
  if (i <= 0 || i + 1 >= name.length || name.length - i > 12) return NO_EXT;
  return name.slice(i + 1).toLowerCase();
}

export interface TypeRow {
  ext: string;
  bytes: number;
}

/** What a node is made of, largest first. Files are their own single type. */
export function nodeTypes(node: SizeNode, names: string[]): TypeRow[] {
  if (!node.isDir) return node.name === OTHER ? [] : [{ ext: extOf(node.name), bytes: node.size }];
  return node.types.map(([id, bytes]) => ({ ext: names[id] ?? OTHER, bytes })).sort((a, b) => b.bytes - a.bytes);
}

/** The folder the person is looking at, or the whole tree if it is not there. */
export function findNode(root: SizeNode, path: string): SizeNode | null {
  if (root.path === path) return root;
  for (const c of root.children) {
    if (c.isDir && c.path && isInside(path, c.path)) {
      const hit = findNode(c, path);
      if (hit) return hit;
    }
  }
  return null;
}

const other = (size: number): SizeNode => ({ name: OTHER, path: "", size, isDir: false, children: [], types: [] });

/**
 * Keep only the given file types. Folders shrink to the bytes of those types;
 * what the pruned tree cannot show by name stays as one grey "(other)" tile.
 */
export function isolate(node: SizeNode, exts: ReadonlySet<string>, names: string[]): SizeNode | null {
  if (!node.isDir) return node.name !== OTHER && exts.has(extOf(node.name)) ? node : null;
  const own = node.types.reduce((sum, [id, bytes]) => sum + (exts.has(names[id] ?? OTHER) ? bytes : 0), 0);
  const children = node.children.map((c) => isolate(c, exts, names)).filter((c): c is SizeNode => c !== null);
  const shown = children.reduce((sum, c) => sum + c.size, 0);
  const size = Math.max(own, shown);
  if (size <= 0) return null;
  if (size > shown && children.length) children.push(other(size - shown));
  return { ...node, size, children, types: node.types.filter(([id]) => exts.has(names[id] ?? OTHER)) };
}

/** Fold files smaller than `min` bytes into "(other)" so the picture shows only what matters. */
export function hideSmall(node: SizeNode, min: number): SizeNode {
  if (min <= 0 || !node.isDir || node.children.length === 0) return node;
  let folded = 0;
  const children: SizeNode[] = [];
  for (const c of node.children) {
    if (c.name === OTHER) folded += c.size;
    else if (!c.isDir && c.size < min) folded += c.size;
    else children.push(c.isDir ? hideSmall(c, min) : c);
  }
  if (folded > 0) children.push(other(folded));
  return { ...node, children };
}

/** The tree without items the person deleted since the scan. */
export function withoutPaths(node: SizeNode, removed: ReadonlySet<string>): SizeNode {
  if (removed.size === 0 || !node.isDir) return node;
  let lost = 0;
  const children: SizeNode[] = [];
  for (const c of node.children) {
    if (c.path && removed.has(c.path)) lost += c.size;
    else if (c.isDir && c.path && [...removed].some((r) => isInside(r, c.path))) {
      const next = withoutPaths(c, removed);
      lost += c.size - next.size;
      children.push(next);
    } else children.push(c);
  }
  return lost ? { ...node, size: Math.max(0, node.size - lost), children } : node;
}

export type Category = "Images" | "Video" | "Audio" | "Documents" | "Archives" | "Code" | "Programs" | "Other";
export const CATEGORIES: Category[] = ["Images", "Video", "Audio", "Documents", "Archives", "Code", "Programs", "Other"];

/** Group a file type into a broad category. */
export function categoryOf(ext: string): Category {
  if (ext === NO_EXT) return "Other";
  switch (kindOf(`x.${ext}`, false)) {
    case "image":
      return "Images";
    case "video":
      return "Video";
    case "audio":
      return "Audio";
    case "doc":
    case "sheet":
    case "slides":
    case "text":
      return "Documents";
    case "archive":
      return "Archives";
    case "code":
    case "data":
      return "Code";
    case "exe":
      return "Programs";
    default:
      return "Other";
  }
}

export type ListSort = "size" | "name" | "type" | "modified";

export interface ListFilter {
  text: string;
  minBytes: number;
  exts: ReadonlySet<string>;
}

export function matchesFilter(name: string, size: number, f: ListFilter): boolean {
  if (f.minBytes > 0 && size < f.minBytes) return false;
  if (f.exts.size > 0 && !f.exts.has(extOf(name))) return false;
  return !f.text.trim() || name.toLowerCase().includes(f.text.trim().toLowerCase());
}

export type LargeFile = Analysis["largest"][number];

export function sortLargest(list: LargeFile[], key: ListSort, asc: boolean): LargeFile[] {
  const dir = asc ? 1 : -1;
  const cmp: Record<ListSort, (a: LargeFile, b: LargeFile) => number> = {
    size: (a, b) => a.size - b.size,
    name: (a, b) => a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" }),
    type: (a, b) => extOf(a.name).localeCompare(extOf(b.name)) || a.size - b.size,
    modified: (a, b) => a.modified - b.modified,
  };
  return [...list].sort((a, b) => dir * cmp[key](a, b));
}
