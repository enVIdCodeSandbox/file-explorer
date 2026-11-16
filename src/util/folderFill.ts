// Whether a folder holds anything, asked in batches for the folders on screen and remembered for a little while.

import { api } from "../api";

export type Fill = "empty" | "onlyFolders" | "files";

const known = new Map<string, { fill: Fill; at: number }>();
const waiting = new Map<string, ((f: Fill) => void)[]>();
let timer: ReturnType<typeof setTimeout> | undefined;
let active = 0;
const TTL_MS = 30_000;

export const cachedFill = (path: string): Fill | undefined => {
  const hit = known.get(path);
  return hit && Date.now() - hit.at < TTL_MS ? hit.fill : undefined;
};

function flush() {
  timer = undefined;
  if (active >= 3) {
    timer = setTimeout(flush, 60);
    return;
  }
  const batch = [...waiting.keys()].slice(0, 64);
  if (!batch.length) return;
  const callbacks = batch.map((p) => waiting.get(p)!);
  batch.forEach((p) => waiting.delete(p));
  active++;
  api
    .folderFill(batch)
    .then((fills) => batch.forEach((p, i) => {
      const f = fills[i] ?? "files";
      known.set(p, { fill: f, at: Date.now() });
      callbacks[i].forEach((cb) => cb(f));
    }))
    .catch(() => batch.forEach((_, i) => callbacks[i].forEach((cb) => cb("files"))))
    .finally(() => {
      active--;
      if (waiting.size && !timer) timer = setTimeout(flush, 10);
    });
}

/** Ask for a folder's contents; the answer comes through `done` (not at all if cancelled). Returns the way to cancel. */
export function askFill(path: string, done: (f: Fill) => void): () => void {
  const list = waiting.get(path) ?? [];
  list.push(done);
  waiting.set(path, list);
  timer ??= setTimeout(flush, 40);
  return () => {
    const l = waiting.get(path);
    if (!l) return;
    const rest = l.filter((cb) => cb !== done);
    if (rest.length) waiting.set(path, rest);
    else waiting.delete(path);
  };
}

/** How the icon shows it: with papers when it has something in it, under the chosen rule. */
export function looksFull(fill: Fill | undefined, mode: string): boolean {
  if (mode === "off" || fill === undefined) return false;
  return mode === "anything" ? fill !== "empty" : fill === "files";
}
