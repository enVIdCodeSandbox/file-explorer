import { describe, expect, it } from "vitest";
import { failedGroups, folderOf } from "./util/retryFailed";

const err = (path: string, dest?: string) => ({ path, message: "in use", needsElevation: false, dest });

describe("retrying only the files that failed", () => {
  it("sends each failed file to the folder it was headed for", () => {
    const groups = failedGroups([
      err("C:\\src\\survival lol\\region\\r.0.0.mca", "D:\\copy\\survival lol\\region\\r.0.0.mca"),
      err("C:\\src\\survival lol\\region\\r.0.1.mca", "D:\\copy\\survival lol\\region\\r.0.1.mca"),
      err("C:\\src\\survival lol\\level.dat", "D:\\copy\\survival lol\\level.dat"),
    ]);
    expect(groups).toEqual([
      { destDir: "D:\\copy\\survival lol\\region", sources: ["C:\\src\\survival lol\\region\\r.0.0.mca", "C:\\src\\survival lol\\region\\r.0.1.mca"] },
      { destDir: "D:\\copy\\survival lol", sources: ["C:\\src\\survival lol\\level.dat"] },
    ]);
  });

  it("works the destination out from what was asked for when it was not reported", () => {
    const spec = { sources: ["C:\\src\\survival lol", "C:\\src\\notes.txt"], destDir: "D:\\copy" };
    expect(failedGroups([err("C:\\src\\Survival Lol\\region\\a.mca")], spec)).toEqual([{ destDir: "D:\\copy\\survival lol\\region", sources: ["C:\\src\\Survival Lol\\region\\a.mca"] }]);
    expect(failedGroups([err("C:\\src\\notes.txt")], spec)).toEqual([{ destDir: "D:\\copy", sources: ["C:\\src\\notes.txt"] }]);
    // Nothing to go on: no retry is offered.
    expect(failedGroups([err("C:\\elsewhere\\x.txt")], spec)).toEqual([]);
    expect(failedGroups([err("C:\\src\\x.txt")])).toEqual([]);
  });

  it("names the folder an item is in", () => {
    expect(folderOf("C:\\src\\survival lol\\region\\r.0.0.mca")).toBe("C:\\src\\survival lol\\region");
    expect(folderOf("/home/me/a.txt")).toBe("/home/me");
  });
});
