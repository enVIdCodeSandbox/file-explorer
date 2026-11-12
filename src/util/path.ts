// Path helpers that work for both `C:\Users\me` and `/home/me` without
// asking the backend. Paths are treated as opaque strings otherwise.

export const HOME = "fast://home";
export const THIS_PC = "fast://thispc";

export const isAppLocation = (loc: string) => loc.startsWith("fast://");

export function sepOf(path: string): string {
  return path.includes("\\") || /^[A-Za-z]:/.test(path) ? "\\" : "/";
}

export function joinPath(dir: string, name: string): string {
  const sep = sepOf(dir);
  return dir.endsWith(sep) ? dir + name : dir + sep + name;
}

export function baseName(path: string): string {
  const sep = sepOf(path);
  const trimmed = path.length > 1 && path.endsWith(sep) ? path.slice(0, -1) : path;
  const i = trimmed.lastIndexOf(sep);
  if (i < 0) return path; // a drive root such as "C:\\"
  return trimmed.slice(i + 1) || path;
}

export function parentPath(path: string): string | null {
  const sep = sepOf(path);
  const trimmed = path.length > 1 && path.endsWith(sep) ? path.slice(0, -1) : path;
  const i = trimmed.lastIndexOf(sep);
  if (i < 0) return null;
  if (i === 0) return trimmed.length > 1 ? sep : null; // "/home" -> "/"
  const parent = trimmed.slice(0, i);
  if (/^[A-Za-z]:$/.test(parent)) return parent + sep; // "C:\Users" -> "C:\"
  if (/^[A-Za-z]:$/.test(trimmed)) return null; // "C:\" has no parent
  return parent;
}

export interface Crumb {
  label: string;
  path: string;
}

/** Split a path into clickable breadcrumb segments. */
export function crumbs(path: string): Crumb[] {
  const out: Crumb[] = [];
  let cur: string | null = path;
  while (cur) {
    out.unshift({ label: baseName(cur), path: cur });
    cur = parentPath(cur);
  }
  return out;
}

export function searchLocation(query: string, scope: string | null): string {
  const params = new URLSearchParams({ q: query });
  if (scope) params.set("scope", scope);
  return `fast://search?${params}`;
}

export function analyzeLocation(path: string): string {
  return `fast://analyze?${new URLSearchParams({ path })}`;
}

export type Location =
  | { kind: "home" }
  | { kind: "thispc" }
  | { kind: "search"; query: string; scope: string | null }
  | { kind: "analyze"; path: string }
  | { kind: "folder"; path: string };

export function parseLocation(loc: string): Location {
  if (loc === HOME) return { kind: "home" };
  if (loc === THIS_PC) return { kind: "thispc" };
  if (loc.startsWith("fast://search?")) {
    const p = new URLSearchParams(loc.slice("fast://search?".length));
    return { kind: "search", query: p.get("q") ?? "", scope: p.get("scope") };
  }
  if (loc.startsWith("fast://analyze?")) {
    const p = new URLSearchParams(loc.slice("fast://analyze?".length));
    return { kind: "analyze", path: p.get("path") ?? "" };
  }
  return { kind: "folder", path: loc };
}

export function locationTitle(loc: string, thisPcLabel: string): string {
  const l = parseLocation(loc);
  switch (l.kind) {
    case "home":
      return "Home";
    case "thispc":
      return thisPcLabel;
    case "search":
      return `Search: ${l.query}`;
    case "analyze":
      return `Disk usage: ${baseName(l.path)}`;
    default:
      return baseName(l.path);
  }
}

/** True when `path` is `dir` itself or lies anywhere beneath it. */
export function isInside(path: string, dir: string): boolean {
  if (path === dir) return true;
  if (!path.startsWith(dir)) return false;
  const sep = sepOf(dir);
  return dir.endsWith(sep) || path[dir.length] === sep;
}
