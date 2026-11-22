// Which program a file opens with, remembered per kind of file so showing it next to every file costs one question
// per extension, not one per file. The system is the authority: this only remembers its answer for two minutes
// (and forgets it at once when the default is changed from here).

import { api, type Opener } from "../api";
import { adv } from "../settings/advanced";
import type { Settings } from "../api";
import { baseName, isArchiveLoc, isRemote } from "./path";

const TTL = 120_000;
const KEEP = 512;
const memo = new Map<string, { at: number; opener: Opener; icon: boolean }>();
const inflight = new Map<string, Promise<Opener>>();

/** Files of one kind share an answer. Without an extension the file itself decides (it may be marked as runnable). */
export function openerKey(path: string): string {
  const name = baseName(path).toLowerCase();
  const i = name.lastIndexOf(".");
  // ".ts" is TypeScript and also video: what is inside decides, so each file is asked about on its own.
  return i > 0 && name.slice(i) !== ".ts" ? name.slice(i) : `p:${path}`;
}

/** Whether the app should show this at all: the setting is on and the file is one on this computer. */
export const showOpener = (settings: Settings | null | undefined) => adv<boolean>(settings, "files.showOpensWith");
export const openerApplies = (path: string) => !isRemote(path) && !isArchiveLoc(path);

export function peekOpener(path: string, wantIcon = false): Opener | null {
  const hit = memo.get(openerKey(path));
  if (!hit || Date.now() - hit.at > TTL || (wantIcon && !hit.icon)) return null;
  return hit.opener;
}

export function loadOpener(path: string, wantIcon = false): Promise<Opener> {
  const have = peekOpener(path, wantIcon);
  if (have) return Promise.resolve(have);
  const key = openerKey(path) + (wantIcon ? "|i" : "");
  const running = inflight.get(key);
  if (running) return running;
  const p = api
    .defaultApp(path, wantIcon)
    .then((opener) => {
      if (memo.size >= KEEP) memo.clear();
      memo.set(openerKey(path), { at: Date.now(), opener, icon: wantIcon });
      return opener;
    })
    .finally(() => inflight.delete(key));
  inflight.set(key, p);
  return p;
}

const listeners = new Set<() => void>();
/** Called whenever the answers are forgotten, so what shows one can ask again. Returns the way to stop. */
export function onOpenersForgotten(fn: () => void): () => void {
  listeners.add(fn);
  return () => void listeners.delete(fn);
}

/** Forget every answer (the default for some kind of file was just changed). */
export function forgetOpeners() {
  memo.clear();
  for (const fn of [...listeners]) fn();
}

/** The words for a menu hint: only when a program really is set. */
export const openerHint = (o: Opener | null): string | undefined => (o?.kind === "app" && o.name ? o.name : o?.kind === "runs" ? "runs itself" : undefined);
