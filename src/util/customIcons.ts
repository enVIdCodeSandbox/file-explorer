// Icons the person has swapped for another app icon or a picture of their own. Kept as JSON in the advanced setting
// "icons.custom": one entry for each kind of icon (the Home icon, the Drive icon, the folder icon…) or for one drive.
// Everything that draws that icon reads the same entry, so a change shows everywhere at once.

export type IconChoice = { app: string } | { img: string } | { svg: string };
export type IconMap = Record<string, IconChoice>;

export const ICONS_KEY = "icons.custom";
/** A picture is shrunk to fit this many pixels each way before it is kept. */
export const MAX_PICTURE = 96;
const MAX_PICTURE_CHARS = 120_000;
const MAX_SVG_CHARS = 24_000;

const SVG_TAGS = new Set(["svg", "path", "g", "circle", "ellipse", "rect", "line", "polyline", "polygon"]);
const SVG_ATTRS = new Set([
  "viewbox", "fill", "stroke", "stroke-width", "stroke-linecap", "stroke-linejoin", "stroke-miterlimit", "stroke-dasharray", "stroke-dashoffset", "fill-rule", "clip-rule", "d", "cx", "cy", "r", "rx", "ry", "x", "y", "x1", "x2", "y1", "y2", "width", "height",
  "points", "transform", "opacity", "fill-opacity", "stroke-opacity", "xmlns", "version", "focusable", "aria-hidden",
]);

/**
 * SVG text that is safe to draw inline: only drawing shapes with plain presentation attributes (no scripts, links,
 * styles, images or text), and nothing outside the tags. Returns it tidied, or null if anything else is in it.
 */
export function safeSvg(text: unknown): string | null {
  if (typeof text !== "string" || !text.startsWith("<svg") || text.length > MAX_SVG_CHARS) return null;
  const tag = /<(\/?)([A-Za-z][A-Za-z0-9]*)((?:\s+[A-Za-z:-]+="[^"<>]*")*)\s*(\/?)>/y;
  let at = 0;
  let depth = 0;
  let out = "";
  while (at < text.length) {
    tag.lastIndex = at;
    const m = tag.exec(text);
    if (!m) return null;
    const [whole, close, name, attrs, selfClose] = m;
    if (!SVG_TAGS.has(name.toLowerCase())) return null;
    for (const a of attrs.matchAll(/([A-Za-z:-]+)="([^"]*)"/g)) {
      const value = a[2].toLowerCase();
      if (!SVG_ATTRS.has(a[1].toLowerCase()) || value.includes("javascript:") || value.includes("url(") || value.includes("&")) return null;
    }
    if (close) depth--;
    else if (!selfClose) depth++;
    if (depth < 0 || (depth === 0 && !close && !selfClose && name.toLowerCase() !== "svg")) return null;
    out += whole;
    at += whole.length;
    if (depth === 0 && at < text.length) return null; // nothing may follow the closing </svg>
  }
  return depth === 0 && out.endsWith("</svg>") ? out : null;
}

let lastRaw: unknown;
let lastMap: IconMap = {};

/** The saved overrides; the same object comes back while the setting is unchanged, so it is cheap to ask often. */
export function iconMap(raw: unknown): IconMap {
  if (raw === lastRaw) return lastMap;
  lastRaw = raw;
  lastMap = parseIcons(raw);
  return lastMap;
}

export function parseIcons(raw: unknown): IconMap {
  let data: unknown = raw;
  if (typeof raw === "string") {
    try {
      data = JSON.parse(raw);
    } catch {
      return {};
    }
  }
  const out: IconMap = {};
  if (!data || typeof data !== "object") return out;
  for (const [slot, v] of Object.entries(data as Record<string, unknown>)) {
    if (!v || typeof v !== "object") continue;
    const { app, img, svg } = v as { app?: unknown; img?: unknown; svg?: unknown };
    if (typeof app === "string" && /^[A-Za-z]{1,24}$/.test(app)) out[slot] = { app };
    else if (typeof svg === "string" && safeSvg(svg)) out[slot] = { svg };
    else if (typeof img === "string" && /^data:image\/(png|jpeg|webp|gif|svg\+xml);base64,/.test(img) && img.length <= MAX_PICTURE_CHARS) out[slot] = { img };
  }
  return out;
}

/** The setting's new value with `slot` set to `choice`, or back to the default when `choice` is null. */
export function withIcon(raw: unknown, slot: string, choice: IconChoice | null): string {
  const map = { ...parseIcons(raw) };
  if (choice) map[slot] = choice;
  else delete map[slot];
  return Object.keys(map).length ? JSON.stringify(map) : "";
}

/** The slot a drive's own icon is kept under. */
export const driveSlot = (mount: string) => `drive:${mount.toLowerCase()}`;

/** The choice for the first of `slots` that has one. */
export function pick(map: IconMap, ...slots: (string | undefined)[]): IconChoice | undefined {
  for (const s of slots) if (s && map[s]) return map[s];
  return undefined;
}

/** "folderOpen" becomes "Folder open". */
export function prettyName(name: string): string {
  const words = name.replace(/([a-z])([A-Z])/g, "$1 $2").toLowerCase();
  return words.charAt(0).toUpperCase() + words.slice(1);
}
