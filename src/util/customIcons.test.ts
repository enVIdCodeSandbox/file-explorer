import { describe, expect, it } from "vitest";
import { driveSlot, iconMap, parseIcons, pick, prettyName, withIcon, safeSvg } from "./customIcons";

describe("custom icons", () => {
  it("keeps an app icon or a picture per kind of icon and resets one at a time", () => {
    let raw = withIcon("", "icon:home", { app: "star" });
    raw = withIcon(raw, "kind:folder", { img: "data:image/png;base64,AAAA" });
    expect(parseIcons(raw)).toEqual({ "icon:home": { app: "star" }, "kind:folder": { img: "data:image/png;base64,AAAA" } });
    raw = withIcon(raw, "icon:home", null);
    expect(Object.keys(parseIcons(raw))).toEqual(["kind:folder"]);
    expect(withIcon(raw, "kind:folder", null)).toBe("");
  });

  it("ignores anything that is not an icon, and pictures that are not pictures or are too large", () => {
    const raw = JSON.stringify({
      a: { app: "star" },
      b: { app: "<script>" },
      c: { img: "https://example.com/x.png" },
      d: { img: "data:text/html;base64,AAAA" },
      e: { img: "data:image/png;base64," + "A".repeat(200_000) },
      f: 7,
    });
    expect(parseIcons(raw)).toEqual({ a: { app: "star" } });
    expect(parseIcons("{nope")).toEqual({});
  });

  it("hands back the same object while the setting is unchanged", () => {
    const raw = withIcon("", "icon:pc", { app: "drive" });
    expect(iconMap(raw)).toBe(iconMap(raw));
    expect(iconMap(raw)["icon:pc"]).toEqual({ app: "drive" });
  });

  it("prefers a drive's own icon over the one for all drives", () => {
    const map = parseIcons(withIcon(withIcon("", "icon:drive", { app: "star" }), driveSlot("C:\\"), { app: "cpu" }));
    expect(pick(map, driveSlot("C:\\"), "icon:drive")).toEqual({ app: "cpu" });
    expect(pick(map, driveSlot("D:\\"), "icon:drive")).toEqual({ app: "star" });
    expect(pick(map, undefined, "icon:nothing")).toBeUndefined();
  });

  it("makes readable names", () => {
    expect(prettyName("folderOpen")).toBe("Folder open");
    expect(prettyName("pc")).toBe("Pc");
  });
});

describe("safeSvg", () => {
  const good = '<svg stroke="currentColor" fill="currentColor" stroke-width="0" viewBox="0 0 24 24" height="1em" width="1em" xmlns="http://www.w3.org/2000/svg"><path d="M0 0h24v24H0z" fill="none"></path><circle cx="1" cy="1" r="1"/></svg>';
  it("keeps drawing shapes with plain attributes", () => {
    expect(safeSvg(good)).toBe(good);
  });
  it("refuses scripts, event handlers, links, styles, images, text and trailing content", () => {
    expect(safeSvg('<svg><script>alert(1)</script></svg>')).toBeNull();
    expect(safeSvg('<svg onload="x()"></svg>')).toBeNull();
    expect(safeSvg('<svg><path d="M0 0" onclick="x()"></path></svg>')).toBeNull();
    expect(safeSvg('<svg><a href="javascript:x()"><path d="M0 0"></path></a></svg>')).toBeNull();
    expect(safeSvg('<svg><path style="x" d="M0 0"></path></svg>')).toBeNull();
    expect(safeSvg('<svg><foreignObject></foreignObject></svg>')).toBeNull();
    expect(safeSvg('<svg><image href="x"></image></svg>')).toBeNull();
    expect(safeSvg('<svg><path fill="url(#a)" d="M0 0"></path></svg>')).toBeNull();
    expect(safeSvg('<svg><text>hi</text></svg>')).toBeNull();
    expect(safeSvg('<svg></svg><svg></svg>')).toBeNull();
    expect(safeSvg('<svg></svg><script></script>')).toBeNull();
    expect(safeSvg('<svg><path d="M0 0">hello</path></svg>')).toBeNull();
    expect(safeSvg('<div></div>')).toBeNull();
    expect(safeSvg(42)).toBeNull();
    expect(safeSvg('<svg>' + "a".repeat(30_000))).toBeNull();
  });
  it("is what the saved setting accepts", () => {
    expect(parseIcons(JSON.stringify({ "kind:folder": { svg: good } }))).toEqual({ "kind:folder": { svg: good } });
    expect(parseIcons(JSON.stringify({ "kind:folder": { svg: "<svg><script></script></svg>" } }))).toEqual({});
  });
});
