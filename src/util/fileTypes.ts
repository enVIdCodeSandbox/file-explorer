// What each well-known extension looks like. A type has a glyph (the drawing
// on the page), a color, and a short label printed on the badge at larger
// sizes. Unknown extensions fall back to their category from format.ts.

export type Glyph =
  | "image" | "video" | "audio" | "archive" | "code" | "data" | "text" | "doc"
  | "sheet" | "slides" | "exe" | "disk" | "font" | "model" | "key" | "web"
  | "db" | "pdf" | "book" | "link" | "config" | "log" | "shell" | "package";

export interface FileType {
  glyph: Glyph;
  color: string;
  label: string;
}

const t = (glyph: Glyph, color: string, label: string): FileType => ({ glyph, color, label });

const PDF = t("pdf", "#e5484d", "PDF");
const WORD = t("doc", "#3b82f6", "DOC");
const EXCEL = t("sheet", "#22a05a", "XLS");
const POWERPOINT = t("slides", "#e8642c", "PPT");
const IMAGE = t("image", "#14b8a6", "IMG");
const VIDEO = t("video", "#a855f7", "VID");
const AUDIO = t("audio", "#eab308", "AUD");
const ZIP = t("archive", "#d97706", "ZIP");

const TYPES: Record<string, FileType> = {
  // Documents
  pdf: PDF,
  doc: WORD, docx: WORD, docm: WORD, odt: t("doc", "#2e7ad1", "ODT"), rtf: t("doc", "#5b8fd9", "RTF"), pages: t("doc", "#f59e0b", "PGS"),
  txt: t("text", "#9ca3af", "TXT"), md: t("text", "#60a5fa", "MD"), markdown: t("text", "#60a5fa", "MD"), tex: t("text", "#16a34a", "TEX"),
  epub: t("book", "#84cc16", "EPUB"), mobi: t("book", "#84cc16", "MOBI"), azw3: t("book", "#84cc16", "AZW"),
  xls: EXCEL, xlsx: EXCEL, xlsm: EXCEL, ods: t("sheet", "#1f8f5b", "ODS"), csv: t("sheet", "#4ade80", "CSV"), tsv: t("sheet", "#4ade80", "TSV"), numbers: t("sheet", "#22c55e", "NUM"),
  ppt: POWERPOINT, pptx: POWERPOINT, pps: POWERPOINT, ppsx: POWERPOINT, odp: t("slides", "#f97316", "ODP"), key: t("slides", "#3b82f6", "KEY"),
  // Images
  jpg: IMAGE, jpeg: IMAGE, png: t("image", "#06b6d4", "PNG"), gif: t("image", "#ec4899", "GIF"), webp: t("image", "#22d3ee", "WEBP"),
  bmp: IMAGE, tif: IMAGE, tiff: IMAGE, heic: t("image", "#2dd4bf", "HEIC"), heif: t("image", "#2dd4bf", "HEIF"), avif: t("image", "#2dd4bf", "AVIF"),
  svg: t("image", "#f59e0b", "SVG"), ico: t("image", "#38bdf8", "ICO"), raw: t("image", "#64748b", "RAW"), cr2: t("image", "#64748b", "CR2"), nef: t("image", "#64748b", "NEF"), arw: t("image", "#64748b", "ARW"), dng: t("image", "#64748b", "DNG"),
  psd: t("image", "#31a8ff", "PSD"), ai: t("image", "#ff9a00", "AI"), xcf: t("image", "#8b7355", "XCF"), fig: t("image", "#a259ff", "FIG"), sketch: t("image", "#f7b500", "SKT"),
  // Video
  mp4: VIDEO, mkv: t("video", "#8b5cf6", "MKV"), mov: t("video", "#c084fc", "MOV"), avi: VIDEO, webm: t("video", "#9333ea", "WEBM"),
  wmv: VIDEO, flv: VIDEO, m4v: VIDEO, mpg: VIDEO, mpeg: VIDEO, "3gp": VIDEO, "3g2": VIDEO,
  m2ts: t("video", "#7c3aed", "M2TS"), m2t: VIDEO, m2v: VIDEO, ogv: VIDEO, ogm: VIDEO, vob: t("video", "#7c3aed", "VOB"), asf: VIDEO, divx: VIDEO,
  f4v: VIDEO, rm: VIDEO, rmvb: VIDEO, mxf: VIDEO, tod: VIDEO,
  // Audio
  mp3: t("audio", "#f59e0b", "MP3"), flac: t("audio", "#fbbf24", "FLAC"), wav: t("audio", "#facc15", "WAV"), ogg: AUDIO, m4a: AUDIO, aac: AUDIO,
  opus: AUDIO, wma: AUDIO, aiff: AUDIO, mid: t("audio", "#fde047", "MID"), midi: t("audio", "#fde047", "MID"),
  // Archives and installers
  zip: ZIP, "7z": t("archive", "#f59e0b", "7Z"), rar: t("archive", "#be185d", "RAR"), tar: ZIP, gz: ZIP, tgz: ZIP, bz2: ZIP, xz: ZIP, zst: ZIP, lz4: ZIP, cab: ZIP, wim: ZIP, arj: ZIP, lzh: ZIP, lha: ZIP, tbz2: ZIP, tbz: ZIP, txz: ZIP, cpio: ZIP,
  iso: t("disk", "#94a3b8", "ISO"), img: t("disk", "#94a3b8", "IMG"), dmg: t("disk", "#94a3b8", "DMG"), vhd: t("disk", "#94a3b8", "VHD"), vhdx: t("disk", "#94a3b8", "VHDX"), vmdk: t("disk", "#94a3b8", "VMDK"),
  exe: t("exe", "#64748b", "EXE"), msi: t("package", "#0ea5e9", "MSI"), app: t("exe", "#64748b", "APP"), apk: t("package", "#3ddc84", "APK"),
  deb: t("package", "#d70a53", "DEB"), rpm: t("package", "#ee0000", "RPM"), appimage: t("package", "#64748b", "APP"), pkg: t("package", "#64748b", "PKG"),
  dll: t("config", "#64748b", "DLL"), so: t("config", "#64748b", "SO"), dylib: t("config", "#64748b", "LIB"), sys: t("config", "#64748b", "SYS"),
  // Code
  js: t("code", "#f7df1e", "JS"), mjs: t("code", "#f7df1e", "MJS"), cjs: t("code", "#f7df1e", "CJS"), jsx: t("code", "#61dafb", "JSX"),
  ts: t("code", "#3178c6", "TS"), tsx: t("code", "#3b9ae1", "TSX"), rs: t("code", "#ce422b", "RS"), py: t("code", "#3776ab", "PY"), pyc: t("code", "#6b7280", "PYC"),
  go: t("code", "#00add8", "GO"), c: t("code", "#5c6bc0", "C"), h: t("code", "#7e57c2", "H"), cpp: t("code", "#00599c", "C++"), cc: t("code", "#00599c", "C++"), hpp: t("code", "#7e57c2", "H++"),
  java: t("code", "#e76f00", "JAVA"), class: t("code", "#e76f00", "CLS"), jar: t("package", "#e76f00", "JAR"), kt: t("code", "#a97bff", "KT"), swift: t("code", "#f05138", "SWFT"),
  cs: t("code", "#68217a", "C#"), vb: t("code", "#5c2d91", "VB"), php: t("code", "#777bb3", "PHP"), rb: t("code", "#cc342d", "RB"), lua: t("code", "#000080", "LUA"),
  dart: t("code", "#0175c2", "DART"), scala: t("code", "#dc322f", "SCLA"), r: t("code", "#276dc3", "R"), jl: t("code", "#9558b2", "JL"), zig: t("code", "#f7a41d", "ZIG"),
  hs: t("code", "#5e5086", "HS"), ex: t("code", "#6e4a7e", "EX"), erl: t("code", "#a90533", "ERL"), clj: t("code", "#5881d8", "CLJ"), sql: t("db", "#e38c00", "SQL"),
  html: t("web", "#e34f26", "HTML"), htm: t("web", "#e34f26", "HTM"), css: t("web", "#1572b6", "CSS"), scss: t("web", "#cc6699", "SCSS"), sass: t("web", "#cc6699", "SASS"), less: t("web", "#1d365d", "LESS"),
  vue: t("web", "#42b883", "VUE"), svelte: t("web", "#ff3e00", "SVLT"), wasm: t("code", "#654ff0", "WASM"),
  sh: t("shell", "#4ade80", "SH"), bash: t("shell", "#4ade80", "BASH"), zsh: t("shell", "#4ade80", "ZSH"), fish: t("shell", "#4ade80", "FISH"),
  bat: t("shell", "#94a3b8", "BAT"), cmd: t("shell", "#94a3b8", "CMD"), ps1: t("shell", "#2d79c7", "PS1"),
  // Data and configuration
  json: t("data", "#f59e0b", "JSON"), jsonc: t("data", "#f59e0b", "JSON"), json5: t("data", "#f59e0b", "JSON"), yaml: t("data", "#ef4444", "YAML"), yml: t("data", "#ef4444", "YML"),
  toml: t("data", "#9c4221", "TOML"), xml: t("data", "#ff7043", "XML"), ini: t("config", "#9ca3af", "INI"), cfg: t("config", "#9ca3af", "CFG"), conf: t("config", "#9ca3af", "CONF"),
  env: t("config", "#eab308", "ENV"), lock: t("config", "#9ca3af", "LOCK"), plist: t("config", "#9ca3af", "PLST"), reg: t("config", "#3b82f6", "REG"),
  db: t("db", "#06b6d4", "DB"), sqlite: t("db", "#0f80cc", "SQLT"), sqlite3: t("db", "#0f80cc", "SQLT"), mdb: t("db", "#a4373a", "MDB"), accdb: t("db", "#a4373a", "ACC"),
  log: t("log", "#9ca3af", "LOG"), bak: t("log", "#9ca3af", "BAK"), tmp: t("log", "#9ca3af", "TMP"),
  // Fonts, 3D, security, shortcuts
  ttf: t("font", "#f472b6", "TTF"), otf: t("font", "#f472b6", "OTF"), woff: t("font", "#f472b6", "WOFF"), woff2: t("font", "#f472b6", "WOFF"),
  obj: t("model", "#a3e635", "OBJ"), fbx: t("model", "#a3e635", "FBX"), stl: t("model", "#a3e635", "STL"), blend: t("model", "#ea7600", "BLND"), glb: t("model", "#a3e635", "GLB"), gltf: t("model", "#a3e635", "GLTF"),
  pem: t("key", "#eab308", "PEM"), crt: t("key", "#eab308", "CRT"), cer: t("key", "#eab308", "CER"), pfx: t("key", "#eab308", "PFX"), gpg: t("key", "#eab308", "GPG"), kdbx: t("key", "#22c55e", "KDBX"), pub: t("key", "#eab308", "PUB"),
  lnk: t("link", "#60a5fa", "LNK"), url: t("link", "#60a5fa", "URL"), torrent: t("link", "#22c55e", "TRNT"),
  ipynb: t("code", "#f37626", "NB"), dockerfile: t("config", "#2496ed", "DOCK"),
};

/**
 * Names that more than one kind of file uses. What the start of the file says (the backend's `sniff`) picks one of
 * these; with no answer yet, or "default", the name's usual look from `TYPES` is used.
 */
export const SNIFFED_LOOKS: Record<string, { category: string; type: FileType }> = {
  "ts:video": { category: "video", type: t("video", "#a855f7", "VID") },
  "mts:video": { category: "video", type: t("video", "#a855f7", "VID") },
  "mod:video": { category: "video", type: t("video", "#a855f7", "VID") },
  "ts:qt": { category: "file", type: t("data", "#41cd52", "QT") },
  "mod:music": { category: "audio", type: t("audio", "#eab308", "MOD") },
  "key:private": { category: "file", type: t("key", "#eab308", "KEY") },
  "pub:publisher": { category: "doc", type: t("doc", "#1f8f5b", "PUB") },
  "db:thumbs": { category: "file", type: t("log", "#9ca3af", "THMB") },
};

/** Files with a well-known name and no extension. */
const BY_NAME: Record<string, FileType> = {
  dockerfile: t("config", "#2496ed", "DOCK"),
  makefile: t("shell", "#6d8086", "MAKE"),
  license: t("text", "#9ca3af", "LIC"),
  readme: t("text", "#60a5fa", "README"),
};

const CATEGORY: Record<string, FileType> = {
  image: IMAGE, video: VIDEO, audio: AUDIO, archive: ZIP, code: t("code", "#61afef", "CODE"),
  doc: t("doc", "#e06c75", "DOC"), sheet: EXCEL, slides: POWERPOINT, exe: t("exe", "#64748b", "EXE"),
};

/** The look of a file, or undefined when it should use the plain page icon. */
export function fileTypeOf(name: string, category: string, look?: string): FileType | undefined {
  const sniffed = look ? SNIFFED_LOOKS[look] : undefined;
  if (sniffed) return sniffed.type;
  const lower = name.toLowerCase();
  const dot = lower.lastIndexOf(".");
  if (dot > 0 && dot < lower.length - 1) {
    const hit = TYPES[lower.slice(dot + 1)];
    if (hit) return hit;
  } else if (BY_NAME[lower]) return BY_NAME[lower];
  return CATEGORY[category];
}

/** Number of extensions with a dedicated look (for docs and tests). */
export const KNOWN_EXTENSIONS = Object.keys(TYPES).length;
