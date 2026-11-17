import { describe, expect, it } from "vitest";
import conf from "../src-tauri/tauri.conf.json";

// Tauri puts a one-time token in `style-src` when the page has an inline <style>. A browser then ignores 'unsafe-inline'
// next to it, which blocks every <style> the app adds later: the terminal's font and spacing, the accent colour and
// custom CSS. The page keeps its inline <style> for the first paint, so the token must not be added.
describe("the content security policy", () => {
  const security = conf.app.security;

  it("allows styles added while the app runs", () => {
    expect(security.csp).toMatch(/style-src[^;]*'unsafe-inline'/);
  });

  it("does not let Tauri add a token that would cancel that", () => {
    const off = security.dangerousDisableAssetCspModification as unknown;
    expect(off === true || (Array.isArray(off) && off.includes("style-src"))).toBe(true);
  });

  it("still lets Tauri guard the scripts", () => {
    expect(security.dangerousDisableAssetCspModification).not.toBe(true);
  });
});
