// Sorting and filtering of a folder listing. Pure functions so they can be
// tested and run on every keystroke without the UI noticing.

import type { FileEntry } from "../api";
import type { FilterState, SortKey } from "../store";
import { extensionOf, kindOf, typeLabel } from "./format";

let collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

/** Sorting preferences (Advanced settings). */
export function configureSort(opts: { natural: boolean; caseSensitive: boolean }) {
  collator = new Intl.Collator(undefined, {
    numeric: opts.natural,
    sensitivity: opts.caseSensitive ? "variant" : "base",
    caseFirst: opts.caseSensitive ? "upper" : "false",
  });
}

/** Where to read a column's value when it comes from the files themselves (see util/columns). */
export interface ExtraValues {
  kind: "text" | "date" | "size" | "number";
  get: (name: string) => string | undefined;
}

export function sortEntries(entries: FileEntry[], key: SortKey, asc: boolean, foldersFirst: boolean, extra?: ExtraValues): FileEntry[] {
  const dir = asc ? 1 : -1;
  const cmp = (a: FileEntry, b: FileEntry): number => {
    if (foldersFirst && a.isDir !== b.isDir) return a.isDir ? -1 : 1;
    let r = 0;
    if (extra) {
      const x = extra.get(a.name), y = extra.get(b.name);
      // Items with no value for it go last when ascending, first when descending (like blanks in Explorer).
      if ((x === undefined || x === "") !== (y === undefined || y === "")) return x === undefined || x === "" ? dir : -dir;
      if (x !== undefined && y !== undefined && x !== "" && y !== "") {
        r = extra.kind === "text" ? collator.compare(x, y) : (Number(x) || 0) - (Number(y) || 0);
        return (r || collator.compare(a.name, b.name)) * dir;
      }
      return collator.compare(a.name, b.name) * dir;
    }
    if (key === "size") r = a.size - b.size;
    else if (key === "modified") r = (a.modified ?? 0) - (b.modified ?? 0);
    else if (key === "created") r = (a.created ?? 0) - (b.created ?? 0);
    else if (key === "type") r = collator.compare(typeLabel(a.name, a.isDir), typeLabel(b.name, b.isDir));
    else if (key === "extension") r = collator.compare(extensionOf(a.name), extensionOf(b.name));
    return (r || collator.compare(a.name, b.name)) * dir;
  };
  return [...entries].sort(cmp);
}

const DAY = 86_400_000;

export function isFiltering(f: FilterState): boolean {
  return (
    !!f.text || f.kinds.length > 0 || f.extensions.length > 0 || f.modified !== "any" || f.size !== "any" ||
    !!f.notText.trim() || f.notKinds.length > 0 || f.notExtensions.length > 0 || f.notModified !== "any" || f.notSize !== "any"
  );
}

/** Is this modified time inside the named range ("older" means more than a year ago)? */
function inRange(range: string, m: number, cutoffs: Record<string, number>): boolean {
  return range === "older" ? m < cutoffs.year : m >= cutoffs[range];
}

/** Does a file's size fall in the named class? Folders have no size class. */
function inSizeClass(cls: string, s: number): boolean {
  if (cls === "empty") return s === 0;
  if (cls === "small") return s > 0 && s < 1024 * 1024;
  if (cls === "medium") return s >= 1024 * 1024 && s < 100 * 1024 * 1024;
  return s >= 100 * 1024 * 1024; // large
}

export function applyFilter(entries: FileEntry[], f: FilterState, now = Date.now()): FileEntry[] {
  if (!isFiltering(f)) return entries;
  const text = f.text.trim().toLowerCase();
  const notText = f.notText.trim().toLowerCase();
  const startOfToday = new Date(now).setHours(0, 0, 0, 0);
  const cutoffs: Record<string, number> = { today: startOfToday, week: now - 7 * DAY, month: now - 30 * DAY, year: now - 365 * DAY };
  return entries.filter((e) => {
    if (text && !e.name.toLowerCase().includes(text)) return false;
    if (f.kinds.length && !f.kinds.includes(kindOf(e.name, e.isDir))) return false;
    if (f.extensions.length && (e.isDir || !f.extensions.includes(extensionOf(e.name)))) return false;
    if (f.modified !== "any" && !inRange(f.modified, e.modified ?? 0, cutoffs)) return false;
    if (f.size !== "any" && (e.isDir || !inSizeClass(f.size, e.size))) return false;
    // ---- "is not" ----
    if (notText && e.name.toLowerCase().includes(notText)) return false;
    if (f.notKinds.length && f.notKinds.includes(kindOf(e.name, e.isDir))) return false;
    if (f.notExtensions.length && !e.isDir && f.notExtensions.includes(extensionOf(e.name))) return false;
    if (f.notModified !== "any" && inRange(f.notModified, e.modified ?? 0, cutoffs)) return false;
    if (f.notSize !== "any" && !e.isDir && inSizeClass(f.notSize, e.size)) return false;
    return true;
  });
}

/** What kinds and extensions are present, with counts, to offer as filters. */
export function facets(entries: FileEntry[]): { kinds: [string, number][]; extensions: [string, number][] } {
  const kinds = new Map<string, number>();
  const exts = new Map<string, number>();
  for (const e of entries) {
    const k = kindOf(e.name, e.isDir);
    kinds.set(k, (kinds.get(k) ?? 0) + 1);
    if (!e.isDir) {
      const x = extensionOf(e.name);
      if (x) exts.set(x, (exts.get(x) ?? 0) + 1);
    }
  }
  const top = (m: Map<string, number>, n: number) => [...m.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).slice(0, n);
  return { kinds: top(kinds, 12), extensions: top(exts, 14) };
}

const SIZE_TERMS: Record<string, string[]> = { empty: ["size:<1b"], small: ["size:>1b", "size:<1mb"], medium: ["size:>1mb", "size:<100mb"], large: ["size:>100mb"] };

/**
 * The same filter written for the search engine, used when "include
 * subfolders" is on. Date filters are applied to the results afterwards
 * because the engine has no date syntax.
 */
export function toSearchQuery(f: FilterState): string {
  const parts: string[] = [];
  if (f.text.trim()) parts.push(f.text.trim());
  if (f.kinds.length === 1 && f.kinds[0] === "folder") parts.push("type:folder");
  else if (f.kinds.length && !f.kinds.includes("folder")) parts.push("type:file");
  else if (!f.kinds.length && f.notKinds.includes("folder")) parts.push("type:file");
  if (f.extensions.length === 1) parts.push(`ext:${f.extensions[0]}`);
  if (f.size !== "any") parts.push(...(SIZE_TERMS[f.size] ?? []));
  return parts.join(" ") || "*";
}
