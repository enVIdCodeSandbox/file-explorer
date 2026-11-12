import { describe, expect, it } from "vitest";
import { DOC_PAGES, searchDocs, sectionsOf } from "./pages";
import { parse, slug } from "./markdown";

describe("documentation", () => {
  it("bundles every page with content", () => {
    expect(DOC_PAGES.length).toBeGreaterThanOrEqual(8);
    for (const p of DOC_PAGES) expect(p.text.length).toBeGreaterThan(200);
    expect(new Set(DOC_PAGES.map((p) => p.id)).size).toBe(DOC_PAGES.length);
  });

  it("parses headings, lists, tables and code", () => {
    const b = parse("# T\n\ntext **b**\n\n- a\n- b\n\n| x | y |\n|---|---|\n| 1 | 2 |\n\n```\ncode\n```\n");
    expect(b.map((x) => x.t)).toEqual(["h", "p", "ul", "table", "code"]);
    expect(b[3]).toMatchObject({ head: ["x", "y"], rows: [["1", "2"]] });
  });

  it("keeps pipes inside escaped table cells", () => {
    const [t] = parse("| a |\n|---|\n| x \\| y |");
    expect(t).toMatchObject({ rows: [["x | y"]] });
  });

  it("finds sections by all search words", () => {
    const hits = searchDocs("force stop");
    expect(hits.length).toBeGreaterThan(0);
    expect(hits.some((h) => /force/i.test(h.heading) || /force stop/i.test(h.snippet))).toBe(true);
    expect(searchDocs("zzzzqqqq")).toEqual([]);
    expect(searchDocs("rclone nfs smb").some((h) => h.page.id === "transfers")).toBe(true);
  });

  it("does not split sections inside code fences", () => {
    const s = sectionsOf("## A\n```\n# not a heading\n```\n## B\nx");
    expect(s.map((x) => x.heading)).toEqual(["A", "B"]);
  });

  it("makes anchors", () => expect(slug("Pause, resume, cancel")).toBe("pause-resume-cancel"));

  it("every internal link points at a bundled page", () => {
    const files = new Set(DOC_PAGES.map((p) => p.file));
    for (const p of DOC_PAGES) {
      for (const m of p.text.matchAll(/\]\(([^)#]+\.md)(#[^)]*)?\)/g)) {
        expect(files.has(m[1].replace(/^.*\//, "")), `${p.file} links to ${m[1]}`).toBe(true);
      }
    }
  });
});
