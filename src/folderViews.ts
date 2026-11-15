// What is remembered about each folder's view: its sort order, its filter and its columns.
// Kept in the browser's storage, most recently used last, and trimmed to a limit so it cannot grow
// without end. Pure functions, so they can be tested.

import type { Settings } from "./api";
import { adv } from "./settings/advanced";
import type { FilterState, SortKey } from "./store";
import type { ColumnSet } from "./util/columns";
import { normalizeSet } from "./util/columns";

export interface FolderView {
  sort?: { key: SortKey; asc: boolean };
  filter?: FilterState;
  columns?: ColumnSet;
}

export type FolderViews = Record<string, FolderView>;

const KEY = "fast.folderViews";
export const MAX_FOLDERS = 1500;

/** Read what was saved. Anything that does not look right is dropped, not trusted. */
export function readViews(raw: string | null): FolderViews {
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object") return {};
    const out: FolderViews = {};
    for (const [loc, v] of Object.entries(parsed as Record<string, unknown>)) {
      if (!v || typeof v !== "object") continue;
      const r = v as Record<string, unknown>;
      const view: FolderView = {};
      const s = r.sort as { key?: unknown; asc?: unknown } | undefined;
      if (s && typeof s.key === "string" && typeof s.asc === "boolean") view.sort = { key: s.key, asc: s.asc };
      if (r.filter && typeof r.filter === "object") view.filter = r.filter as FilterState;
      if (r.columns && typeof r.columns === "object") view.columns = normalizeSet(r.columns);
      if (view.sort || view.filter || view.columns) out[loc] = view;
    }
    return out;
  } catch {
    return {};
  }
}

/** The views with `loc` changed (`null` forgets that part), `loc` moved to the most recent place. */
export function withView(views: FolderViews, loc: string, patch: { [K in keyof FolderView]?: FolderView[K] | null }, max = MAX_FOLDERS): FolderViews {
  const next: FolderView = { ...views[loc] };
  for (const k of Object.keys(patch) as (keyof FolderView)[]) {
    if (patch[k] == null) delete next[k];
    else (next as Record<string, unknown>)[k] = patch[k];
  }
  const rest = { ...views };
  delete rest[loc];
  const out: FolderViews = Object.keys(next).length ? { ...rest, [loc]: next } : rest;
  const keys = Object.keys(out);
  // Objects keep insertion order, so the oldest are first.
  for (const k of keys.slice(0, Math.max(0, keys.length - max))) delete out[k];
  return out;
}

/** The same views with every folder's own columns forgotten (a column choice for all folders). */
export function withoutColumns(views: FolderViews): FolderViews {
  const out: FolderViews = {};
  for (const [loc, v] of Object.entries(views)) {
    const { columns: _gone, ...rest } = v;
    if (Object.keys(rest).length) out[loc] = rest;
  }
  return out;
}

export function loadViews(): FolderViews {
  try {
    return readViews(localStorage.getItem(KEY));
  } catch {
    return {};
  }
}

export function saveViews(views: FolderViews): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(views));
  } catch {
    /* storage can be unavailable; the views then last until the app closes */
  }
}

/** Does each folder have its own columns? Not when one set is used everywhere. */
export function columnsPerFolder(settings: Settings | null | undefined): boolean {
  return adv<boolean>(settings, "files.perFolderView") && !adv<boolean>(settings, "files.columnsEverywhere");
}
