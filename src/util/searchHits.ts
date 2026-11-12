import type { Hit } from "../api";

/** Best first; ties go to the shorter name, then alphabetical (same as the engine). */
export function rankHits(a: Hit, b: Hit): number {
  return (
    b.score - a.score ||
    a.name.length - b.name.length ||
    a.name.toLowerCase().localeCompare(b.name.toLowerCase()) ||
    (a.path < b.path ? -1 : a.path > b.path ? 1 : 0)
  );
}

/** Fold a streamed batch into the results: no duplicates, best first, capped. */
export function mergeHits(current: Hit[], incoming: Hit[], limit: number): Hit[] {
  if (!incoming.length) return current;
  const seen = new Set(current.map((h) => h.path));
  const fresh = incoming.filter((h) => !seen.has(h.path));
  if (!fresh.length) return current;
  return [...current, ...fresh].sort(rankHits).slice(0, limit);
}
