import { describe, expect, it } from "vitest";
import { safeSvg } from "./customIcons";
import { matchesQuery, packIconLabel, parsePack } from "./iconPacks";

const LUCIDE_BODY = '<path fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 16h.01m-7.798-4.423"/>';

describe("icon collections", () => {
  it("reads what the shell sends, in alphabetical order", () => {
    const list = parsePack(JSON.stringify({ w: 24, h: 24, icons: { b: LUCIDE_BODY, a: ["<path d=\"M0 0\"/>", 32, 16] } }));
    expect(list.map((i) => i.name)).toEqual(["a", "b"]);
    expect(list[0].svg).toContain('viewBox="0 0 32 16"');
    expect(list[1].svg).toContain('viewBox="0 0 24 24"');
  });

  it("gives an empty list for anything unexpected", () => {
    expect(parsePack("nope")).toEqual([]);
    expect(parsePack("null")).toEqual([]);
    expect(parsePack('{"icons":5}')).toEqual([]);
    expect(parsePack('{"w":1,"h":1,"icons":{"a":7}}')).toEqual([]);
  });

  it("draws an icon the way the shell stores it", () => {
    const [icon] = parsePack(JSON.stringify({ w: 24, h: 24, icons: { x: LUCIDE_BODY } }));
    expect(safeSvg(icon.svg)).not.toBeNull();
  });

  it("refuses an icon with a script or a link in it", () => {
    const [icon] = parsePack(JSON.stringify({ w: 24, h: 24, icons: { x: '<path d="M0 0"/><script>alert(1)</script>' } }));
    expect(safeSvg(icon.svg)).toBeNull();
    const [link] = parsePack(JSON.stringify({ w: 24, h: 24, icons: { x: '<path fill="url(#g)" d="M0 0"/>' } }));
    expect(safeSvg(link.svg)).toBeNull();
  });

  it("labels and matches names", () => {
    expect(packIconLabel("hard-drive")).toBe("Hard drive");
    expect(matchesQuery("arrow-up-right", "arrow right")).toBe(true);
    expect(matchesQuery("arrow-up", "arrow right")).toBe(false);
    expect(matchesQuery("anything", "  ")).toBe(true);
  });
});

describe("download progress", () => {
  it("fills toward the estimate, waits at 99% and is full only when saving", async () => {
    const { packFraction, packProgressText } = await import("./iconPacks");
    const p = (phase: "connecting" | "downloading" | "saving", done: number) => ({ id: "x", phase, done, total: 1000 });
    expect(packFraction(p("connecting", 0))).toBe(0);
    expect(packFraction(p("downloading", 250))).toBe(0.25);
    expect(packFraction(p("downloading", 5000))).toBe(0.99);
    expect(packFraction(p("saving", 1000))).toBe(1);
    expect(packProgressText(p("downloading", 500))).toMatch(/of about/);
    expect(packProgressText(p("connecting", 0))).toBe("Connecting…");
  });
});
