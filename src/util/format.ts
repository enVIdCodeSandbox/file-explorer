import type { Progress } from "../api";

/** How sizes are written: decimal "MB" (1000), binary "MiB" (1024), or Windows-style "MB" that is really 1024. */
export type SizeUnits = "decimal" | "binary" | "windows";
const DECIMAL = ["B", "KB", "MB", "GB", "TB", "PB"];
const BINARY = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];

/** Display preferences (Advanced settings), applied once when they change. */
const prefs = { base: 1000, labels: DECIMAL, dates: "system", clock: "auto" };

/** Bytes in one MB / MiB under the current setting, for size pickers. */
export const unitBytes = (power: 1 | 2 | 3) => prefs.base ** power;
let dateFmt: Intl.DateTimeFormat;

export function configureFormat(p: { units: SizeUnits; dates: string; clock: string }) {
  prefs.base = p.units === "decimal" ? 1000 : 1024;
  prefs.labels = p.units === "binary" ? BINARY : DECIMAL;
  prefs.dates = p.dates;
  prefs.clock = p.clock;
  dateFmt = new Intl.DateTimeFormat(undefined, {
    year: "numeric",
    month: "numeric",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
    ...(p.clock === "12" ? { hour12: true } : p.clock === "24" ? { hour12: false } : {}),
  });
}

export function formatSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "";
  let i = 0;
  let v = bytes;
  while (v >= prefs.base && i < prefs.labels.length - 1) {
    v /= prefs.base;
    i++;
  }
  const digits = i === 0 ? 0 : v >= 100 ? 0 : v >= 10 ? 1 : 2;
  return `${v.toFixed(digits)} ${prefs.labels[i]}`;
}

/**
 * Memory and similar hardware figures are made in powers of two but sold by
 * their nearest round number: 16 GiB of RAM is "16 GB". Binary mode says GiB.
 */
export function formatMemory(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "";
  let i = 0;
  let v = bytes;
  while (v >= 1024 && i < BINARY.length - 1) {
    v /= 1024;
    i++;
  }
  const text = v >= 10 ? v.toFixed(0) : v.toFixed(1).replace(/\.0$/, "");
  return `${text} ${(prefs.labels === BINARY ? BINARY : DECIMAL)[i]}`;
}

configureFormat({ units: "decimal", dates: "system", clock: "auto" });

const pad2 = (n: number) => String(n).padStart(2, "0");

export function formatDate(ms: number | null | undefined, now = Date.now()): string {
  if (ms == null || ms === 0) return "";
  const d = new Date(ms);
  if (prefs.dates === "iso") return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())} ${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
  if (prefs.dates === "relative") {
    const days = Math.floor((new Date(now).setHours(0, 0, 0, 0) - new Date(ms).setHours(0, 0, 0, 0)) / 86_400_000);
    const time = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit", ...(prefs.clock === "12" ? { hour12: true } : prefs.clock === "24" ? { hour12: false } : {}) }).format(d);
    if (days === 0) return `Today, ${time}`;
    if (days === 1) return `Yesterday, ${time}`;
    if (days > 1 && days < 7) return `${days} days ago`;
  }
  return dateFmt.format(d);
}

const numFmt = new Intl.NumberFormat();
export const formatCount = (n: number) => numFmt.format(Math.round(n));

/** "5 seconds", "About 3 minutes", "About 2 hours 10 minutes". */
export function formatEta(secs: number | null): string {
  if (secs == null || !Number.isFinite(secs)) return "Calculating…";
  if (secs < 1) return "Almost done";
  if (secs < 60) return `${Math.ceil(secs / 5) * 5} seconds left`;
  if (secs < 3600) {
    const m = Math.round(secs / 60);
    return `About ${m} minute${m === 1 ? "" : "s"} left`;
  }
  const h = Math.floor(secs / 3600);
  const m = Math.round((secs % 3600) / 60 / 5) * 5;
  return `About ${h} hour${h === 1 ? "" : "s"}${m ? ` ${m} minutes` : ""} left`;
}

/**
 * The speed readout. Which unit is shown follows what is actually limiting
 * the transfer: throughput (MB/s), per-file overhead (files/s), or both.
 */
export function formatRate(p: Pick<Progress, "display" | "bytesPerSec" | "filesPerSec">): string {
  const bytes = `${formatSize(p.bytesPerSec)}/s`;
  const files = `${p.filesPerSec >= 10 ? formatCount(p.filesPerSec) : p.filesPerSec.toFixed(1)} files/s`;
  if (p.display === "bytes") return bytes;
  if (p.display === "files") return files;
  return `${files} · ${bytes}`;
}

/** "12 sec", "3 min 20 sec", "2 h 5 min": a duration for readouts and graphs. */
export function formatDuration(secs: number): string {
  if (!Number.isFinite(secs) || secs < 0) return "";
  const s = Math.round(secs);
  if (s < 60) return `${s} sec`;
  if (s < 3600) return `${Math.floor(s / 60)} min${s % 60 ? ` ${s % 60} sec` : ""}`;
  const m = Math.round((s % 3600) / 60);
  return `${Math.floor(s / 3600)} h${m ? ` ${m} min` : ""}`;
}

/** Progress shown for a job: never lower than what was already shown. */
export function steadyFraction(previous: number, p: Pick<Progress, "phase" | "fraction">): number {
  if (p.phase === "done") return 1;
  return Math.max(previous, Math.min(0.999, p.fraction));
}

export function extensionOf(name: string): string {
  const i = name.lastIndexOf(".");
  return i > 0 && i < name.length - 1 ? name.slice(i + 1).toLowerCase() : "";
}

const KINDS: Record<string, string[]> = {
  image: ["jpg", "jpeg", "png", "gif", "webp", "bmp", "svg", "ico", "tiff", "heic", "avif", "raw"],
  video: ["mp4", "mkv", "mov", "avi", "webm", "wmv", "m4v", "flv"],
  audio: ["mp3", "flac", "wav", "ogg", "m4a", "aac", "opus", "wma"],
  archive: ["zip", "7z", "rar", "tar", "gz", "bz2", "xz", "zst", "iso", "dmg"],
  code: ["ts", "tsx", "js", "jsx", "rs", "py", "go", "c", "cpp", "h", "java", "cs", "html", "css", "scss", "json", "toml", "yaml", "yml", "xml", "sh", "ps1", "sql"],
  doc: ["pdf", "doc", "docx", "odt", "rtf", "txt", "md", "epub"],
  sheet: ["xls", "xlsx", "csv", "ods"],
  slides: ["ppt", "pptx", "odp", "key"],
  exe: ["exe", "msi", "app", "deb", "rpm", "appimage", "bat", "cmd", "dll", "so"],
};

const KIND_BY_EXT = new Map<string, string>();
for (const [kind, exts] of Object.entries(KINDS)) for (const e of exts) KIND_BY_EXT.set(e, kind);

/** Coarse file category used for icons and type labels. */
export function kindOf(name: string, isDir: boolean): string {
  if (isDir) return "folder";
  return KIND_BY_EXT.get(extensionOf(name)) ?? "file";
}

export function typeLabel(name: string, isDir: boolean): string {
  if (isDir) return "File folder";
  const ext = extensionOf(name);
  return ext ? `${ext.toUpperCase()} File` : "File";
}
