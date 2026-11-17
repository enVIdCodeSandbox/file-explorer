import { describe, expect, it } from "vitest";
import { FONT_AREAS, fontStack } from "./fontAreas";

describe("fonts for parts of the app", () => {
  it("puts the chosen font first and the fallbacks after it", () => {
    expect(fontStack("Cascadia Code", "monospace")).toBe('"Cascadia Code", monospace');
    expect(fontStack(" Fira Code , JetBrains Mono ", "monospace")).toBe('"Fira Code", "JetBrains Mono", monospace');
  });
  it("follows the app when nothing is chosen", () => {
    expect(fontStack("", "monospace")).toBe("");
    expect(fontStack("   ", "monospace")).toBe("");
  });
  it("cannot be used to break out of the CSS value", () => {
    const out = fontStack('x"; color: red; "', "monospace");
    // Quotes are dropped, so the whole thing stays one quoted name.
    expect(out.match(/"/g)?.length).toBe(2);
    expect(out.endsWith(", monospace")).toBe(true);
  });
  it("uses an imported font under the name it was loaded as, and nothing until it has loaded", () => {
    expect(fontStack("imported:My.ttf", "serif", "Imported My")).toBe('"Imported My", serif');
    expect(fontStack("imported:My.ttf", "serif", null)).toBe("");
  });
  it("keeps every choice under its own key", () => {
    expect(new Set(FONT_AREAS.map((a) => a.key)).size).toBe(FONT_AREAS.length);
  });
});
