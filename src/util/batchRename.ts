// Planning for "rename many at once". Pure functions, so the preview in the
// dialog and the renames that follow are guaranteed to agree.

export interface RenameRules {
  /** Text to look for in each name (without the extension). Empty means skip. */
  find: string;
  replace: string;
  matchCase: boolean;
  /**
   * What the name becomes. `{name}` is the name after find and replace,
   * `{n}` a counter, `{date}` the modified date (YYYY-MM-DD).
   */
  template: string;
  start: number;
  /** Counter width: 3 gives 001, 002, … */
  digits: number;
  /** "keep", "lower", "upper" or "title". */
  letters: "keep" | "lower" | "upper" | "title";
}

export const DEFAULT_RULES: RenameRules = { find: "", replace: "", matchCase: false, template: "{name}", start: 1, digits: 1, letters: "keep" };

export interface RenameInput {
  name: string;
  isDir: boolean;
  /** Seconds since 1970, for `{date}`. */
  modified: number;
}

export type Problem = "empty" | "invalid" | "duplicate" | "exists" | "tooLong";

export interface RenamePlan {
  from: string;
  to: string;
  changed: boolean;
  problem: Problem | null;
}

/** Characters no common file system accepts in a name. */
const FORBIDDEN = /[\\/:*?"<>|\u0000-\u001f]/;
const RESERVED = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(\..*)?$/i;

export const PROBLEM_TEXT: Record<Problem, string> = {
  empty: "The name would be empty",
  invalid: "Contains a character that is not allowed",
  duplicate: "Two items would get the same name",
  exists: "Another item here already has this name",
  tooLong: "The name is too long",
};

/** Split "report.final.pdf" into ["report.final", ".pdf"]; folders and dotfiles have no extension. */
export function splitName(name: string, isDir: boolean): [string, string] {
  if (isDir) return [name, ""];
  const dot = name.lastIndexOf(".");
  return dot > 0 ? [name.slice(0, dot), name.slice(dot)] : [name, ""];
}

function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function titleCase(s: string): string {
  return s.toLowerCase().replace(/(^|[\s_\-.([])(\p{L})/gu, (_, sep: string, ch: string) => sep + ch.toUpperCase());
}

function isoDate(seconds: number): string {
  const d = new Date(seconds * 1000);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** The new names for `items`, in the order given, and what is wrong with any of them. */
export function planRenames(items: RenameInput[], rules: RenameRules, siblings: Iterable<string> = []): RenamePlan[] {
  const own = new Set(items.map((i) => i.name));
  const others = new Set<string>();
  for (const s of siblings) if (!own.has(s)) others.add(s);
  const fold = (s: string) => s.toLowerCase();
  const othersFolded = new Set([...others].map(fold));

  const plans: RenamePlan[] = items.map((item, index) => {
    const [stem, ext] = splitName(item.name, item.isDir);
    let base = stem;
    if (rules.find) base = base.replace(new RegExp(escapeRegExp(rules.find), rules.matchCase ? "g" : "gi"), () => rules.replace);
    const counter = String(Math.max(0, Math.trunc(rules.start)) + index).padStart(Math.max(1, Math.min(12, Math.trunc(rules.digits))), "0");
    let made = (rules.template || "{name}").replace(/\{(name|n|date)\}/g, (_, token: string) => (token === "name" ? base : token === "n" ? counter : isoDate(item.modified)));
    if (rules.letters === "lower") made = made.toLowerCase();
    else if (rules.letters === "upper") made = made.toUpperCase();
    else if (rules.letters === "title") made = titleCase(made);
    // Windows silently drops trailing dots and spaces, so do not create them.
    made = made.replace(/^\s+|[\s.]+$/g, "");
    const to = made + ext;
    let problem: Problem | null = null;
    if (!made) problem = "empty";
    else if (FORBIDDEN.test(made) || RESERVED.test(to)) problem = "invalid";
    else if (new TextEncoder().encode(to).length > 255) problem = "tooLong";
    else if (othersFolded.has(fold(to))) problem = "exists";
    return { from: item.name, to, changed: to !== item.name, problem };
  });

  // Two items ending up with the same name (case-insensitively, as on
  // Windows and macOS) would silently lose one of them.
  const seen = new Map<string, number>();
  for (const p of plans) seen.set(fold(p.to), (seen.get(fold(p.to)) ?? 0) + 1);
  for (const p of plans) if (!p.problem && (seen.get(fold(p.to)) ?? 0) > 1) p.problem = "duplicate";
  return plans;
}

export interface RenameStep {
  from: string;
  to: string;
}

/**
 * The renames to carry out, in rounds. Normally one round. When an item takes
 * the name another selected item still has (1→2 while 2→3, or a swap) the
 * affected items go through a temporary name first, so nothing is overwritten.
 */
export function orderRenames(plans: RenamePlan[], tempTag = String(Date.now())): RenameStep[][] {
  const moving = plans.filter((p) => p.changed && !p.problem);
  const fold = (s: string) => s.toLowerCase();
  const leaving = new Set(moving.map((p) => fold(p.from)));
  const tangled = moving.filter((p) => leaving.has(fold(p.to)));
  if (!tangled.length) return moving.length ? [moving.map((p) => ({ from: p.from, to: p.to }))] : [];
  const temp = (_p: RenamePlan, i: number) => `.fast-rename-${tempTag}-${i}`;
  const isTangled = new Set(tangled);
  const first: RenameStep[] = [];
  const second: RenameStep[] = [];
  const direct: RenameStep[] = [];
  moving.forEach((p, i) => {
    if (isTangled.has(p)) {
      first.push({ from: p.from, to: temp(p, i) });
      second.push({ from: temp(p, i), to: p.to });
    } else direct.push({ from: p.from, to: p.to });
  });
  // Items whose new name nobody is leaving go first: they free the names the
  // tangled ones are waiting for. Then the tangled ones, through temp names.
  return [direct, first, second].filter((round) => round.length);
}
