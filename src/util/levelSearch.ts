// Searching one folder level: the folder's own items, matched by name. No index is needed,
// so it is instant, and nothing deeper is looked at.

import type { FileEntry, Hit } from "../api";
import { joinPath } from "./path";

/** Items of `entries` whose name holds every word of `query` (any case), as search hits. */
export function levelHits(folder: string, entries: FileEntry[], query: string): Hit[] {
  const words = query.toLowerCase().split(/\s+/).filter((w) => w && !/^[a-z]+:/.test(w));
  if (!words.length) return [];
  const out: Hit[] = [];
  for (const e of entries) {
    const name = e.name.toLowerCase();
    if (!words.every((w) => name.includes(w))) continue;
    // Exact name > starts with > anywhere, the same order the search engine uses.
    const score = name === words.join(" ") ? 3 : name.startsWith(words[0]) ? 2 : 1;
    out.push({ score, path: joinPath(folder, e.name), name: e.name, isDir: e.isDir, size: e.size, modified: e.modified ?? 0 });
  }
  return out;
}
