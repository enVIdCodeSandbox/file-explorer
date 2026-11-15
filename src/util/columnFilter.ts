// Filtering a folder by what one column says, like the column filter in File Explorer: each
// column offers the values it has (or, for dates and sizes, ranges such as "Today" or "Large"),
// and the folder keeps the items whose value is ticked.

import type { FileEntry } from "../api";
import type { ColumnDef } from "./columns";
import { cellValue } from "./cellValue";

export const BLANK = "(Blank)";
const DAY = 86_400_000;

const DATE_ORDER = ["Today", "Yesterday", "Earlier this week", "Earlier this month", "Earlier this year", "A long time ago"];
const SIZE_ORDER = ["Empty", "Tiny (up to 16 KB)", "Small (up to 1 MB)", "Medium (up to 128 MB)", "Large (up to 1 GB)", "Huge (over 1 GB)"];

export function dateBucket(ms: number, now: number): string {
  const today = new Date(now).setHours(0, 0, 0, 0);
  if (ms >= today) return DATE_ORDER[0];
  if (ms >= today - DAY) return DATE_ORDER[1];
  if (ms >= today - 6 * DAY) return DATE_ORDER[2];
  if (ms >= today - 30 * DAY) return DATE_ORDER[3];
  if (ms >= today - 365 * DAY) return DATE_ORDER[4];
  return DATE_ORDER[5];
}

export function sizeBucket(bytes: number): string {
  if (bytes <= 0) return SIZE_ORDER[0];
  if (bytes <= 16 * 1024) return SIZE_ORDER[1];
  if (bytes <= 1024 * 1024) return SIZE_ORDER[2];
  if (bytes <= 128 * 1024 * 1024) return SIZE_ORDER[3];
  if (bytes <= 1024 * 1024 * 1024) return SIZE_ORDER[4];
  return SIZE_ORDER[5];
}

/** The value an item is filtered by in a column: its text, or the range its date or size falls in. */
export function filterValue(def: ColumnDef, e: FileEntry, extra: Record<string, string> | undefined, now = Date.now()): string {
  if (def.kind === "date" || def.kind === "size") {
    let n: number | undefined;
    if (def.id === "modified") n = e.modified ?? undefined;
    else if (def.id === "created") n = e.created ?? undefined;
    else if (def.id === "size") n = e.isDir ? undefined : e.size;
    else {
      const raw = extra?.[def.id];
      n = raw !== undefined && /^\d+$/.test(raw) ? Number(raw) : undefined;
    }
    if (n === undefined || (def.kind === "date" && n <= 0)) return BLANK;
    return def.kind === "date" ? dateBucket(n, now) : sizeBucket(n);
  }
  return cellValue(def, e, extra) || BLANK;
}

/** Keep the items whose value in every filtered column is one of the ticked values. */
export function applyColumnFilters(
  entries: FileEntry[],
  cols: Record<string, string[]> | undefined,
  defOf: (id: string) => ColumnDef | undefined,
  extra: Record<string, Record<string, string>>,
  now = Date.now(),
): FileEntry[] {
  const active = Object.entries(cols ?? {}).filter(([, v]) => v.length > 0);
  if (!active.length) return entries;
  const checks = active.flatMap(([id, values]) => {
    const def = defOf(id);
    return def ? [{ def, values: new Set(values) }] : [];
  });
  return entries.filter((e) => checks.every((c) => c.values.has(filterValue(c.def, e, extra[e.name], now))));
}

/** The values a column has in this folder, with how many items have each, in a sensible order. */
export function columnFacets(
  def: ColumnDef,
  entries: FileEntry[],
  extra: Record<string, Record<string, string>>,
  now = Date.now(),
): [string, number][] {
  const counts = new Map<string, number>();
  for (const e of entries) {
    const v = filterValue(def, e, extra[e.name], now);
    counts.set(v, (counts.get(v) ?? 0) + 1);
  }
  const order = def.kind === "date" ? DATE_ORDER : def.kind === "size" ? SIZE_ORDER : null;
  const out = [...counts.entries()];
  if (order) return out.sort((a, b) => rank(order, a[0]) - rank(order, b[0]));
  const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });
  // Blank last; the rest alphabetical (numbers in number order).
  return out.sort((a, b) => (a[0] === BLANK ? 1 : b[0] === BLANK ? -1 : collator.compare(a[0], b[0])));
}
const rank = (order: string[], v: string) => (v === BLANK ? order.length : order.indexOf(v));
