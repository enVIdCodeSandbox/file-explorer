import { describe, expect, it } from "vitest";
import type { FileEntry } from "./api";
import { emptyGroup, groupMatches, nameTest, parseSize } from "./util/selectGroup";

const f = (name: string, size = 10, modified = 1000, isDir = false): FileEntry => ({ name, isDir, isSymlink: false, isHidden: false, size, modified, created: null });

describe("select group", () => {
  const list = [f("a.jpg"), f("B.PNG"), f("notes.txt", 5000), f("README"), f("thumb_1.jpg"), f("docs", 0, 1000, true)];
  const names = (o: Partial<typeof emptyGroup>) => groupMatches(list, { ...emptyGroup, ...o })!.map((e) => e.name);

  it("matches wildcards, several at once, and leaves out the exclusions", () => {
    expect(names({ pattern: "*.jpg;*.png" })).toEqual(["a.jpg", "B.PNG", "thumb_1.jpg"]);
    expect(names({ pattern: "*.jpg|thumb*" })).toEqual(["a.jpg"]);
    expect(names({ pattern: "*.*" })).toHaveLength(5);
    expect(names({ pattern: "?.jpg" })).toEqual(["a.jpg"]);
  });
  it("can be case sensitive, or a regular expression", () => {
    expect(names({ pattern: "*.png", caseSensitive: true })).toEqual([]);
    expect(names({ pattern: "[a-z]+\\.(jpg|txt)", regex: true })).toEqual(["a.jpg", "notes.txt"]);
    expect(nameTest("(", true, false)).toBeNull();
  });
  it("filters by size, date and folders", () => {
    expect(names({ minSize: 1000 })).toEqual(["notes.txt"]);
    expect(names({ maxSize: 5, pattern: "*" })).toEqual([]);
    expect(names({ includeFolders: true, pattern: "d*" })).toEqual(["docs"]);
    expect(names({ newerThan: 2000 })).toEqual([]);
  });
  it("reads sizes", () => {
    expect(parseSize("10 KB")).toBe(10240);
    expect(parseSize("1.5mb")).toBe(1572864);
    expect(parseSize("300")).toBe(300);
    expect(parseSize("abc")).toBeNull();
  });
});
