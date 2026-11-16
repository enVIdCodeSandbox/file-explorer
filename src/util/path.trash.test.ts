import { describe, expect, it } from "vitest";
import { TRASH, THIS_PC, locationTitle, parseLocation, trashLabel } from "./path";

describe("the recycle bin location", () => {
  it("is its own kind of place", () => {
    expect(parseLocation(TRASH)).toEqual({ kind: "trash" });
    expect(parseLocation(THIS_PC)).toEqual({ kind: "thispc" });
    expect(TRASH.startsWith("fast://")).toBe(true);
  });

  it("is called what the system calls it", () => {
    expect(trashLabel("windows")).toBe("Recycle Bin");
    expect(trashLabel("macos")).toBe("Trash");
    expect(trashLabel("linux")).toBe("Trash");
    expect(locationTitle(TRASH, "This PC")).toBe("Trash");
  });
});
