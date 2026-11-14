// Paths as a phone shows them. A phone hides where its storage really sits
// ("/storage/emulated/0"), so the path on screen starts at the storage's own
// name, and going "up" from the top of a storage leaves it instead of walking
// into system folders nobody may read.

import { baseName, isArchiveLoc, isRemote, parentPath, type Crumb } from "./path";

export interface Root {
  /** What the storage is called on screen. */
  name: string;
  /** Where it is mounted. */
  mount: string;
}

const trimEnd = (p: string) => (p.length > 1 ? p.replace(/[\\/]+$/, "") : p);

/** The storage a path is on: the one with the longest mount point that contains it. */
export function rootOf(path: string, roots: Root[]): Root | null {
  const p = trimEnd(path);
  let best: Root | null = null;
  for (const r of roots) {
    const m = trimEnd(r.mount);
    const inside = p === m || p.startsWith(m.endsWith("/") || m.endsWith("\\") ? m : `${m}/`) || p.startsWith(`${m}\\`);
    if (inside && (!best || m.length > trimEnd(best.mount).length)) best = r;
  }
  return best;
}

/** The folder above, or null at the top of a storage, a server or the device. */
export function phoneParent(path: string, roots: Root[]): string | null {
  if (!isRemote(path) && !isArchiveLoc(path)) {
    const r = rootOf(path, roots);
    if (r && trimEnd(r.mount) === trimEnd(path)) return null;
  }
  return parentPath(path);
}

/** Breadcrumbs that start at the storage ("Internal storage > Download"), not at "/". */
export function phoneCrumbs(path: string, roots: Root[]): Crumb[] {
  const out: Crumb[] = [];
  let cur: string | null = path;
  // A path never has more parts than this; the limit only guards against a cycle.
  for (let i = 0; cur && i < 256; i++) {
    const up: string | null = phoneParent(cur, roots);
    const r = up === null && !isRemote(cur) && !isArchiveLoc(cur) ? rootOf(cur, roots) : null;
    out.unshift({ label: r ? r.name : baseName(cur), path: cur });
    cur = up;
  }
  return out;
}

/** What a folder is called at the top of the screen. */
export function phoneTitle(path: string, roots: Root[]): string {
  const c = phoneCrumbs(path, roots);
  return c.length ? c[c.length - 1].label : baseName(path);
}
