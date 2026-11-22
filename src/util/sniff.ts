// Asking the backend what the start of a file says it is, for names that more than one kind of file uses.
// Requests from many rows are gathered into one call; "unknown" answers (a busy or silent drive) are asked again later.

import { useEffect, useSyncExternalStore } from "react";
import { api } from "../api";
import { AMBIGUOUS_EXTS, setSniffed, sniffSig, sniffedFor, sniffedLook, subscribeSniffed } from "./sniffCache";
import { extensionOf } from "./format";

const BATCH = 32;
const GATHER_MS = 40;
const RETRY_MS = 5000;
const TRIES = 3;

const waiting = new Map<string, string>();
const asked = new Map<string, number>();
let timer: ReturnType<typeof setTimeout> | undefined;
let running = false;

export const isAmbiguousName = (name: string) => AMBIGUOUS_EXTS.includes(extensionOf(name));

async function flush() {
  timer = undefined;
  if (running) return;
  running = true;
  try {
    while (waiting.size) {
      const batch = [...waiting.entries()].slice(0, BATCH);
      for (const [path] of batch) waiting.delete(path);
      let answers: string[];
      try {
        answers = await api.sniffTypes(batch.map(([path]) => path));
      } catch {
        answers = batch.map(() => "unknown");
      }
      batch.forEach(([path, sig], i) => {
        const look = answers[i] ?? "unknown";
        if (look !== "unknown") {
          asked.delete(path);
          setSniffed(path, sig, look);
          return;
        }
        const n = (asked.get(path) ?? 0) + 1;
        asked.set(path, n);
        if (n < TRIES) setTimeout(() => request(path, sig), RETRY_MS);
      });
    }
  } finally {
    running = false;
  }
}

/** Ask what the file at `path` really is (once, unless it changed). */
export function request(path: string, sig: string) {
  const had = sniffedFor(path);
  if (had && had.sig === sig) return;
  if (waiting.get(path) === sig) return;
  waiting.set(path, sig);
  timer ??= setTimeout(() => void flush(), GATHER_MS);
}

/**
 * The look a file's start gave it ("ts:video"…), or undefined while that is not known or the file is what its name
 * says. `ask` says whether to find out (a file in a folder that can be looked into).
 */
export function useSniffedLook(path: string, modified: number | null, size: number, ask: boolean): string | undefined {
  const look = useSyncExternalStore(subscribeSniffed, () => sniffedLook(path));
  const sig = sniffSig(modified, size);
  useEffect(() => {
    if (ask) request(path, sig);
  }, [ask, path, sig]);
  return look;
}
