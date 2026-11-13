import { describe, expect, it } from "vitest";
import type { FileEntry } from "../api";
import { NO_FILTER } from "../store";
import { applyFilter, isFiltering } from "./filter";

const e = (name: string, isDir = false, size = 10, modified = Date.now()): FileEntry => ({ name, isDir, size, modified } as FileEntry);
const list = [e("docs", true), e("a.png", false, 5), e("b.txt", false, 0), e("old.log", false, 200 * 1024 * 1024, 1000), e("backup.zip", false, 3 * 1024 * 1024)];
const names = (f: Partial<typeof NO_FILTER>) => applyFilter(list, { ...NO_FILTER, ...f }).map((x) => x.name);

describe("inverse filters", () => {
  it("is not a folder", () => expect(names({ notKinds: ["folder"] })).toEqual(["a.png", "b.txt", "old.log", "backup.zip"]));
  it("is not an image", () => expect(names({ notKinds: ["image"] })).not.toContain("a.png"));
  it("extension is not", () => expect(names({ notExtensions: ["txt", "log"] })).toEqual(["docs", "a.png", "backup.zip"]));
  it("name does not contain", () => expect(names({ notText: "BACK" })).not.toContain("backup.zip"));
  it("size is not (folders are not sized, so they stay)", () => {
    expect(names({ notSize: "empty" })).toEqual(["docs", "a.png", "old.log", "backup.zip"]);
    expect(names({ notSize: "large" })).not.toContain("old.log");
  });
  it("date is not", () => expect(names({ notModified: "older" })).not.toContain("old.log"));
  it("combines with the positive filters", () => expect(names({ kinds: ["archive", "code"], notExtensions: ["zip"] })).toEqual([]));
  it("counts as filtering", () => {
    expect(isFiltering(NO_FILTER)).toBe(false);
    expect(isFiltering({ ...NO_FILTER, notKinds: ["folder"] })).toBe(true);
    expect(isFiltering({ ...NO_FILTER, notSize: "small" })).toBe(true);
  });
});
