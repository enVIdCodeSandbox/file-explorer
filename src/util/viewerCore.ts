// The logic behind the file viewer (Lister-style): decoding bytes in the encoding chosen,
// splitting them into lines, laying out hex and binary rows, and writing a search text the way
// the file stores it. No DOM, so it can be tested.

export type Enc = "auto" | "utf-8" | "cp1252" | "cp437" | "latin1" | "utf-16le" | "utf-16be" | "cp1251" | "cp866" | "koi8-r" | "shift_jis" | "gbk" | "euc-kr" | "big5";

export const ENCODINGS: [Enc, string][] = [
  ["auto", "Automatic"],
  ["utf-8", "Unicode (UTF-8)"],
  ["cp1252", "ANSI (Windows 1252)"],
  ["cp437", "ASCII (DOS, code page 437)"],
  ["latin1", "Latin-1 (ISO 8859-1)"],
  ["utf-16le", "Unicode (UTF-16 little-endian)"],
  ["utf-16be", "Unicode (UTF-16 big-endian)"],
  ["cp1251", "Cyrillic (Windows 1251)"],
  ["cp866", "Cyrillic (DOS 866)"],
  ["koi8-r", "Cyrillic (KOI8-R)"],
  ["shift_jis", "Japanese (Shift JIS)"],
  ["gbk", "Chinese (GBK)"],
  ["big5", "Chinese (Big5)"],
  ["euc-kr", "Korean (EUC-KR)"],
];

const CP437_HIGH = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■ ";
export const CP437 = [...CP437_HIGH];

const LABEL: Partial<Record<Enc, string>> = { cp1252: "windows-1252", cp1251: "windows-1251", cp866: "ibm866", "koi8-r": "koi8-r", shift_jis: "shift_jis", gbk: "gbk", big5: "big5", "euc-kr": "euc-kr", "utf-8": "utf-8", "utf-16le": "utf-16le", "utf-16be": "utf-16be" };

/** Which encoding "Automatic" means for these bytes, and how many bytes of byte-order mark to skip. */
export function resolveEnc(sample: Uint8Array, enc: Enc, truncated = false): { enc: Exclude<Enc, "auto">; bom: number } {
  const bomOf = (e: Exclude<Enc, "auto">) => (e === "utf-8" && sample[0] === 0xef && sample[1] === 0xbb && sample[2] === 0xbf ? 3 : (e === "utf-16le" && sample[0] === 0xff && sample[1] === 0xfe) || (e === "utf-16be" && sample[0] === 0xfe && sample[1] === 0xff) ? 2 : 0);
  if (enc !== "auto") return { enc, bom: bomOf(enc) };
  if (sample[0] === 0xff && sample[1] === 0xfe) return { enc: "utf-16le", bom: 2 };
  if (sample[0] === 0xfe && sample[1] === 0xff) return { enc: "utf-16be", bom: 2 };
  if (sample[0] === 0xef && sample[1] === 0xbb && sample[2] === 0xbf) return { enc: "utf-8", bom: 3 };
  // A sample cut out of a bigger file may end in the middle of a character, so allow for that.
  for (let cut = 0; cut <= (truncated ? 3 : 0); cut++) {
    try {
      new TextDecoder("utf-8", { fatal: true }).decode(sample.subarray(0, sample.length - cut));
      return { enc: "utf-8", bom: 0 };
    } catch {
      /* try a shorter sample */
    }
  }
  return { enc: "cp1252", bom: 0 };
}

const decoders = new Map<string, TextDecoder>();
function decoder(label: string): TextDecoder {
  let d = decoders.get(label);
  if (!d) {
    d = new TextDecoder(label, { fatal: false });
    decoders.set(label, d);
  }
  return d;
}

export function decode(bytes: Uint8Array, enc: Exclude<Enc, "auto">): string {
  if (enc === "cp437") {
    let s = "";
    for (let i = 0; i < bytes.length; i++) {
      const b = bytes[i];
      s += b < 0x80 ? String.fromCharCode(b) : CP437[b - 0x80];
    }
    return s;
  }
  if (enc === "latin1") {
    let s = "";
    for (let i = 0; i < bytes.length; i++) s += String.fromCharCode(bytes[i]);
    return s;
  }
  try {
    return decoder(LABEL[enc] ?? "utf-8").decode(bytes);
  } catch {
    return decoder("utf-8").decode(bytes);
  }
}

export const unitOf = (enc: Exclude<Enc, "auto">) => (enc === "utf-16le" || enc === "utf-16be" ? 2 : 1);

/** Where each line starts, as byte offsets into `bytes` (always beginning with 0). A line ends after a line feed. */
export function lineStarts(bytes: Uint8Array, enc: Exclude<Enc, "auto">): number[] {
  const starts = [0];
  if (unitOf(enc) === 2) {
    const le = enc === "utf-16le";
    for (let i = 0; i + 1 < bytes.length; i += 2) {
      if (le ? bytes[i] === 0x0a && bytes[i + 1] === 0 : bytes[i] === 0 && bytes[i + 1] === 0x0a) starts.push(i + 2);
    }
  } else {
    for (let i = 0; i < bytes.length; i++) if (bytes[i] === 0x0a) starts.push(i + 1);
  }
  if (starts.length > 1 && starts[starts.length - 1] >= bytes.length) starts.pop();
  return starts;
}

/** The text of one line without its line ending. */
export function lineText(bytes: Uint8Array, from: number, to: number, enc: Exclude<Enc, "auto">): string {
  let t = decode(bytes.subarray(from, to), enc);
  if (t.endsWith("\n")) t = t.slice(0, -1);
  if (t.endsWith("\r")) t = t.slice(0, -1);
  return t;
}

/** What to look for, written as the file stores it in this encoding; null when the encoding cannot write it. */
export function needleFor(text: string, enc: Exclude<Enc, "auto">): Uint8Array | null {
  if (!text) return null;
  if (enc === "utf-8") return new TextEncoder().encode(text);
  if (unitOf(enc) === 2) {
    const out: number[] = [];
    for (let i = 0; i < text.length; i++) {
      const c = text.charCodeAt(i);
      out.push(...(enc === "utf-16le" ? [c & 255, c >> 8] : [c >> 8, c & 255]));
    }
    return Uint8Array.from(out);
  }
  const out: number[] = [];
  const reverse = reverseTable(enc);
  for (const ch of text) {
    const code = ch.codePointAt(0)!;
    if (code < 0x80) out.push(code);
    else {
      const b = reverse?.get(ch);
      if (b === undefined) return null;
      out.push(b);
    }
  }
  return Uint8Array.from(out);
}

const reverseCache = new Map<string, Map<string, number>>();
function reverseTable(enc: Exclude<Enc, "auto">): Map<string, number> | null {
  if (enc === "shift_jis" || enc === "gbk" || enc === "big5" || enc === "euc-kr") return null;
  let m = reverseCache.get(enc);
  if (!m) {
    m = new Map();
    for (let b = 0x80; b < 0x100; b++) m.set(decode(Uint8Array.of(b), enc), b);
    reverseCache.set(enc, m);
  }
  return m;
}

const hex2 = (n: number) => n.toString(16).toUpperCase().padStart(2, "0");

export interface HexRow {
  offset: string;
  bytes: string[];
  text: string;
}

/** One row of the hex view: the offset, the bytes, and what they look like as text. */
export function hexRow(bytes: Uint8Array, from: number, perRow: number, base: number, enc: Exclude<Enc, "auto">, width: number): HexRow {
  const slice = bytes.subarray(from, from + perRow);
  const single = unitOf(enc) === 1 && enc !== "utf-8" ? enc : "latin1";
  let text = "";
  for (const b of slice) {
    if (b < 0x20 || b === 0x7f) text += "·";
    else if (b < 0x80) text += String.fromCharCode(b);
    else text += single === "latin1" ? "·" : decode(Uint8Array.of(b), single as Exclude<Enc, "auto">).replace(/[\u0080-\u009f]/, "·");
  }
  return { offset: (base + from).toString(16).toUpperCase().padStart(width, "0"), bytes: Array.from(slice, hex2), text };
}

export function binaryRow(bytes: Uint8Array, from: number, perRow: number, base: number, width: number): { offset: string; bits: string[] } {
  const slice = bytes.subarray(from, from + perRow);
  return { offset: (base + from).toString(16).toUpperCase().padStart(width, "0"), bits: Array.from(slice, (b) => b.toString(2).padStart(8, "0")) };
}

export const offsetWidth = (size: number) => Math.max(8, size.toString(16).length);

/** "1234", "0x4D2" or "4D2h" to a number. */
export function parseOffset(text: string): number | null {
  const t = text.trim().replace(/[\s,_]/g, "");
  if (!t) return null;
  const hex = /^0x([0-9a-f]+)$/i.exec(t) ?? /^([0-9a-f]+)h$/i.exec(t);
  const n = hex ? parseInt(hex[1], 16) : /^\d+$/.test(t) ? parseInt(t, 10) : NaN;
  return Number.isFinite(n) && n >= 0 ? n : null;
}

// ---- What kind of thing a file is ------------------------------------------------------------

export type ViewKind = "image" | "video" | "audio" | "pdf" | "html" | "text" | "other";

const KINDS: Record<string, string[]> = {
  image: ["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "ico", "avif"],
  video: ["mp4", "webm", "mov", "m4v", "ogv"],
  audio: ["mp3", "wav", "ogg", "oga", "m4a", "flac", "aac", "opus"],
  pdf: ["pdf"],
  html: ["html", "htm", "xhtml"],
};
const BY_EXT = new Map<string, ViewKind>();
for (const [k, exts] of Object.entries(KINDS)) for (const e of exts) BY_EXT.set(e, k as ViewKind);

export function viewKindOf(name: string): ViewKind {
  const ext = name.includes(".") ? name.slice(name.lastIndexOf(".") + 1).toLowerCase() : "";
  return BY_EXT.get(ext) ?? "other";
}

const MIME: Record<string, string> = {
  png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg", gif: "image/gif", webp: "image/webp", bmp: "image/bmp", svg: "image/svg+xml", ico: "image/x-icon", avif: "image/avif",
  mp4: "video/mp4", webm: "video/webm", mov: "video/quicktime", m4v: "video/mp4", ogv: "video/ogg",
  mp3: "audio/mpeg", wav: "audio/wav", ogg: "audio/ogg", oga: "audio/ogg", m4a: "audio/mp4", flac: "audio/flac", aac: "audio/aac", opus: "audio/ogg",
  pdf: "application/pdf",
};
export const mimeOf = (name: string) => MIME[name.slice(name.lastIndexOf(".") + 1).toLowerCase()] ?? "application/octet-stream";

/** Largest file (bytes) shown as a picture, a player or a page. Larger ones open as text or hex. */
export const MEDIA_LIMIT: Record<string, number> = { image: 64 << 20, video: 256 << 20, audio: 128 << 20, pdf: 64 << 20, html: 2 << 20 };

/** Does the start of a file look like text (no zero bytes, apart from UTF-16 text)? */
export function looksLikeText(sample: Uint8Array): boolean {
  if (sample.length === 0) return true;
  if ((sample[0] === 0xff && sample[1] === 0xfe) || (sample[0] === 0xfe && sample[1] === 0xff)) return true;
  let odd = 0;
  for (let i = 0; i < sample.length; i++) {
    const b = sample[i];
    if (b === 0) return false;
    if (b < 9 || (b > 13 && b < 32)) odd++;
  }
  return odd * 20 < sample.length;
}
