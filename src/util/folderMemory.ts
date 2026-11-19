// Is something about a folder's view shared by every folder, or remembered for each folder on its own?
// Four things can be chosen separately: the sort order, the filter, the columns and the view (Details, List, icons).

import type { Settings } from "../api";

export type MemoryAspect = "sort" | "filter" | "columns" | "view";
export type MemoryScope = "shared" | "folder";

export const MEMORY_ASPECTS: MemoryAspect[] = ["sort", "filter", "columns", "view"];

/** The setting key for one of them. */
export const scopeKey = (a: MemoryAspect) => `files.scope.${a}`;

/**
 * How one thing is kept. Read from what was chosen; where nothing was chosen yet it follows the older options
 * (each folder keeping its own order, filter and columns, or one view everywhere), so nothing changes for a
 * person who set those. New installs: sort, filter and columns for each folder, the view shared.
 */
export function scopeOf(settings: Settings | null | undefined, aspect: MemoryAspect): MemoryScope {
  const a = (settings?.advanced ?? {}) as Record<string, unknown>;
  const chosen = a[scopeKey(aspect)];
  if (chosen === "shared" || chosen === "folder") return chosen;
  const perFolder = a["files.perFolderView"] !== false;
  switch (aspect) {
    case "sort":
    case "filter":
      return perFolder ? "folder" : "shared";
    case "columns":
      return perFolder && a["files.columnsEverywhere"] !== true ? "folder" : "shared";
    case "view":
      return a["files.rememberView"] === false ? "folder" : "shared";
  }
}

/** A signature of all four, to notice when any changed. */
export const scopesSignature = (settings: Settings | null | undefined) => MEMORY_ASPECTS.map((a) => scopeOf(settings, a)[0]).join("");

export const isPerFolder = (settings: Settings | null | undefined, aspect: MemoryAspect) => scopeOf(settings, aspect) === "folder";
