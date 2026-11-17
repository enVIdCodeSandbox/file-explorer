import { describe, expect, it } from "vitest";
import { defaultLayout, isDefault, moveNav, parseLayout, placed, serializeLayout, zoneOf, type Zone, type NavId } from "./sideLayout";

describe("the navigation entries' places", () => {
  it("starts with everything at the top, in order", () => {
    expect(parseLayout(undefined).top).toEqual(["home", "thispc", "trash", "analyze", "clear"]);
    expect(isDefault(parseLayout(""))).toBe(true);
  });

  it("moves an entry into a section and keeps it there after saving and loading", () => {
    const moved = moveNav(defaultLayout(), "clear", "pinned");
    expect(zoneOf(moved, "clear")).toBe("pinned");
    expect(moved.top).not.toContain("clear");
    const again = parseLayout(serializeLayout(moved));
    expect(again.pinned).toEqual(["clear"]);
    expect(isDefault(again)).toBe(false);
  });

  it("places an entry before or after another", () => {
    let l = moveNav(defaultLayout(), "home", "drives");
    l = moveNav(l, "trash", "drives", "home", "after");
    l = moveNav(l, "analyze", "drives", "home", "before");
    expect(l.drives).toEqual(["analyze", "home", "trash"]);
  });

  it("can leave an entry between two sections, which stays when a section is turned off", () => {
    const l = moveNav(defaultLayout(), "home", "below:pinned");
    expect(parseLayout(serializeLayout(l))["below:pinned"]).toEqual(["home"]);
    const shown = new Set<Zone>(["top", "frequent"]);
    expect(placed(l, shown, new Set())["below:pinned"]).toEqual(["home"]);
  });

  it("puts right what is missing, repeated or unknown", () => {
    const l = parseLayout(JSON.stringify({ top: ["home", "home", "bogus"], pinned: ["home", "clear"], elsewhere: ["trash"] }));
    expect(l.top).toEqual(["home", "thispc", "trash", "analyze"]);
    expect(l.pinned).toEqual(["clear"]);
    expect(parseLayout("{not json").top).toHaveLength(5);
  });

  it("shows the entries of a section that is turned off at the top, and leaves hidden ones out", () => {
    const l = moveNav(moveNav(defaultLayout(), "clear", "recents"), "trash", "pinned");
    const shown = new Set<Zone>(["top", "pinned", "drives"]);
    const out = placed(l, shown, new Set<NavId>(["analyze"]));
    expect(out.pinned).toEqual(["trash"]);
    expect(out.recents).toEqual([]);
    expect(out.top).toEqual(["home", "thispc", "clear"]);
    // Turned on again: it is back in its section.
    expect(placed(l, new Set<Zone>(["top", "recents", "pinned"]), new Set()).recents).toEqual(["clear"]);
  });
});
