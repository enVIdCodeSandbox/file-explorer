// Extra icon collections for the icon picker. None come with the app: a collection is looked at over the network
// (a preview of a few icons) and installed on request, after which it is read from the app's cache folder and works
// with no network. A chosen icon is kept as a few lines of plain SVG that take the text color, so it keeps working
// even if the collection is removed later.

import { formatSize } from "./format";

export interface PackState {
  id: string;
  label: string;
  about: string;
  /** About how much installing downloads, in bytes. */
  download: number;
  installed: boolean;
  /** What it takes on disk now. */
  disk: number;
}

export interface PackList {
  packs: PackState[];
  disk: number;
}

/** What the shell sends for a collection: the default size and each icon's body (or body, width, height). */
export interface PackData {
  w: number;
  h: number;
  icons: Record<string, string | [string, number, number]>;
}

export interface PackIcon {
  name: string;
  svg: string;
}

/** Read what the shell sent, as a list in alphabetical order. Anything unexpected gives an empty list. */
export function parsePack(json: string): PackIcon[] {
  let data: PackData;
  try {
    data = JSON.parse(json) as PackData;
  } catch {
    return [];
  }
  if (!data || typeof data.icons !== "object" || data.icons === null) return [];
  const w = Number(data.w) || 16;
  const h = Number(data.h) || 16;
  const out: PackIcon[] = [];
  for (const [name, v] of Object.entries(data.icons)) {
    const body = Array.isArray(v) ? v[0] : v;
    if (typeof body !== "string") continue;
    const iw = Array.isArray(v) ? Number(v[1]) || w : w;
    const ih = Array.isArray(v) ? Number(v[2]) || h : h;
    out.push({ name, svg: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${iw} ${ih}" width="1em" height="1em">${body}</svg>` });
  }
  return out.sort((a, b) => a.name.localeCompare(b.name));
}

/** "hard-drive" shows as "Hard drive". */
export function packIconLabel(name: string): string {
  const words = name.replace(/[-_]+/g, " ").trim().toLowerCase();
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/** Whether an icon's name matches what was typed (every word of it). */
export function matchesQuery(name: string, query: string): boolean {
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const n = name.toLowerCase();
  return words.every((w) => n.includes(w));
}

/** A download's progress, as the shell reports it. */
export interface PackProgress {
  id: string;
  phase: "connecting" | "downloading" | "saving";
  done: number;
  total: number;
}

/** How full the bar is, 0 to 1. The total is an estimate, so the bar waits at 99% until the file really is done. */
export function packFraction(p: PackProgress): number {
  if (p.phase === "saving") return 1;
  if (p.phase === "connecting" || p.total <= 0) return 0;
  return Math.max(0, Math.min(0.99, p.done / p.total));
}

export function packProgressText(p: PackProgress): string {
  if (p.phase === "connecting") return "Connecting…";
  if (p.phase === "saving") return "Saving…";
  return `${formatSize(p.done)} of about ${formatSize(p.total)}`;
}
