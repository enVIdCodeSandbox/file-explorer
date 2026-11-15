import { describe, expect, it } from "vitest";
import { levelHits } from "./levelSearch";

const e = (name: string, isDir = false) => ({ name, isDir, isSymlink: false, isHidden: false, size: 1, modified: 5, created: null });

describe("level search", () => {
  const list = [e("Budget 2026.xlsx"), e("budget-old.xlsx"), e("Photos", true), e("notes.txt")];
  it("matches every word in any order, ignoring case", () => {
    expect(levelHits("/h", list, "2026 BUDGET").map((h) => h.name)).toEqual(["Budget 2026.xlsx"]);
  });
  it("only looks at the given items and gives full paths", () => {
    const h = levelHits("/h", list, "photos");
    expect(h).toHaveLength(1);
    expect(h[0].path).toBe("/h/Photos");
    expect(h[0].isDir).toBe(true);
  });
  it("finds nothing for an empty query", () => {
    expect(levelHits("/h", list, "  ")).toEqual([]);
  });
});
