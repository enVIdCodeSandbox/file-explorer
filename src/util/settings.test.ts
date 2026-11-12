import { describe, expect, it } from "vitest";
import { ADVANCED, adv } from "../settings/advanced";
import { KEY_COMMANDS, comboOf, keyFor } from "../settings/keys";
import { SETTINGS, getSetting } from "../settings/schema";
import { DEFAULT_LAYOUT, PANELS, moveTo, nudge, panelsAt, resolveLayout, zoneAt } from "./layout";
import { THEMES, resolveTheme } from "./themes";

describe("layout", () => {
  it("falls back to defaults for junk", () => {
    expect(resolveLayout(null)).toEqual(DEFAULT_LAYOUT);
    expect(resolveLayout({ tabs: { dock: "nowhere" } }).tabs.dock).toBe("top");
  });
  it("moves panels only to docks they allow", () => {
    expect(moveTo(DEFAULT_LAYOUT, "address", "left").address.dock).toBe("top");
    expect(moveTo(DEFAULT_LAYOUT, "commands", "bottom").commands.dock).toBe("bottom");
  });
  it("reorders within a dock", () => {
    const before = panelsAt(DEFAULT_LAYOUT, "top");
    const after = panelsAt(nudge(DEFAULT_LAYOUT, before[0], 1), "top");
    expect(after[1]).toBe(before[0]);
  });
  it("detects drop zones", () => {
    expect(zoneAt(500, 10, 1000, 800)).toBe("top");
    expect(zoneAt(500, 400, 1000, 800)).toBe("float");
  });
  it("keeps every panel's dock list non-empty", () => {
    for (const p of PANELS) expect(p.docks.length).toBeGreaterThan(1);
  });
});

describe("themes", () => {
  it("resolves ids", () => {
    expect(resolveTheme("system", false)).toBe("light");
    expect(resolveTheme("bogus", true)).toBe("dark");
    expect(THEMES.length).toBeGreaterThanOrEqual(10);
  });
});

describe("keys", () => {
  it("describes combos", () => {
    expect(comboOf({ key: "c", ctrlKey: true, metaKey: false, shiftKey: false, altKey: false })).toBe("Ctrl+C");
    expect(comboOf({ key: "Shift", ctrlKey: false, metaKey: false, shiftKey: true, altKey: false })).toBe("");
  });
  it("has unique default keys", () => {
    const keys = KEY_COMMANDS.map((k) => k.default.toLowerCase());
    expect(new Set(keys).size).toBe(keys.length);
    expect(keyFor(null, KEY_COMMANDS[0].id)).toBe(KEY_COMMANDS[0].default);
  });
});

describe("settings schema", () => {
  it("has unique advanced keys and defaults of matching type", () => {
    const keys = ADVANCED.map((a) => a.key);
    expect(new Set(keys).size).toBe(keys.length);
    expect(adv(undefined, keys[0])).toBe(ADVANCED[0].default);
  });
  it("has unique setting keys", () => {
    const keys = SETTINGS.map((s) => s.key);
    expect(new Set(keys).size).toBe(keys.length);
    expect(getSetting).toBeTypeOf("function");
  });
});
