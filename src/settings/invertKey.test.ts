import { describe, expect, it } from "vitest";
import type { Settings } from "../api";
import { KEYS_OFF, commandsFor, keyFor } from "./keys";

const press = (key: string, extra: Partial<KeyboardEvent> = {}) => ({ key, ctrlKey: false, metaKey: false, shiftKey: false, altKey: false, ...extra }) as KeyboardEvent;
const withSettings = (patch: { keys?: Record<string, string>; off?: string }) => ({ keys: patch.keys ?? {}, advanced: patch.off ? { [KEYS_OFF]: patch.off } : {} }) as unknown as Settings;

describe("Ctrl+I inverts the selection", () => {
  it("is the default key, and the numeric keypad star still works as the second key", () => {
    expect(commandsFor(null, press("i", { ctrlKey: true }))).toContain("invertSelection");
    expect(keyFor(null, "invertSelectionAlt")).toBe("Num*");
  });
  it("can be given another key", () => {
    const s = withSettings({ keys: { invertSelection: "Ctrl+Shift+X" } });
    expect(commandsFor(s, press("i", { ctrlKey: true }))).not.toContain("invertSelection");
    expect(commandsFor(s, press("x", { ctrlKey: true, shiftKey: true }))).toContain("invertSelection");
  });
  it("does nothing when set to nothing", () => {
    const s = withSettings({ keys: { invertSelection: "" } });
    expect(commandsFor(s, press("i", { ctrlKey: true }))).toEqual([]);
  });
  it("does nothing when turned off, and remembers its key", () => {
    const s = withSettings({ off: "invertSelection" });
    expect(commandsFor(s, press("i", { ctrlKey: true }))).toEqual([]);
    expect(keyFor(s, "invertSelection")).toBe("Ctrl+I");
  });
});
