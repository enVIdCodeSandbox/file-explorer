import { describe, expect, it } from "vitest";
import { closestIndex, nextText } from "./typeFind";

const names = ["notes.txt", "Photos", "alpha", "Beta", "budget.xlsx", "zeta"];

describe("type to find", () => {
  it("prefers a name that starts with the text", () => {
    expect(names[closestIndex(names, "b")]).toBe("Beta");
    expect(names[closestIndex(names, "bu")]).toBe("budget.xlsx");
  });
  it("otherwise takes the next one alphabetically", () => {
    expect(names[closestIndex(names, "c")]).toBe("notes.txt");
    expect(names[closestIndex(names, "bz")]).toBe("notes.txt");
  });
  it("falls back to the last when nothing sorts after", () => {
    expect(names[closestIndex(names, "zz")]).toBe("zeta");
  });
  it("handles empty input", () => {
    expect(closestIndex([], "a")).toBe(-1);
    expect(closestIndex(names, "")).toBe(-1);
  });
  it("edits the typed text", () => {
    expect(nextText("ab", "c")).toBe("abc");
    expect(nextText("abc", "Backspace")).toBe("ab");
    expect(nextText("ab", "Enter")).toBeNull();
  });
});
