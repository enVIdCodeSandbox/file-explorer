// The columns the Details view can show, and how a folder's choice of them is kept.
// The same catalog is used on every kind of device. Which of them can be filled in depends on
// where the folder is: details that come from a file's own header or the file system need the
// file on this computer's disks, so phones, servers and archives show only the first few.

import type { FileEntry } from "../api";
import { kindOf } from "./format";

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
  /** For a column worked out from what the listing already has: its raw value for an item
   *  (a number as text, or text), the same shape the backend gives for the other columns. */
  derive?: (e: FileEntry) => string | undefined;
  /** How a raw value is shown, when that is not the raw text (a weekday number as its name). */
  display?: (raw: string) => string;
  /** Sort by another column's values instead (Length sorts by length in seconds, not by its text). */
  sortBy?: string;
}

type Extra = Pick<ColumnDef, "derive" | "display" | "sortBy">;

const c = (id: string, label: string, group: string, kind: ColumnKind, source: ColumnDef["source"], width: number, description?: string, extra: Extra = {}): ColumnDef => ({
  id, label, group, kind, source, width, min: 50, description, ...extra,
});
/** A column read from the file itself. */
const d = (id: string, label: string, group: string, kind: ColumnKind, width: number, description?: string, extra: Extra = {}) => c(id, label, group, kind, "disk", width, description, extra);
/** A column worked out from the listing, so it is there for servers, phones and archives too. */
const w = (id: string, label: string, group: string, kind: ColumnKind, width: number, derive: (e: FileEntry) => string | undefined, description?: string, display?: (raw: string) => string) =>
  c(id, label, group, kind, "listing", width, description, { derive, display });

// ---- What the derived columns say ------------------------------------------------------------

const pad = (n: number) => String(n).padStart(2, "0");
const WEEKDAYS = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const when = (ms: number | null | undefined): Date | null => (ms && ms > 0 ? new Date(ms) : null);
const dateOf = (ms: number | null | undefined, f: (d: Date) => string) => {
  const t = when(ms);
  return t ? f(t) : undefined;
};
const isoDay = (t: Date) => `${t.getFullYear()}-${pad(t.getMonth() + 1)}-${pad(t.getDate())}`;
const daysAgo = (ms: number | null | undefined) => (when(ms) ? String(Math.max(0, Math.floor((Date.now() - ms!) / 86_400_000))) : undefined);
const ageText = (raw: string) => (raw === "0" ? "Today" : raw === "1" ? "1 day ago" : `${Number(raw).toLocaleString()} days ago`);
const baseName = (e: FileEntry) => {
  const i = e.name.lastIndexOf(".");
  return e.isDir || i <= 0 ? e.name : e.name.slice(0, i);
};
const unit = (n: number, suffix: string) => `${n.toLocaleString(undefined, { maximumFractionDigits: 1 })} ${suffix}`;
const kindName = (e: FileEntry) => {
  const k = kindOf(e.name, e.isDir);
  return k === "file" ? "Other" : k[0].toUpperCase() + k.slice(1);
};
const yes = (b: boolean) => (b ? "Yes" : undefined);
const num = (raw: string) => Number(raw);

export const COLUMNS: ColumnDef[] = [
  // ---- Basic
  c("name", "Name", "Basic", "text", "listing", 420),
  c("modified", "Date modified", "Basic", "date", "listing", 170),
  c("type", "Type", "Basic", "text", "listing", 140),
  c("size", "Size", "Basic", "size", "listing", 100, "Files, and folders when folder sizes are on."),
  c("created", "Date created", "Basic", "date", "listing", 170),
  c("extension", "Extension", "Basic", "text", "listing", 90),
  w("kind", "Kind", "Basic", "text", 100, kindName, "Picture, Video, Audio, Document, Archive, Code, Folder…"),

  // ---- Name
  w("baseName", "Name without extension", "Name", "text", 260, baseName),
  w("nameLength", "Name length", "Name", "number", 90, (e) => String(e.name.length), "How many characters the name has.", (r) => `${r}`),
  w("initial", "First letter", "Name", "text", 80, (e) => {
    const ch = e.name.replace(/^[^\p{L}\p{N}]+/u, "")[0];
    return ch ? (/\d/.test(ch) ? "#" : ch.toUpperCase()) : "#";
  }, "The letter a name starts with (numbers under #)."),

  // ---- Dates
  w("modifiedDate", "Modified: date", "Dates", "text", 110, (e) => dateOf(e.modified, isoDay), "Just the day, such as 2026-11-15."),
  w("modifiedTime", "Modified: time of day", "Dates", "text", 100, (e) => dateOf(e.modified, (t) => `${pad(t.getHours())}:${pad(t.getMinutes())}`)),
  w("modifiedWeekday", "Modified: weekday", "Dates", "number", 110, (e) => dateOf(e.modified, (t) => String((t.getDay() + 6) % 7)), "Monday to Sunday, in that order.", (r) => WEEKDAYS[num(r)] ?? r),
  w("modifiedMonth", "Modified: month", "Dates", "number", 100, (e) => dateOf(e.modified, (t) => String(t.getMonth() + 1)), "January to December, in that order.", (r) => MONTHS[num(r) - 1] ?? r),
  w("modifiedYear", "Modified: year", "Dates", "number", 90, (e) => dateOf(e.modified, (t) => String(t.getFullYear()))),
  w("modifiedAge", "Modified: how long ago", "Dates", "number", 120, (e) => daysAgo(e.modified), "Whole days since it changed.", ageText),
  w("createdDate", "Created: date", "Dates", "text", 110, (e) => dateOf(e.created, isoDay)),
  w("createdYear", "Created: year", "Dates", "number", 90, (e) => dateOf(e.created, (t) => String(t.getFullYear()))),
  w("createdAge", "Created: how long ago", "Dates", "number", 120, (e) => daysAgo(e.created), "Whole days since it was created.", ageText),
  d("accessed", "Date accessed", "Dates", "date", 170),
  d("changed", "Date changed (properties)", "Dates", "date", 170, "When the file's properties (name, permissions, owner) last changed. macOS and Linux."),

  // ---- Size
  d("sizeOnDisk", "Size on disk", "Size", "size", 110, "Space the file takes up on the drive (macOS and Linux)."),
  w("sizeBytes", "Size in bytes", "Size", "number", 130, (e) => (e.isDir ? undefined : String(e.size)), "The exact number of bytes.", (r) => num(r).toLocaleString()),
  w("sizeKB", "Size in KB", "Size", "number", 100, (e) => (e.isDir ? undefined : String(e.size / 1000)), "Always in kilobytes, whatever the size units setting says.", (r) => unit(num(r), "KB")),
  w("sizeMB", "Size in MB", "Size", "number", 100, (e) => (e.isDir ? undefined : String(e.size / 1e6)), "Always in megabytes.", (r) => unit(num(r), "MB")),
  d("items", "Items", "Size", "number", 80, "How many items are directly inside a folder."),
  d("filesInside", "Files inside", "Size", "number", 90, "Folders: how many files are directly inside."),
  d("foldersInside", "Folders inside", "Size", "number", 100, "Folders: how many folders are directly inside."),
  d("hiddenInside", "Hidden items inside", "Size", "number", 130, "Folders: how many items starting with a dot are directly inside."),

  // ---- Kind and flags
  w("hidden", "Hidden", "Kind and flags", "text", 80, (e) => yes(e.isHidden)),
  w("symlink", "Link", "Kind and flags", "text", 70, (e) => yes(e.isSymlink), "Yes for symbolic links and shortcuts."),
  w("empty", "Empty file", "Kind and flags", "text", 100, (e) => (e.isDir ? undefined : yes(e.size === 0)), "Files with no content."),

  // ---- File system
  d("attributes", "Attributes", "File system", "text", 150, "Windows attributes, or the permissions on macOS and Linux."),
  d("ownerName", "Owner", "File system", "text", 120, "macOS and Linux."),
  d("groupName", "Group", "File system", "text", 120, "macOS and Linux."),
  d("owner", "Owner (user ID)", "File system", "text", 110, "macOS and Linux."),
  d("group", "Group (ID)", "File system", "number", 90, "macOS and Linux."),
  d("inode", "Inode", "File system", "number", 110, "The file's number on its drive. macOS and Linux."),
  d("links", "Hard links", "File system", "number", 90, "How many names the same file has. macOS and Linux."),
  d("mime", "Content type", "File system", "text", 150, "From the file name."),
  d("linkTarget", "Link target", "File system", "text", 220, "Where a shortcut or symbolic link points."),

  // ---- Permissions and flags
  d("permsText", "Permissions (rwx)", "Permissions", "text", 120, "Like ls -l: -rwxr-xr-x. macOS and Linux."),
  d("permsOctal", "Permissions (number)", "Permissions", "text", 120, "Like 644 or 755. macOS and Linux."),
  d("readOnly", "Read-only", "Permissions", "text", 90),
  d("executable", "Executable", "Permissions", "text", 90, "Can be run as a program (by permission, or by extension on Windows)."),
  d("winSystem", "System file", "Permissions", "text", 100, "Windows."),
  d("winArchive", "Ready for archiving", "Permissions", "text", 140, "Windows."),
  d("winCompressed", "Compressed (NTFS)", "Permissions", "text", 130, "Windows."),
  d("winEncrypted", "Encrypted (NTFS)", "Permissions", "text", 130, "Windows."),
  d("winOffline", "Offline", "Permissions", "text", 80, "Windows: the content is stored elsewhere (cloud files)."),
  d("winTemporary", "Temporary", "Permissions", "text", 90, "Windows."),
  d("winSparse", "Sparse", "Permissions", "text", 80, "Windows."),
  d("winReparse", "Reparse point", "Permissions", "text", 110, "Windows: links, junctions and cloud placeholders."),

  // ---- Content
  d("detected", "Detected type", "Content", "text", 190, "What the file is by its first bytes, whatever its name says."),
  d("textOrBinary", "Text or binary", "Content", "text", 110),
  d("encoding", "Text encoding", "Content", "text", 150, "ASCII, UTF-8, UTF-16 and so on."),
  d("lineEndings", "Line endings", "Content", "text", 130, "Unix (LF), Windows (CRLF) or Mixed."),
  d("lines", "Lines", "Content", "number", 80, "Text files up to 8 MB."),
  d("words", "Words", "Content", "number", 80, "Text files up to 8 MB."),
  d("interpreter", "Script interpreter", "Content", "text", 140, "From the #! line, such as python3 or bash."),

  // ---- Checksums
  d("sha256", "SHA-256", "Checksums", "text", 520, "Files up to 64 MB."),
  d("sha1", "SHA-1", "Checksums", "text", 340, "Files up to 64 MB."),
  d("md5", "MD5", "Checksums", "text", 280, "Files up to 64 MB."),
  d("crc32", "CRC-32", "Checksums", "text", 100, "Files up to 64 MB."),

  // ---- Documents
  d("docTitle", "Document title", "Documents", "text", 220, "Word, Excel, PowerPoint, OpenDocument and PDF."),
  d("docAuthor", "Author", "Documents", "text", 150, "Word, Excel, PowerPoint, OpenDocument and PDF."),
  d("docLastBy", "Last saved by", "Documents", "text", 150),
  d("docSubject", "Subject", "Documents", "text", 160),
  d("docKeywords", "Keywords", "Documents", "text", 180),
  d("docComments", "Document comments", "Documents", "text", 200),
  d("docCategory", "Category", "Documents", "text", 120),
  d("docCompany", "Company", "Documents", "text", 140),
  d("docApp", "Made with", "Documents", "text", 180, "The program that made the document."),
  d("docPages", "Pages", "Documents", "number", 70, "PDF, Word and OpenDocument. A PDF whose page list is packed away may show nothing."),
  d("docWords", "Document words", "Documents", "number", 120),
  d("docChars", "Document characters", "Documents", "number", 150),
  d("docSlides", "Slides", "Documents", "number", 70),
  d("docCreated", "Document created", "Documents", "date", 170, "When the document says it was made."),
  d("docSaved", "Document saved", "Documents", "date", 170, "When the document says it was last saved."),

  // ---- Programs
  d("exeType", "Program type", "Programs", "text", 170, "Windows, Linux and macOS programs and libraries."),
  d("exeArch", "Architecture", "Programs", "text", 140, "x86, x64, ARM64…"),
  d("exeSubsystem", "Windows subsystem", "Programs", "text", 140),
  d("exeCompiled", "Built on", "Programs", "date", 170, "Windows programs: the build time they record."),
  d("exeProduct", "Product name", "Programs", "text", 160, "Windows programs."),
  d("exeCompany", "Publisher", "Programs", "text", 160, "Windows programs."),
  d("exeDescription", "Program description", "Programs", "text", 220, "Windows programs."),
  d("exeVersion", "File version", "Programs", "text", 110, "Windows programs."),
  d("exeCopyright", "Program copyright", "Programs", "text", 220, "Windows programs."),

  // ---- Archives
  d("archiveItems", "Files in archive", "Archives", "number", 110, "Zip, jar, apk, epub and similar."),
  d("archiveSize", "Unpacked size", "Archives", "size", 110, "Zip, jar, apk, epub and similar."),
  d("archiveRatio", "Packed to", "Archives", "number", 90, "How much of the original size the archive takes.", { display: (r) => `${r}%` }),

  // ---- Pictures
  d("dimensions", "Dimensions", "Pictures", "text", 120),
  d("width", "Width", "Pictures", "number", 80, "Pictures and videos, in pixels.", { display: (r) => `${r} px` }),
  d("height", "Height", "Pictures", "number", 80, "Pictures and videos, in pixels.", { display: (r) => `${r} px` }),
  d("megapixels", "Megapixels", "Pictures", "number", 100, "Pictures and videos.", { display: (r) => `${r} MP` }),
  d("aspect", "Aspect ratio", "Pictures", "text", 100, "16:9, 4:3, 3:2…"),
  d("shape", "Landscape or portrait", "Pictures", "text", 140),
  d("taken", "Date taken", "Pictures", "text", 150),
  d("camera", "Camera", "Pictures", "text", 160),
  d("cameraMaker", "Camera maker", "Pictures", "text", 120),
  d("lens", "Lens", "Pictures", "text", 160),
  d("iso", "ISO", "Pictures", "number", 70),
  d("aperture", "Aperture", "Pictures", "text", 80),
  d("exposure", "Exposure", "Pictures", "text", 90),
  d("focal", "Focal length", "Pictures", "text", 100),
  d("exposureBias", "Exposure bias", "Pictures", "text", 110),
  d("exposureProgram", "Exposure program", "Pictures", "text", 140),
  d("metering", "Metering", "Pictures", "text", 140),
  d("whiteBalance", "White balance", "Pictures", "text", 110),
  d("flash", "Flash", "Pictures", "text", 180),
  d("orientation", "Orientation", "Pictures", "text", 150),
  d("colorSpace", "Color space", "Pictures", "text", 100),
  d("dpi", "Resolution (dpi)", "Pictures", "text", 150),
  d("gps", "GPS position", "Pictures", "text", 200, "Where the photo was taken."),
  d("altitude", "Altitude", "Pictures", "text", 90),
  d("photographer", "Photographer", "Pictures", "text", 150),
  d("copyright", "Copyright", "Pictures", "text", 180, "Pictures and music."),
  d("description", "Picture description", "Pictures", "text", 220),
  d("software", "Software", "Pictures", "text", 150),

  // ---- Music and video
  d("length", "Length", "Music and video", "text", 80, undefined, { sortBy: "lengthSeconds" }),
  d("lengthSeconds", "Length in seconds", "Music and video", "number", 120),
  d("title", "Title", "Music and video", "text", 200),
  d("artist", "Artist", "Music and video", "text", 160),
  d("albumArtist", "Album artist", "Music and video", "text", 160),
  d("album", "Album", "Music and video", "text", 160),
  d("track", "Track", "Music and video", "text", 80),
  d("disc", "Disc", "Music and video", "text", 80),
  d("composer", "Composer", "Music and video", "text", 150),
  d("genre", "Genre", "Music and video", "text", 110),
  d("year", "Year", "Music and video", "number", 70),
  d("bpm", "Beats per minute", "Music and video", "number", 130),
  d("bitrate", "Bit rate", "Music and video", "text", 100),
  d("sampleRate", "Sample rate", "Music and video", "text", 100),
  d("bitDepth", "Bit depth", "Music and video", "text", 90),
  d("channels", "Channels", "Music and video", "text", 100),
  d("comment", "Comment", "Music and video", "text", 200),
  d("publisher", "Publisher (music)", "Music and video", "text", 150),
  d("frameSize", "Frame size", "Music and video", "text", 110),
  d("frameRate", "Frame rate", "Music and video", "text", 100),
  d("videoCodec", "Video codec", "Music and video", "text", 110),
  d("videoBitrate", "Video bit rate", "Music and video", "text", 120),
  d("audioCodec", "Audio codec", "Music and video", "text", 110),
];

export const COLUMN_GROUPS = ["Basic", "Name", "Dates", "Size", "Kind and flags", "File system", "Permissions", "Content", "Checksums", "Documents", "Programs", "Archives", "Pictures", "Music and video"];

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

/** Put a column at a position in the list (as when its heading is dragged there). Name stays first. */
export function moveColumnTo(set: ColumnSet, id: string, to: number): ColumnSet {
  const i = set.order.indexOf(id);
  if (id === "name" || i < 0) return set;
  const order = set.order.filter((x) => x !== id);
  order.splice(Math.max(1, Math.min(order.length, to)), 0, id);
  return order.join() === set.order.join() ? set : { ...set, order };
}

/** The columns to offer a folder, with those it cannot fill marked. Remote places, phones and archives have a listing only. */
export function canFill(def: ColumnDef, where: { remote: boolean }): boolean {
  return def.source === "listing" || !where.remote;
}
