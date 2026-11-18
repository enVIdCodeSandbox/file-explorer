import { describe, expect, it } from "vitest";
import { ADVANCED, adv } from "../settings/advanced";
import { KEYS_OFF, KEY_COMMANDS, KEY_GROUPS, activeKey, clashesOf, comboFrom, comboOf, commandFor, commandsFor, isOn, keyFor, mouseComboOf, piecesOf } from "../settings/keys";
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
    // Several are not set by default; the keys that are set are never the same twice (apart from the pane keys and Space).
    const keys = KEY_COMMANDS.filter((k) => k.default && k.scope !== "panes" && k.id !== "toggleCheck").map((k) => k.default.toLowerCase());
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

describe("keybinding registry", () => {
  const ev = (key: string, mods: Partial<{ ctrlKey: boolean; shiftKey: boolean; altKey: boolean; metaKey: boolean; code: string }> = {}) =>
    ({ key, ctrlKey: false, shiftKey: false, altKey: false, metaKey: false, ...mods }) as unknown as KeyboardEvent;
  const withKeys = (keys: Record<string, string>, off: string[] = []) =>
    ({ keys, advanced: off.length ? { [KEYS_OFF]: off.join(",") } : {} }) as unknown as import("../api").Settings;

  it("finds the default command for a key", () => {
    expect(commandFor(null, ev("t", { ctrlKey: true }))).toBe("newTab");
    expect(commandFor(null, ev("F2"))).toBe("rename");
    expect(commandFor(null, ev("a", { ctrlKey: true }))).toBe("selectAll");
  });

  it("follows a changed binding and ignores the old one", () => {
    const s = withKeys({ newTab: "Ctrl+Alt+N" });
    expect(commandFor(s, ev("n", { ctrlKey: true, altKey: true }))).toBe("newTab");
    expect(commandFor(s, ev("t", { ctrlKey: true }))).toBeNull();
  });

  it("does nothing for a switched-off command", () => {
    const s = withKeys({}, ["rename"]);
    expect(commandFor(s, ev("F2"))).toBeNull();
    expect(isOn(s, "rename")).toBe(false);
    expect(activeKey(s, "rename")).toBe("");
    expect(keyFor(s, "rename")).toBe("F2");
  });

  it("does nothing for a command with no key", () => {
    expect(commandFor(null, ev("", {}))).toBeNull();
    expect(keyFor(null, "newFile")).toBe("");
  });

  it("lets Shift extend a cursor key and make Delete permanent", () => {
    expect(commandsFor(null, ev("ArrowDown", { shiftKey: true }))).toEqual(["cursorDown"]);
    expect(commandsFor(null, ev("Delete", { shiftKey: true }))).toEqual(["delete"]);
    // but not where the command does not take Shift
    expect(commandsFor(null, ev("F2", { shiftKey: true }))).toEqual([]);
  });

  it("treats = and + as one zoom key", () => {
    expect(commandFor(null, ev("=", { ctrlKey: true, code: "Equal" }))).toBe("zoomIn");
    expect(commandFor(null, ev("+", { ctrlKey: true, shiftKey: true, code: "Equal" }))).toBe("zoomIn");
    expect(commandFor(null, ev("+", { ctrlKey: true, code: "NumpadAdd" }))).toBe("zoomInNum");
    expect(commandFor(null, ev("-", { ctrlKey: true, code: "Minus" }))).toBe("zoomOut");
  });

  it("writes shifted symbols by the key they are on", () => {
    expect(comboOf({ key: ":", ctrlKey: true, shiftKey: true, altKey: false, metaKey: false, code: "Semicolon" } as never)).toBe("Ctrl+Shift+;");
    expect(commandFor(null, ev(":", { ctrlKey: true, shiftKey: true, code: "Semicolon" }))).toBe("focusCommandLine");
    expect(comboOf({ key: "+", ctrlKey: false, shiftKey: false, altKey: false, metaKey: false, code: "NumpadAdd" } as never)).toBe("Num+");
  });

  it("matches the side mouse buttons", () => {
    const m = (button: number, extra = {}) => ({ button, ctrlKey: false, shiftKey: false, altKey: false, metaKey: false, ...extra }) as unknown as MouseEvent;
    expect(mouseComboOf(m(3))).toBe("Mouse4");
    expect(mouseComboOf(m(4, { altKey: true }))).toBe("Alt+Mouse5");
    expect(mouseComboOf(m(0))).toBe("");
    expect(commandFor(null, m(3))).toBe("backMouse");
    expect(commandFor(null, m(4))).toBe("forwardMouse");
    expect(commandFor(withKeys({ forwardMouse: "" }), m(4))).toBeNull();
    expect(commandFor(withKeys({ goUp: "Mouse5", forwardMouse: "Alt+Mouse5" }), m(4))).toBe("goUp");
  });

  it("reports clashes only between commands that are on and can meet", () => {
    const s = withKeys({ rename: "Ctrl+T" });
    expect(clashesOf(s, "rename").map((k) => k.id)).toEqual(["newTab"]);
    expect(clashesOf(withKeys({ rename: "Ctrl+T" }, ["rename"]), "newTab")).toEqual([]);
    expect(clashesOf(null, "refresh")).toEqual([]); // the pane's F5 does not count
    expect(clashesOf(null, "quickLook")).toEqual([]); // Space is shared on purpose
  });

  it("splits and joins bindings for the picker", () => {
    expect(piecesOf("Ctrl+Shift+T")).toEqual({ ctrl: true, alt: false, shift: true, key: "T" });
    expect(piecesOf("Ctrl+Num+")).toEqual({ ctrl: true, alt: false, shift: false, key: "Num+" });
    expect(piecesOf("Ctrl++")).toEqual({ ctrl: true, alt: false, shift: false, key: "+" });
    expect(comboFrom({ ctrl: true, alt: false, shift: true, key: "T" })).toBe("Ctrl+Shift+T");
    expect(comboFrom({ ctrl: false, alt: false, shift: false, key: "" })).toBe("");
  });

  it("gives every command a unique id and a known group", () => {
    const ids = KEY_COMMANDS.map((k) => k.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const k of KEY_COMMANDS) expect(KEY_GROUPS).toContain(k.group);
  });
});
