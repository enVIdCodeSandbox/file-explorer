// Icons the person has swapped for another app icon or a picture of their own. Kept as JSON in the advanced setting
// "icons.custom": one entry for each kind of icon (the Home icon, the Drive icon, the folder icon…) or for one drive.
// Everything that draws that icon reads the same entry, so a change shows everywhere at once.

export type IconChoice = { app: string } | { img: string };
export type IconMap = Record<string, IconChoice>;

export const ICONS_KEY = "icons.custom";
/** A picture is shrunk to fit this many pixels each way before it is kept. */
export const MAX_PICTURE = 96;
const MAX_PICTURE_CHARS = 120_000;

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
    const { app, img } = v as { app?: unknown; img?: unknown };
    if (typeof app === "string" && /^[A-Za-z]{1,24}$/.test(app)) out[slot] = { app };
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
