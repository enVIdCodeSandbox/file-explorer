import { describe, expect, it } from "vitest";
import type { FileEntry } from "./api";
import { BLANK, applyColumnFilters, columnFacets, dateBucket, sizeBucket } from "./util/columnFilter";
import { DEFAULT_SET, columnDef, moveColumnTo, showColumn } from "./util/columns";
import { NO_FILTER } from "./store";
import { isFiltering } from "./util/filter";
import { columnsPerFolder } from "./folderViews";

const file = (name: string, size = 0, modified: number | null = null): FileEntry => ({ name, isDir: false, size, modified, created: null } as FileEntry);
const defOf = (id: string) => columnDef(id);

describe("column filters", () => {
  const now = new Date("2026-11-15T15:00:00").getTime();
  const DAY = 86_400_000;
  const entries = [file("a.jpg", 5000, now - 1000), file("b.jpg", 3_000_000, now - 2 * DAY), file("c.txt", 0, now - 400 * DAY), file("d.png", 200_000_000, null)];
  const extra = { "a.jpg": { camera: "Canon" }, "b.jpg": { camera: "Nikon" }, "c.txt": {}, "d.png": { camera: "Canon" } };

  it("buckets dates and sizes", () => {
    expect(dateBucket(now - 1000, now)).toBe("Today");
    expect(dateBucket(now - DAY - 1000, now)).toBe("Yesterday");
    expect(dateBucket(now - 400 * DAY, now)).toBe("A long time ago");
    expect(sizeBucket(0)).toBe("Empty");
    expect(sizeBucket(3_000_000)).toBe("Medium (up to 128 MB)");
    expect(sizeBucket(2_000_000_000)).toBe("Huge (over 1 GB)");
  });
  it("keeps items whose value is ticked, on text columns read from the files", () => {
    const out = applyColumnFilters(entries, { camera: ["Canon"] }, defOf, extra, now);
    expect(out.map((e) => e.name)).toEqual(["a.jpg", "d.png"]);
  });
  it("offers (Blank) for items with no value, last", () => {
    const f = columnFacets(columnDef("camera")!, entries, extra, now);
    expect(f).toEqual([["Canon", 2], ["Nikon", 1], [BLANK, 1]]);
    expect(applyColumnFilters(entries, { camera: [BLANK] }, defOf, extra, now).map((e) => e.name)).toEqual(["c.txt"]);
  });
  it("filters dates and sizes by range and combines columns", () => {
    expect(applyColumnFilters(entries, { modified: ["Today", "Yesterday", "Earlier this week"] }, defOf, extra, now).map((e) => e.name)).toEqual(["a.jpg", "b.jpg"]);
    expect(applyColumnFilters(entries, { size: ["Medium (up to 128 MB)"], extension: ["jpg"] }, defOf, extra, now).map((e) => e.name)).toEqual(["b.jpg"]);
    expect(columnFacets(columnDef("size")!, entries, extra, now).map(([v]) => v)).toEqual(["Empty", "Tiny (up to 16 KB)", "Medium (up to 128 MB)", "Large (up to 1 GB)"]);
  });
  it("an empty selection filters nothing, and counts as no filter", () => {
    expect(applyColumnFilters(entries, { camera: [] }, defOf, extra, now)).toHaveLength(4);
    expect(isFiltering({ ...NO_FILTER, cols: { camera: [] } })).toBe(false);
    expect(isFiltering({ ...NO_FILTER, cols: { camera: ["Canon"] } })).toBe(true);
    // A filter saved before this existed has no cols at all.
    expect(isFiltering({ ...NO_FILTER, cols: undefined as never })).toBe(false);
  });
});

describe("moving a column to a place", () => {
  const set = showColumn(showColumn(DEFAULT_SET, "created"), "extension"); // name modified type size created extension
  it("puts it there, never ahead of Name", () => {
    expect(moveColumnTo(set, "extension", 1).order).toEqual(["name", "extension", "modified", "type", "size", "created"]);
    expect(moveColumnTo(set, "modified", 0).order[0]).toBe("name");
    expect(moveColumnTo(set, "modified", 99).order.at(-1)).toBe("modified");
    expect(moveColumnTo(set, "name", 3)).toBe(set);
  });
});

describe("same columns in every folder", () => {
  const s = (perFolder: boolean, everywhere: boolean) => ({ advanced: { "files.perFolderView": perFolder, "files.columnsEverywhere": everywhere } }) as never;
  it("is per folder only when views are kept per folder and the option is off", () => {
    expect(columnsPerFolder(s(true, false))).toBe(true);
    expect(columnsPerFolder(s(true, true))).toBe(false);
    expect(columnsPerFolder(s(false, false))).toBe(false);
  });
});

import { COLUMNS, COLUMN_GROUPS } from "./util/columns";
import { cellValue, rawValue } from "./util/cellValue";

describe("the column catalog", () => {
  const ids = COLUMNS.map((c) => c.id);
  it("is large, with every column in a known group, once, and sort targets that exist", () => {
    expect(COLUMNS.length).toBeGreaterThanOrEqual(130);
    expect(new Set(ids).size).toBe(ids.length);
    for (const c of COLUMNS) {
      expect(COLUMN_GROUPS).toContain(c.group);
      if (c.sortBy) expect(ids).toContain(c.sortBy);
    }
    for (const g of COLUMN_GROUPS) expect(COLUMNS.some((c) => c.group === g)).toBe(true);
  });
  it("derived columns work for servers and phones too", () => {
    for (const c of COLUMNS.filter((c) => c.derive)) expect(c.source).toBe("listing");
  });
  const entry = (over: Partial<FileEntry> = {}): FileEntry => ({ name: "Holiday.2024.JPG", isDir: false, isSymlink: false, isHidden: false, size: 2_500_000, modified: new Date(2026, 10, 15, 9, 5).getTime(), created: null, ...over }) as FileEntry;
  const col = (id: string) => columnDef(id)!;
  it("works out names, kinds, dates and sizes from the listing", () => {
    const e = entry();
    expect(cellValue(col("baseName"), e, undefined)).toBe("Holiday.2024");
    expect(cellValue(col("baseName"), entry({ name: ".bashrc" }), undefined)).toBe(".bashrc");
    expect(cellValue(col("kind"), e, undefined)).toBe("Image");
    expect(cellValue(col("initial"), e, undefined)).toBe("H");
    expect(cellValue(col("initial"), entry({ name: "2024 notes" }), undefined)).toBe("#");
    expect(cellValue(col("modifiedDate"), e, undefined)).toBe("2026-11-15");
    expect(cellValue(col("modifiedTime"), e, undefined)).toBe("09:05");
    expect(cellValue(col("modifiedWeekday"), e, undefined)).toBe("Sunday");
    expect(rawValue(col("modifiedWeekday"), e, undefined)).toBe("6");
    expect(cellValue(col("modifiedMonth"), e, undefined)).toBe("November");
    expect(cellValue(col("modifiedYear"), e, undefined)).toBe("2026");
    expect(cellValue(col("sizeBytes"), e, undefined)).toBe((2_500_000).toLocaleString());
    expect(rawValue(col("sizeKB"), e, undefined)).toBe("2500");
    expect(cellValue(col("sizeMB"), e, undefined)).toContain("2.5");
    expect(cellValue(col("hidden"), entry({ isHidden: true }), undefined)).toBe("Yes");
    expect(cellValue(col("hidden"), e, undefined)).toBe("");
    expect(cellValue(col("empty"), entry({ size: 0 }), undefined)).toBe("Yes");
    expect(rawValue(col("empty"), entry({ isDir: true, size: 0 }), undefined)).toBeUndefined();
    expect(cellValue(col("modifiedDate"), entry({ modified: null }), undefined)).toBe("");
  });
  it("shows what the files said, formatted by column", () => {
    const e = entry();
    expect(cellValue(col("archiveRatio"), e, { archiveRatio: "34" })).toBe("34%");
    expect(cellValue(col("megapixels"), e, { megapixels: "12.0" })).toBe("12.0 MP");
    expect(cellValue(col("width"), e, { width: "4000" })).toBe("4000 px");
    expect(cellValue(col("sha1"), e, { sha1: "abc" })).toBe("abc");
    expect(cellValue(col("docCreated"), e, { docCreated: "1715263402000" })).not.toBe("1715263402000");
    expect(cellValue(col("exeArch"), e, undefined)).toBe("");
  });
});
