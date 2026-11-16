import { describe, expect, it } from "vitest";
import { CP437, binaryRow, decode, hexRow, lineStarts, lineText, looksLikeText, needleFor, parseOffset, resolveEnc, viewKindOf } from "./util/viewerCore";

const bytes = (s: string) => new TextEncoder().encode(s);

describe("viewer encodings", () => {
  it("tells encodings apart automatically", () => {
    expect(resolveEnc(Uint8Array.of(0xff, 0xfe, 0x41, 0), "auto")).toEqual({ enc: "utf-16le", bom: 2 });
    expect(resolveEnc(Uint8Array.of(0xfe, 0xff, 0, 0x41), "auto")).toEqual({ enc: "utf-16be", bom: 2 });
    expect(resolveEnc(Uint8Array.of(0xef, 0xbb, 0xbf, 0x41), "auto")).toEqual({ enc: "utf-8", bom: 3 });
    expect(resolveEnc(bytes("héllo wörld"), "auto").enc).toBe("utf-8");
    // Cut in the middle of a character: fine for a piece of a bigger file, not for a whole small one.
    const cut = bytes("héllo wörld").subarray(0, 9);
    expect(resolveEnc(cut, "auto", true).enc).toBe("utf-8");
    expect(resolveEnc(cut, "auto", false).enc).toBe("cp1252");
    expect(resolveEnc(Uint8Array.of(0x68, 0xe9, 0x6c, 0x6c, 0x6f, 0x20, 0x77, 0xf6, 0x72, 0x6c, 0x64, 0x21, 0x21, 0x21), "auto").enc).toBe("cp1252");
    expect(resolveEnc(bytes("x"), "cp437")).toEqual({ enc: "cp437", bom: 0 });
  });
  it("decodes the DOS and ANSI code pages", () => {
    expect(CP437.length).toBe(128);
    expect(decode(Uint8Array.of(0x80, 0xb0, 0xdb, 0xe1, 0x41), "cp437")).toBe("Ç░█ßA");
    expect(decode(Uint8Array.of(0x80, 0xe9), "cp1252")).toBe("€é");
    expect(decode(Uint8Array.of(0x80, 0xe9), "latin1")).toBe("\u0080é");
    expect(decode(Uint8Array.of(0xc7, 0xe0), "cp1251")).toBe("За");
    expect(decode(bytes("日本"), "utf-8")).toBe("日本");
  });
  it("splits lines by the encoding's own line feed", () => {
    expect(lineStarts(bytes("a\nbb\n\nc"), "utf-8")).toEqual([0, 2, 5, 6]);
    expect(lineStarts(bytes("a\n"), "utf-8")).toEqual([0]);
    const le = Uint8Array.of(0x61, 0, 0x0a, 0, 0x62, 0);
    expect(lineStarts(le, "utf-16le")).toEqual([0, 4]);
    expect(lineText(le, 0, 4, "utf-16le")).toBe("a");
    const be = Uint8Array.of(0, 0x61, 0, 0x0a, 0, 0x62);
    expect(lineStarts(be, "utf-16be")).toEqual([0, 4]);
    expect(lineText(bytes("one\r\n"), 0, 5, "utf-8")).toBe("one");
  });
  it("writes a search the way the file stores it", () => {
    expect(Array.from(needleFor("hé", "utf-8")!)).toEqual([0x68, 0xc3, 0xa9]);
    expect(Array.from(needleFor("hé", "cp1252")!)).toEqual([0x68, 0xe9]);
    expect(Array.from(needleFor("é", "cp437")!)).toEqual([0x82]);
    expect(Array.from(needleFor("a", "utf-16le")!)).toEqual([0x61, 0]);
    expect(Array.from(needleFor("a", "utf-16be")!)).toEqual([0, 0x61]);
    expect(needleFor("日", "cp1252")).toBeNull();
    expect(needleFor("", "utf-8")).toBeNull();
    expect(Array.from(needleFor("abc", "shift_jis")!)).toEqual([0x61, 0x62, 0x63]);
  });
  it("lays out hex and binary rows", () => {
    const b = Uint8Array.from([0x48, 0x69, 0x00, 0xff, 0x41]);
    const row = hexRow(b, 0, 4, 0x10, "cp1252", 8);
    expect(row).toEqual({ offset: "00000010", bytes: ["48", "69", "00", "FF"], text: "Hi·ÿ" });
    expect(hexRow(b, 4, 4, 0x10, "utf-8", 8).bytes).toEqual(["41"]);
    expect(binaryRow(b, 0, 2, 0, 4)).toEqual({ offset: "0000", bits: ["01001000", "01101001"] });
  });
  it("reads offsets and recognises files", () => {
    expect(parseOffset("1234")).toBe(1234);
    expect(parseOffset("0x4D2")).toBe(1234);
    expect(parseOffset("4d2h")).toBe(1234);
    expect(parseOffset("1,234")).toBe(1234);
    expect(parseOffset("zz")).toBeNull();
    expect(parseOffset("-5")).toBeNull();
    expect(viewKindOf("a.PNG")).toBe("image");
    expect(viewKindOf("a.html")).toBe("html");
    expect(viewKindOf("a.tar.gz")).toBe("other");
    expect(viewKindOf("noext")).toBe("other");
    expect(looksLikeText(bytes("hello\n"))).toBe(true);
    expect(looksLikeText(Uint8Array.of(1, 2, 0, 3))).toBe(false);
    expect(looksLikeText(Uint8Array.of(0xff, 0xfe, 0x41, 0))).toBe(true);
  });
});
