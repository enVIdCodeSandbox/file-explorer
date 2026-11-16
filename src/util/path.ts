// Path helpers that work for both `C:\Users\me` and `/home/me` without
// asking the backend. Paths are treated as opaque strings otherwise.

export const HOME = "fast://home";
export const THIS_PC = "fast://thispc";
/** The Recycle Bin (Windows) or Trash (macOS, Linux). */
export const TRASH = "fast://trash";

export const isAppLocation = (loc: string) => loc.startsWith("fast://");

/** An address on a server: `sftp://user@host/folder` (also ftp, dav, davs). */
export const isRemote = (loc: string) => /^(ftp|sftp|dav|davs|mtp):\/\//i.test(loc);

// ---- Archives opened like folders ---------------------------------------
// `archive:///home/me/photos.zip!/2024/trip` is the folder "2024/trip" inside
// photos.zip. The inside part always uses "/". They are read-only.

export const ARCHIVE_SCHEME = "archive://";
export const isArchiveLoc = (loc: string) => loc.startsWith(ARCHIVE_SCHEME);

/** Names that can be opened as folders. Must stay a subset of what fast_core::browse knows. */
const ARCHIVE_NAME = /.\.(zip|7z|rar|tar|tgz|tbz2?|txz|tzst|tar\.(gz|bz2|xz|zst|lz)|gz|bz2|xz|zst|lz4|cab|iso|wim|arj|lzh|lha|cpio|rpm|deb)$/i;
export const isArchiveFile = (name: string) => ARCHIVE_NAME.test(name);

export function splitArchive(loc: string): { archive: string; inner: string } | null {
  if (!isArchiveLoc(loc)) return null;
  const rest = loc.slice(ARCHIVE_SCHEME.length);
  for (let i = rest.indexOf("!"); i >= 0; i = rest.indexOf("!", i + 1)) {
    const after = rest.slice(i + 1);
    if ((after === "" || after.startsWith("/")) && isArchiveFile(rest.slice(0, i).replace(/^.*[\\/]/, ""))) {
      return { archive: rest.slice(0, i), inner: after.replace(/^\/+|\/+$/g, "") };
    }
  }
  return null;
}

export const archiveUrl = (archive: string, inner = "") => `${ARCHIVE_SCHEME}${archive}!/${inner.replace(/^\/+|\/+$/g, "")}`;

/** What to show when the address is edited: `/home/me/photos.zip/2024/trip`. */
export function archiveDisplay(loc: string): string {
  const arc = splitArchive(loc);
  if (!arc) return loc;
  if (!arc.inner) return arc.archive;
  const sep = arc.archive.includes("\\") ? "\\" : "/";
  return arc.archive + sep + arc.inner.split("/").join(sep);
}

/** A typed path that goes through an archive file (`/home/me/a.zip/docs`) as an archive location. */
export function archiveFromTyped(text: string): string | null {
  if (isArchiveLoc(text)) return splitArchive(text) ? text : null;
  for (let i = 0; i <= text.length; i++) {
    if (i < text.length && text[i] !== "/" && text[i] !== "\\") continue;
    const prefix = text.slice(0, i);
    if (prefix && isArchiveFile(prefix.replace(/^.*[\\/]/, ""))) return archiveUrl(prefix, text.slice(i + 1).replace(/\\/g, "/"));
  }
  return null;
}

/** The archive file when `path` is inside one (for "read-only" checks and the archive's own folder). */
export const insideArchive = (path: string): string | null => splitArchive(path)?.archive ?? null;

/** Split a server address into the server part and the folder path (always starting with "/"). */
export function splitRemote(url: string): { origin: string; path: string } {
  const start = url.indexOf("://") + 3;
  const slash = url.indexOf("/", start);
  if (slash < 0) return { origin: url, path: "/" };
  const path = url.slice(slash).replace(/\/+$/, "") || "/";
  return { origin: url.slice(0, slash), path };
}

// ---- Address privacy ----------------------------------------------------
// Hosts, IP addresses and share paths are hidden wherever they are only
// displayed. The real address is still used to connect; it is shown only when
// the person turns hiding off (Settings > Appearance) or edits the address.

const privacy: { hide: boolean; names: Map<string, string> } = { hide: false, names: new Map() };

/** Set from the settings and the saved-server list; see store.ts. */
export function setAddressPrivacy(hide: boolean, servers: { name: string; host: string }[]) {
  privacy.hide = hide;
  privacy.names = new Map(servers.map((s) => [s.host.toLowerCase(), s.name]));
}

export const addressesHidden = () => privacy.hide;

// ---- Plugged-in devices -------------------------------------------------
// `mtp://3f9a…/Internal storage/DCIM`: the code after the scheme names a phone or
// camera. Show the device's own name wherever the address is only displayed.

const deviceNames = new Map<string, string>();
/** The key the app uses in addresses, from a device's address (`mtp://key/`). */
const deviceKey = (path: string | null | undefined) => (path && /^mtp:\/\//i.test(path) ? splitRemote(path).origin.slice(6).toLowerCase() : null);

export function setDeviceNames(devices: { name: string; path: string | null }[]) {
  deviceNames.clear();
  for (const d of devices) {
    const k = deviceKey(d.path);
    if (k) deviceNames.set(k, d.name);
  }
}
const HIDDEN = "•••";

/** What stands in for a host: the name the person gave the server, else dots. */
function hostAlias(host: string): string {
  return privacy.names.get(host.toLowerCase().replace(/^\[|\]$/g, "")) ?? HIDDEN;
}

/**
 * Hides server addresses in text that is only shown: URLs (`sftp://me@10.0.0.5:22/x`),
 * UNC paths (`\\nas\share`), NFS exports (`nas:/export`) and GVFS mount names.
 * `force` overrides the global choice (used for a drive with its own setting).
 */
export function maskAddresses(text: string, force?: boolean): string {
  if (!(force ?? privacy.hide) || !text) return text;
  return text
    .replace(/\b([a-z][a-z0-9+.-]*:\/\/)(?:([^@/\s]*)@)?(\[[0-9a-f:]+\]|[^:/\s?#]+)(?::\d+)?/gi, (_m, scheme: string, user: string | undefined, host: string) => `${scheme}${user ? `${user}@` : ""}${hostAlias(host)}`)
    .replace(/\\\\[^\\/\s]+(?=[\\/]|$)/g, `\\\\${HIDDEN}`)
    .replace(/(^|\s)([A-Za-z0-9][A-Za-z0-9._-]+):\/(?!\/)/g, `$1${HIDDEN}:/`)
    .replace(/(server=)[^,/\s]+/g, `$1${HIDDEN}`);
}

/** "user@host" for a server address, for titles (the host is hidden when addresses are). */
export function remoteLabel(url: string): string {
  if (/^mtp:\/\//i.test(url)) return deviceNames.get(splitRemote(url).origin.slice(6).toLowerCase()) ?? "Device";
  const origin = splitRemote(url).origin.replace(/^[a-z]+:\/\//i, "");
  if (!privacy.hide) return origin;
  const at = origin.lastIndexOf("@");
  const host = (at >= 0 ? origin.slice(at + 1) : origin).replace(/:\d+$/, "");
  return `${at >= 0 ? origin.slice(0, at + 1) : ""}${hostAlias(host)}`;
}

/** The real "user@host", for saving and connecting. */
export function rawRemoteLabel(url: string): string {
  if (/^mtp:\/\//i.test(url)) return remoteLabel(url);
  return splitRemote(url).origin.replace(/^[a-z]+:\/\//i, "");
}

export function sepOf(path: string): string {
  return path.includes("\\") || /^[A-Za-z]:/.test(path) ? "\\" : "/";
}

export function joinPath(dir: string, name: string): string {
  const arc = splitArchive(dir);
  if (arc) return archiveUrl(arc.archive, arc.inner ? `${arc.inner}/${name}` : name);
  if (isRemote(dir)) {
    const { origin, path } = splitRemote(dir);
    return origin + (path === "/" ? "" : path) + "/" + name;
  }
  const sep = sepOf(dir);
  return dir.endsWith(sep) ? dir + name : dir + sep + name;
}

export function baseName(path: string): string {
  const arc = splitArchive(path);
  if (arc) return arc.inner ? arc.inner.slice(arc.inner.lastIndexOf("/") + 1) : baseName(arc.archive);
  if (isRemote(path)) {
    const { origin, path: p } = splitRemote(path);
    return p === "/" ? remoteLabel(origin) : p.slice(p.lastIndexOf("/") + 1);
  }
  const sep = sepOf(path);
  const trimmed = path.length > 1 && path.endsWith(sep) ? path.slice(0, -1) : path;
  const i = trimmed.lastIndexOf(sep);
  if (i < 0) return path; // a drive root such as "C:\\"
  return trimmed.slice(i + 1) || path;
}

export function parentPath(path: string): string | null {
  const arc = splitArchive(path);
  if (arc) {
    if (!arc.inner) return parentPath(arc.archive);
    const cut = arc.inner.lastIndexOf("/");
    return archiveUrl(arc.archive, cut < 0 ? "" : arc.inner.slice(0, cut));
  }
  if (isRemote(path)) {
    const { origin, path: p } = splitRemote(path);
    if (p === "/") return null;
    const cut = p.lastIndexOf("/");
    return origin + (cut <= 0 ? "/" : p.slice(0, cut));
  }
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

/** How far a search reaches: this folder only, this folder and everything under it, or everywhere. */
export type SearchMode = "level" | "tree" | "global";

export function searchLocation(query: string, scope: string | null, mode?: SearchMode): string {
  const params = new URLSearchParams({ q: query });
  if (scope) params.set("scope", scope);
  if (mode && scope && mode !== "tree") params.set("mode", mode);
  return `fast://search?${params}`;
}

export function analyzeLocation(path: string): string {
  return `fast://analyze?${new URLSearchParams({ path })}`;
}

/** The Smart Clear page for one drive (its mount point), or for the whole device. */
export function clearLocation(drive: string | null): string {
  return `fast://clear?${new URLSearchParams(drive ? { drive } : {})}`;
}

export type Location =
  | { kind: "home" }
  | { kind: "thispc" }
  | { kind: "trash" }
  | { kind: "search"; query: string; scope: string | null; mode: SearchMode }
  | { kind: "analyze"; path: string }
  | { kind: "clear"; drive: string | null }
  | { kind: "folder"; path: string };

export function parseLocation(loc: string): Location {
  if (loc === HOME) return { kind: "home" };
  if (loc === THIS_PC) return { kind: "thispc" };
  if (loc === TRASH) return { kind: "trash" };
  if (loc.startsWith("fast://search?")) {
    const p = new URLSearchParams(loc.slice("fast://search?".length));
    const scope = p.get("scope");
    return { kind: "search", query: p.get("q") ?? "", scope, mode: !scope ? "global" : p.get("mode") === "level" ? "level" : "tree" };
  }
  if (loc.startsWith("fast://analyze?")) {
    const p = new URLSearchParams(loc.slice("fast://analyze?".length));
    return { kind: "analyze", path: p.get("path") ?? "" };
  }
  if (loc.startsWith("fast://clear")) {
    const p = new URLSearchParams(loc.slice("fast://clear?".length));
    return { kind: "clear", drive: p.get("drive") };
  }
  return { kind: "folder", path: loc };
}

/** What the bin is called here. */
export const trashLabel = (os: string | undefined) => (os === "windows" ? "Recycle Bin" : "Trash");

export function locationTitle(loc: string, thisPcLabel: string): string {
  const l = parseLocation(loc);
  switch (l.kind) {
    case "home":
      return "Home";
    case "thispc":
      return thisPcLabel;
    case "trash":
      return trashLabel(undefined);
    case "search":
      return `Search: ${l.query}`;
    case "analyze":
      return `Disk usage: ${baseName(l.path)}`;
    case "clear":
      return "Smart Clear";
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
