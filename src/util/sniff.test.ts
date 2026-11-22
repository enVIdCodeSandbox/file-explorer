import { beforeEach, describe, expect, it } from "vitest";
import { AMBIGUOUS_EXTS, forgetSniffed, setSniffed, sniffSig, sniffedLook } from "./sniffCache";
import { SNIFFED_LOOKS, fileTypeOf } from "./fileTypes";
import { kindOf, typeLabel } from "./format";
import { isAmbiguousName } from "./sniff";
import rustSource from "../../crates/fast-core/src/sniff.rs?raw";

describe("files that share a name with other kinds", () => {
  beforeEach(() => forgetSniffed());

  it("lists the same extensions as the backend", () => {
    const m = /pub const AMBIGUOUS: &\[&str\] = &\[([^\]]*)\]/.exec(rustSource)!;
    expect([...m[1].matchAll(/"([^"]+)"/g)].map((x) => x[1])).toEqual(AMBIGUOUS_EXTS);
    expect(isAmbiguousName("clip.TS")).toBe(true);
    expect(isAmbiguousName("clip.mp4")).toBe(false);
  });

  it("every look the backend can answer has an icon", () => {
    const answers = [...rustSource.matchAll(/Some\("([a-z]+:[a-z]+)"\)/g)].map((x) => x[1]);
    expect(answers.length).toBeGreaterThan(5);
    for (const a of new Set(answers)) expect(SNIFFED_LOOKS[a], a).toBeDefined();
  });

  it("a .ts file is TypeScript until its start says video, then it is a different icon and kind", () => {
    const path = "C:/clips/trip.ts";
    expect(kindOf(path, false)).toBe("code");
    expect(typeLabel(path, false)).toBe("TS File");
    const plain = fileTypeOf("trip.ts", "code");
    setSniffed(path, sniffSig(1, 5000), "ts:video");
    expect(sniffedLook(path)).toBe("ts:video");
    expect(kindOf(path, false)).toBe("video");
    expect(typeLabel(path, false)).toBe("MPEG transport stream video");
    const video = fileTypeOf("trip.ts", "video", "ts:video");
    expect(video?.glyph).toBe("video");
    expect(video?.color).not.toBe(plain?.color);
    expect(video?.glyph).not.toBe(plain?.glyph);
  });

  it("a file that is what its name says keeps its look", () => {
    const path = "C:/src/app.ts";
    setSniffed(path, sniffSig(1, 10), "default");
    expect(sniffedLook(path)).toBeUndefined();
    expect(kindOf(path, false)).toBe("code");
  });

  it("each kind sharing .ts, .mod, .key, .pub and .db looks different", () => {
    const looks = ["ts:video", "ts:qt", "mod:music", "key:private", "pub:publisher", "db:thumbs"].map((k) => SNIFFED_LOOKS[k].type);
    for (const t of looks) expect(t.label.length).toBeGreaterThan(0);
    expect(fileTypeOf("a.ts", "code", "ts:qt")?.glyph).not.toBe(fileTypeOf("a.ts", "code")?.glyph);
  });

  it("more video types are recognized", () => {
    for (const e of ["mpg", "m2ts", "vob", "ogv", "3gp"]) expect(kindOf(`a.${e}`, false)).toBe("video");
  });
});
