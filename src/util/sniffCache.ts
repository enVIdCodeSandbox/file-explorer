// What the start of a file said it is, for names that more than one kind of file uses (".ts" is TypeScript, video or a
// Qt translation). Kept by full path. Leaf module: format.ts reads it, sniff.ts fills it.

import { SNIFFED_LOOKS } from "./fileTypes";

/** The extensions whose files are looked into (the backend's `sniff::AMBIGUOUS`). */
export const AMBIGUOUS_EXTS = ["ts", "mts", "mod", "key", "pub", "db"];

interface Found {
  /** Size and modification time it was found for. */
  sig: string;
  /** A name from SNIFFED_LOOKS, or "default" for "it is what its name says". */
  look: string;
}

const found = new Map<string, Found>();
const listeners = new Set<() => void>();

export const sniffSig = (modified: number | null, size: number) => `${modified ?? 0}|${size}`;

export function setSniffed(path: string, sig: string, look: string) {
  const had = found.get(path);
  if (had && had.sig === sig && had.look === look) return;
  found.set(path, { sig, look });
  for (const fn of [...listeners]) fn();
}

export function subscribeSniffed(fn: () => void): () => void {
  listeners.add(fn);
  return () => void listeners.delete(fn);
}

/** The recorded entry for a file, if any. */
export function sniffedFor(path: string): Found | undefined {
  return found.get(path);
}

/** The look a file's start gave it, or undefined when it has none (not looked at yet, or it is what its name says). */
export function sniffedLook(path: string): string | undefined {
  const look = found.get(path)?.look;
  return look && SNIFFED_LOOKS[look] ? look : undefined;
}

/** For tests. */
export function forgetSniffed() {
  found.clear();
}
