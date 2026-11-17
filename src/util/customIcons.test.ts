import { describe, expect, it } from "vitest";
import { driveSlot, iconMap, parseIcons, pick, prettyName, withIcon } from "./customIcons";

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
