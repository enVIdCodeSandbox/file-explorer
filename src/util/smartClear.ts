// Smart Clear's choosing logic: which found items match the filters, which
// are ticked to begin with, and which ones "clear 40%" means. Kept apart from
// the page so it can be tested by itself.

import type { ClearItem, ClearKind } from "../api";

export interface KindInfo {
  kind: ClearKind;
  label: string;
  /** One sentence on what these are and what clearing them costs. */
  about: string;
  /** Recreated when needed, or already thrown away: ticked from the start. */
  safe: boolean;
}

/** Every kind, from the plainly disposable to what only the person can judge. The slider clears in this order. */
export const KINDS: KindInfo[] = [
  { kind: "temp", label: "Temporary files", about: "Left behind by apps and installers. Nothing uses them any more.", safe: true },
  { kind: "cache", label: "App caches", about: "Kept by apps to start faster. They are rebuilt as needed; an app may be slower the next time it opens.", safe: true },
  { kind: "thumbnails", label: "Picture previews", about: "Small copies of pictures used in lists. They are made again when a folder is opened.", safe: true },
  { kind: "trash", label: "Trash", about: "Things you already deleted. Clearing them here removes them for good.", safe: true },
  { kind: "partial", label: "Unfinished downloads", about: "Downloads that stopped before the end and cannot be opened.", safe: true },
  { kind: "logs", label: "Old logs and crash reports", about: "Records apps wrote about themselves. Only useful when chasing a problem.", safe: false },
  { kind: "installers", label: "Installers in Downloads", about: "Setup files and disk images, usually not needed once the app is installed.", safe: false },
  { kind: "buildLeftovers", label: "Project build folders", about: "node_modules, build output and similar in projects not touched for a long time. The project's own tools recreate them.", safe: false },
  { kind: "duplicates", label: "Exact copies", about: "Files with exactly the same contents as another file. One copy of each is always kept.", safe: false },
  { kind: "oldDownloads", label: "Old downloads", about: "In Downloads and not opened for a long time. Look before clearing: they may be the only copy.", safe: false },
  { kind: "largeOld", label: "Large files not used for a long time", about: "Big, and not opened in months. Look before clearing: they may be the only copy.", safe: false },
];

const ORDER = new Map(KINDS.map((k, i) => [k.kind, i]));
export const kindInfo = (kind: ClearKind): KindInfo => KINDS[ORDER.get(kind) ?? 0];

export interface ClearFilter {
  /** Only these kinds; empty means every kind. */
  kinds: ClearKind[];
  /** Only items not used for at least this many days (0: any age). */
  olderDays: number;
  /** Only items at least this big, in bytes (0: any size). */
  minBytes: number;
  /** Only items whose name or folder contains this, ignoring case. */
  text: string;
}

export const NO_FILTER: ClearFilter = { kinds: [], olderDays: 0, minBytes: 0, text: "" };

export const isFiltered = (f: ClearFilter): boolean => f.kinds.length > 0 || f.olderDays > 0 || f.minBytes > 0 || f.text.trim() !== "";

/** The items the filters let through. `now` is in seconds. */
export function matching(items: ClearItem[], f: ClearFilter, now: number): ClearItem[] {
  const text = f.text.trim().toLowerCase();
  const before = f.olderDays > 0 ? now - f.olderDays * 86_400 : Infinity;
  return items.filter(
    (i) =>
      (f.kinds.length === 0 || f.kinds.includes(i.kind)) &&
      i.size >= f.minBytes &&
      // An item of unknown age is never "old enough".
      (f.olderDays === 0 || (i.used > 0 && i.used <= before)) &&
      (text === "" || i.path.toLowerCase().includes(text)),
  );
}

export const totalBytes = (items: ClearItem[]): number => items.reduce((sum, i) => sum + i.size, 0);

/** What is ticked when a scan finishes: everything that is safe to clear without looking. */
export function initialSelection(items: ClearItem[]): Set<string> {
  return new Set(items.filter((i) => kindInfo(i.kind).safe).map((i) => i.path));
}

/**
 * "Clear this much": the safest kinds first, and within a kind the items left
 * alone the longest, until at least `percent` of the bytes on offer are
 * covered. 0 picks nothing and 100 picks everything.
 */
export function pickPercent(items: ClearItem[], percent: number): Set<string> {
  const picked = new Set<string>();
  const goal = (totalBytes(items) * Math.max(0, Math.min(100, percent))) / 100;
  if (goal <= 0) return picked;
  const ordered = [...items].sort((a, b) => (ORDER.get(a.kind) ?? 0) - (ORDER.get(b.kind) ?? 0) || a.used - b.used || b.size - a.size);
  let sum = 0;
  for (const item of ordered) {
    if (sum >= goal) break;
    picked.add(item.path);
    sum += item.size;
  }
  return picked;
}

/** The share of the offered bytes that is ticked, as a whole percentage. */
export function percentOf(items: ClearItem[], selected: Set<string>): number {
  const all = totalBytes(items);
  if (all === 0) return 0;
  return Math.round((totalBytes(items.filter((i) => selected.has(i.path))) / all) * 100);
}

/** Every path the ordinary delete has to be given for these items. */
export function pathsToDelete(items: ClearItem[]): string[] {
  return items.flatMap((i) => [i.path, ...i.also]);
}

/** Days since something was last used, for "not used for 7 months". */
export function idleText(used: number, now: number): string {
  if (used <= 0) return "";
  const days = Math.floor((now - used) / 86_400);
  if (days < 1) return "used today";
  if (days < 60) return `${days} day${days === 1 ? "" : "s"} ago`;
  if (days < 730) return `${Math.floor(days / 30)} months ago`;
  return `${Math.floor(days / 365)} years ago`;
}
