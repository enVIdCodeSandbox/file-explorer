import { describe, expect, it } from "vitest";
import { THEMES } from "./themes";
import { auditTheme, contrast, over, parseColor, themeVars } from "./themeAudit";

// The css file is read from disk: importing it with ?raw gives an empty string under the test runner's CSS handling.
// (The module name is joined so the type checker, which has no Node types here, does not look for it.)
const { readFileSync } = (await import(/* @vite-ignore */ "node:" + "fs")) as { readFileSync: (p: URL, enc: string) => string };
const css = readFileSync(new URL("../styles/tokens.css", import.meta.url), "utf8");

describe("color maths", () => {
  it("reads the colors the themes use", () => {
    expect(parseColor("#fff")).toEqual([255, 255, 255, 1]);
    expect(parseColor("#1c1c1c")).toEqual([28, 28, 28, 1]);
    expect(parseColor("rgba(255, 255, 255, .06)")).toEqual([255, 255, 255, 0.06]);
    expect(parseColor("color-mix(in srgb, red 20%, transparent)")).toBeNull();
  });
  it("matches the published contrast ratios", () => {
    expect(contrast([0, 0, 0, 1], [255, 255, 255, 1])).toBeCloseTo(21, 0);
    expect(contrast([119, 119, 119, 1], [255, 255, 255, 1])).toBeCloseTo(4.48, 1);
  });
  it("lays a translucent color over the page", () => {
    expect(over([255, 255, 255, 0.5], [0, 0, 0, 1])).toEqual([127.5, 127.5, 127.5, 1]);
  });
});

describe("every built-in theme is readable", () => {
  for (const t of THEMES) {
    it(t.name, () => {
      expect(auditTheme(themeVars(css, t.id))).toEqual([]);
    });
  }
});
