import { describe, expect, it } from "vitest";
import { moveGroupBlock } from "./tabs";

const t = (id: string, groupId: string | null = null) => ({ id, groupId });
const ids = (v: { id: string }[]) => v.map((x) => x.id).join("");

describe("moveGroupBlock", () => {
  const tabs = [t("a"), t("b", "g"), t("c", "g"), t("d"), t("e")];
  it("moves the whole group to the end", () => expect(ids(moveGroupBlock(tabs, "g", 3))).toBe("adebc"));
  it("moves the whole group to the front", () => expect(ids(moveGroupBlock(tabs, "g", 0))).toBe("bcade"));
  it("keeps members in order and clamps the index", () => expect(ids(moveGroupBlock(tabs, "g", 99))).toBe("adebc"));
  it("moves between other tabs", () => expect(ids(moveGroupBlock(tabs, "g", 2))).toBe("adbce"));
  it("ignores an unknown group", () => expect(moveGroupBlock(tabs, "x", 1)).toBe(tabs));
});
