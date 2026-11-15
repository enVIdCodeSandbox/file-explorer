// The columns the Details view can show, and how a folder's choice of them is kept.
// The same catalog is used on every kind of device. Which of them can be filled in depends on
// where the folder is: details that come from a file's own header or the file system need the
// file on this computer's disks, so phones, servers and archives show only the first few.

export type ColumnKind = "text" | "size" | "date" | "number";

export interface ColumnDef {
  id: string;
  label: string;
  group: string;
  kind: ColumnKind;
  /** "listing": comes with the folder listing, so every kind of location has it.
   *  "disk": read from the file itself, only for folders on this computer's drives. */
  source: "listing" | "disk";
  width: number;
  min: number;
  description?: string;
}

const c = (id: string, label: string, group: string, kind: ColumnKind, source: ColumnDef["source"], width: number, description?: string): ColumnDef => ({
  id, label, group, kind, source, width, min: 50, description,
});

export const COLUMNS: ColumnDef[] = [
  c("name", "Name", "Basic", "text", "listing", 420),
  c("modified", "Date modified", "Basic", "date", "listing", 170),
  c("type", "Type", "Basic", "text", "listing", 140),
  c("size", "Size", "Basic", "size", "listing", 100, "Files, and folders when folder sizes are on."),
  c("created", "Date created", "Basic", "date", "listing", 170),
  c("extension", "Extension", "Basic", "text", "listing", 90),
  c("accessed", "Date accessed", "Time", "date", "disk", 170),
  c("sizeOnDisk", "Size on disk", "Size", "size", "disk", 110, "Space the file takes up on the drive (macOS and Linux)."),
  c("items", "Items", "Size", "number", "disk", 80, "How many items are directly inside a folder."),
  c("attributes", "Attributes", "Technical", "text", "disk", 150, "Windows attributes, or the permissions on macOS and Linux."),
  c("owner", "Owner (user ID)", "Technical", "text", "disk", 110, "macOS and Linux."),
  c("mime", "Content type", "Technical", "text", "disk", 150),
  c("linkTarget", "Link target", "Technical", "text", "disk", 220, "Where a shortcut or symbolic link points."),
  c("dimensions", "Dimensions", "Pictures", "text", "disk", 120),
  c("taken", "Date taken", "Pictures", "text", "disk", 150),
  c("camera", "Camera", "Pictures", "text", "disk", 160),
  c("lens", "Lens", "Pictures", "text", "disk", 160),
  c("iso", "ISO", "Pictures", "number", "disk", 70),
  c("aperture", "Aperture", "Pictures", "text", "disk", 80),
  c("exposure", "Exposure", "Pictures", "text", "disk", 90),
  c("focal", "Focal length", "Pictures", "text", "disk", 100),
  c("software", "Software", "Pictures", "text", "disk", 150),
  c("length", "Length", "Music and video", "text", "disk", 80),
  c("title", "Title", "Music and video", "text", "disk", 200),
  c("artist", "Artist", "Music and video", "text", "disk", 160),
  c("album", "Album", "Music and video", "text", "disk", 160),
  c("genre", "Genre", "Music and video", "text", "disk", 110),
  c("year", "Year", "Music and video", "number", "disk", 70),
  c("bitrate", "Bit rate", "Music and video", "text", "disk", 100),
  c("sampleRate", "Sample rate", "Music and video", "text", "disk", 100),
  c("frameSize", "Frame size", "Music and video", "text", "disk", 110),
  c("frameRate", "Frame rate", "Music and video", "text", "disk", 100),
  c("videoCodec", "Video codec", "Music and video", "text", "disk", 110),
  c("audioCodec", "Audio codec", "Music and video", "text", "disk", 110),
];

export const COLUMN_GROUPS = ["Basic", "Time", "Size", "Technical", "Pictures", "Music and video"];

const BY_ID = new Map(COLUMNS.map((d) => [d.id, d]));
export const columnDef = (id: string): ColumnDef | undefined => BY_ID.get(id);
export const isColumn = (id: string): boolean => BY_ID.has(id);

/** Which columns a folder shows, in order (Name first), and any widths the person set. */
export interface ColumnSet {
  order: string[];
  widths: Record<string, number>;
}

export const DEFAULT_ORDER = ["name", "modified", "type", "size"];
export const DEFAULT_SET: ColumnSet = { order: DEFAULT_ORDER, widths: {} };

/** A set read from storage or built by hand, made safe: known columns once each, Name first. */
export function normalizeSet(raw: unknown): ColumnSet {
  const r = (raw && typeof raw === "object" ? raw : {}) as Partial<ColumnSet>;
  const seen = new Set<string>(["name"]);
  const order = ["name"];
  for (const id of Array.isArray(r.order) ? r.order : DEFAULT_ORDER) {
    if (typeof id === "string" && isColumn(id) && !seen.has(id)) {
      seen.add(id);
      order.push(id);
    }
  }
  const widths: Record<string, number> = {};
  for (const [id, w] of Object.entries(r.widths && typeof r.widths === "object" ? r.widths : {})) {
    const d = columnDef(id);
    if (d && typeof w === "number" && Number.isFinite(w)) widths[id] = Math.max(d.min, Math.min(1200, Math.round(w)));
  }
  return { order, widths };
}

/** Widths from before columns could be chosen were saved as { name, modified, type, size }. */
export function setFromLegacyWidths(raw: unknown): ColumnSet {
  const r = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  return normalizeSet({ order: DEFAULT_ORDER, widths: Object.fromEntries(DEFAULT_ORDER.filter((k) => typeof r[k] === "number").map((k) => [k, r[k]])) });
}

export const widthOf = (set: ColumnSet, id: string): number => set.widths[id] ?? columnDef(id)?.width ?? 120;

export const isDefaultSet = (s: ColumnSet): boolean => s.order.join() === DEFAULT_ORDER.join();

export function showColumn(set: ColumnSet, id: string): ColumnSet {
  return set.order.includes(id) || !isColumn(id) ? set : { ...set, order: [...set.order, id] };
}
export function hideColumn(set: ColumnSet, id: string): ColumnSet {
  return id === "name" ? set : { ...set, order: set.order.filter((x) => x !== id) };
}
/** Move a column one place up or down. Name stays first. */
export function moveColumn(set: ColumnSet, id: string, by: -1 | 1): ColumnSet {
  const i = set.order.indexOf(id);
  const j = i + by;
  if (id === "name" || i < 0 || j < 1 || j >= set.order.length) return set;
  const order = [...set.order];
  [order[i], order[j]] = [order[j], order[i]];
  return { ...set, order };
}

/** The columns to offer a folder, with those it cannot fill marked. Remote places, phones and archives have a listing only. */
export function canFill(def: ColumnDef, where: { remote: boolean }): boolean {
  return def.source === "listing" || !where.remote;
}
