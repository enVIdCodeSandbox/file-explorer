// "Select group" (Total Commander's numpad +, − and *): which items of a folder match a pattern, a size
// range, a date range, and optionally some text inside the file.

import type { FileEntry } from "../api";

export interface GroupOpts {
  /** Wildcards (`*.jpg; img_??.png`), several separated by ; , or a space. After a `|` come the ones to leave out. */
  pattern: string;
  /** Read the pattern as a regular expression (matched against the whole name) instead. */
  regex: boolean;
  caseSensitive: boolean;
  /** Folders too (else only files). */
  includeFolders: boolean;
  /** Bytes; null = no limit. */
  minSize: number | null;
  maxSize: number | null;
  /** Milliseconds since 1970; null = no limit. */
  newerThan: number | null;
  olderThan: number | null;
  /** Only files that have this text inside (checked by the caller, with the file contents). */
  contains: string;
}

export const emptyGroup: GroupOpts = { pattern: "*", regex: false, caseSensitive: false, includeFolders: false, minSize: null, maxSize: null, newerThan: null, olderThan: null, contains: "" };

function wildRegex(w: string, cs: boolean): RegExp {
  const body = w.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".");
  return new RegExp(`^${body}$`, cs ? "" : "i");
}

/** `*.jpg;*.png|thumb*` → a test for a name. A pattern without a dot also matches names without extension, like Total Commander's `*`. */
export function nameTest(pattern: string, regex: boolean, caseSensitive: boolean): ((name: string) => boolean) | null {
  const text = pattern.trim() || "*";
  if (regex) {
    try {
      const re = new RegExp(`^(?:${text})$`, caseSensitive ? "" : "i");
      return (n) => re.test(n);
    } catch {
      return null;
    }
  }
  const [inc, exc = ""] = text.split("|");
  const list = (s: string) => s.split(/[;, ]+/).map((x) => x.trim()).filter(Boolean);
  const inclusions = (list(inc).length ? list(inc) : ["*"]).map((w) => wildRegex(w === "*.*" ? "*" : w, caseSensitive));
  const exclusions = list(exc).map((w) => wildRegex(w === "*.*" ? "*" : w, caseSensitive));
  return (n) => inclusions.some((r) => r.test(n)) && !exclusions.some((r) => r.test(n));
}

/** Everything but the text inside the files. */
export function groupMatches(entries: FileEntry[], o: GroupOpts): FileEntry[] | null {
  const test = nameTest(o.pattern, o.regex, o.caseSensitive);
  if (!test) return null;
  return entries.filter((e) => {
    if (e.isDir && !o.includeFolders) return false;
    if (!test(e.name)) return false;
    if (!e.isDir) {
      if (o.minSize !== null && e.size < o.minSize) return false;
      if (o.maxSize !== null && e.size > o.maxSize) return false;
    }
    if (o.newerThan !== null && (e.modified ?? 0) < o.newerThan) return false;
    if (o.olderThan !== null && (e.modified ?? 0) > o.olderThan) return false;
    return true;
  });
}

/** Parse "10 KB", "1.5mb", "300" into bytes; null for empty or unreadable. */
export function parseSize(text: string): number | null {
  const m = /^\s*([\d.,]+)\s*(b|kb?|mb?|gb?|tb?)?(?:i?b)?\s*$/i.exec(text);
  if (!m) return null;
  const n = Number(m[1].replace(",", "."));
  if (!Number.isFinite(n)) return null;
  const unit = (m[2] ?? "b").toLowerCase()[0];
  return Math.round(n * ({ b: 1, k: 1024, m: 1024 ** 2, g: 1024 ** 3, t: 1024 ** 4 }[unit] ?? 1));
}
