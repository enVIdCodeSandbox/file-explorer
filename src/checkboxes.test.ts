import { describe, expect, it } from "vitest";
import type { FileEntry } from "./api";
import { cellValue } from "./util/cellValue";
import { flipRange, toggled } from "./util/checkSelect";
import { columnDef } from "./util/columns";
import { sortEntries } from "./util/filter";

const names = ["a", "b", "c", "d", "e", "f"];

describe("checkbox selection", () => {
  it("a click ticks, another unticks, and other ticks stay", () => {
    const one = toggled(new Set(), "b");
    expect([...one]).toEqual(["b"]);
    const two = toggled(one, "d");
    expect([...two].sort()).toEqual(["b", "d"]);
    expect([...toggled(two, "b")]).toEqual(["d"]);
  });

  it("shift turns over what is between: ticked becomes unticked and the rest ticked", () => {
    // b and d were ticked; the anchor is c (just ticked), then shift+click on e.
    const base = new Set(["b", "c", "d"]);
    const next = flipRange(names, base, 2, 4);
    // c keeps its tick; d (ticked) is unticked; e (unticked) is ticked.
    expect([...next].sort()).toEqual(["b", "c", "e"]);
  });

  it("shift works upwards too and leaves items outside the range alone", () => {
    const next = flipRange(names, new Set(["a", "e", "f"]), 4, 1);
    // between b..d turned over (none were ticked -> all ticked); e anchor stays; a, f untouched.
    expect([...next].sort()).toEqual(["a", "b", "c", "d", "e", "f"]);
    const back = flipRange(names, next, 4, 1);
    expect([...back].sort()).toEqual(["a", "e", "f"].sort().concat([]).sort());
  });
});

describe("extra columns", () => {
  const e = (name: string, extra: Partial<FileEntry> = {}): FileEntry => ({ name, isDir: false, isSymlink: false, isHidden: false, size: 0, modified: null, created: null, ...extra }) as FileEntry;

  it("sorts by a value from the file, blanks last when ascending", () => {
    const values: Record<string, string> = { a: "10", b: "9", c: "100" };
    const get = (n: string) => values[n];
    const asc = sortEntries([e("a"), e("b"), e("c"), e("d")], "iso", true, false, { kind: "number", get }).map((x) => x.name);
    expect(asc).toEqual(["b", "a", "c", "d"]);
    const desc = sortEntries([e("a"), e("b"), e("c"), e("d")], "iso", false, false, { kind: "number", get }).map((x) => x.name);
    expect(desc).toEqual(["d", "c", "a", "b"]);
  });

  it("sorts text values naturally", () => {
    const values: Record<string, string> = { a: "Track 10", b: "Track 2" };
    const out = sortEntries([e("a"), e("b")], "title", true, false, { kind: "text", get: (n) => values[n] }).map((x) => x.name);
    expect(out).toEqual(["b", "a"]);
  });

  it("shows nothing for a value that is not there and formats dates and sizes", () => {
    expect(cellValue(columnDef("artist")!, e("a"), undefined)).toBe("");
    expect(cellValue(columnDef("artist")!, e("a"), { artist: "Someone" })).toBe("Someone");
    expect(cellValue(columnDef("sizeOnDisk")!, e("a"), { sizeOnDisk: "2048" })).not.toBe("2048");
    expect(cellValue(columnDef("accessed")!, e("a"), { accessed: "1700000000000" })).not.toBe("");
    expect(cellValue(columnDef("extension")!, e("report.pdf"), undefined).toLowerCase()).toContain("pdf");
  });
});
