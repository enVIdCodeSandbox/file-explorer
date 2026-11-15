import { describe, expect, it } from "vitest";
import { COLUMNS, DEFAULT_SET, canFill, columnDef, hideColumn, isDefaultSet, moveColumn, normalizeSet, setFromLegacyWidths, showColumn, widthOf } from "./util/columns";

describe("column sets", () => {
  it("has unique ids, a default width for each and Name first", () => {
    expect(new Set(COLUMNS.map((d) => d.id)).size).toBe(COLUMNS.length);
    for (const d of COLUMNS) expect(d.width).toBeGreaterThanOrEqual(d.min);
    expect(DEFAULT_SET.order[0]).toBe("name");
  });

  it("repairs what storage hands back", () => {
    expect(normalizeSet(null).order).toEqual(["name", "modified", "type", "size"]);
    const s = normalizeSet({ order: ["size", "bogus", "size", "name", "camera"], widths: { size: 5, camera: 9999, bogus: 100, type: "x" } });
    expect(s.order).toEqual(["name", "size", "camera"]);
    expect(s.widths).toEqual({ size: 50, camera: 1200 });
  });

  it("keeps the widths set before columns could be chosen", () => {
    const s = setFromLegacyWidths({ name: 300, modified: 200, type: 100, size: 90 });
    expect(s.order).toEqual(["name", "modified", "type", "size"]);
    expect(widthOf(s, "modified")).toBe(200);
    expect(widthOf(s, "created")).toBe(columnDef("created")!.width);
  });

  it("shows, hides and moves columns, never losing or moving Name", () => {
    let s = showColumn(DEFAULT_SET, "created");
    expect(s.order).toEqual(["name", "modified", "type", "size", "created"]);
    expect(showColumn(s, "created")).toBe(s);
    s = moveColumn(s, "created", -1);
    expect(s.order).toEqual(["name", "modified", "type", "created", "size"]);
    expect(moveColumn(s, "modified", -1)).toBe(s);
    expect(moveColumn(s, "name", 1)).toBe(s);
    expect(moveColumn(s, "size", 1)).toBe(s);
    expect(hideColumn(s, "name")).toBe(s);
    expect(hideColumn(s, "type").order).toEqual(["name", "modified", "created", "size"]);
    expect(isDefaultSet(DEFAULT_SET)).toBe(true);
    expect(isDefaultSet(s)).toBe(false);
  });

  it("knows which columns a phone or a server can fill", () => {
    expect(canFill(columnDef("modified")!, { remote: true })).toBe(true);
    expect(canFill(columnDef("camera")!, { remote: true })).toBe(false);
    expect(canFill(columnDef("camera")!, { remote: false })).toBe(true);
  });
});
