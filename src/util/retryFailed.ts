// "Retry the failed files": which items of a finished transfer failed, and into which folder each was going.

import type { TransferError } from "../api";
import { baseName, joinPath, parentPath } from "./path";

export interface RetryGroup {
  destDir: string;
  sources: string[];
}

const same = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();
const parts = (p: string) => p.split(/[\\/]/).filter(Boolean);

/** Where an item that failed was going: what the transfer said, or worked out from what was asked for. */
function destinationOf(e: TransferError, spec?: { sources: string[]; destDir: string }): string | null {
  if (e.dest) return e.dest;
  if (!spec) return null;
  const at = parts(e.path);
  for (const s of spec.sources) {
    const top = parts(s);
    if (top.length > at.length || !top.every((c, i) => same(c, at[i]))) continue;
    return at.slice(top.length).reduce(joinPath, joinPath(spec.destDir, baseName(s)));
  }
  return null;
}

/** The failed items, grouped by the folder they were going to, so each group is one ordinary transfer. */
export function failedGroups(errors: TransferError[], spec?: { sources: string[]; destDir: string }): RetryGroup[] {
  const groups = new Map<string, RetryGroup>();
  for (const e of errors) {
    const dest = destinationOf(e, spec);
    const destDir = dest ? parentPath(dest) : null;
    if (!destDir) continue;
    const g = groups.get(destDir.toLowerCase()) ?? { destDir, sources: [] };
    if (!g.sources.some((s) => same(s, e.path))) g.sources.push(e.path);
    groups.set(destDir.toLowerCase(), g);
  }
  return [...groups.values()];
}

/** The folder an item is in, for showing next to its name. */
export const folderOf = (path: string): string => parentPath(path) ?? "";
