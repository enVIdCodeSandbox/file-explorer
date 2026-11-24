import { describe, expect, it } from "vitest";
import { PERF_PRESETS, thumbViews } from "./presets";

describe("performance levels and thumbnail views", () => {
  it("a level without thumbnails switches every view off", () => {
    const p = thumbViews({ thumbs: false, thumbPx: 48 });
    expect(Object.values(p).filter((v) => v === true)).toEqual([]);
    expect(p["advanced.thumbs.videoPrewarm"]).toBe("off");
  });
  it("a light level shows the icon views and the pane only", () => {
    const p = thumbViews({ thumbs: true, thumbPx: 64 });
    expect(p["advanced.thumbs.inMedium"]).toBe(true);
    expect(p["advanced.thumbs.inPane"]).toBe(true);
    expect(p["advanced.thumbs.inSmall"]).toBe(false);
    expect(p["advanced.thumbs.inDetails"]).toBe(false);
    expect(p["advanced.thumbs.highPerfAlways"]).toBe(false);
  });
  it("the strongest levels show them everywhere and force them on", () => {
    const p = thumbViews({ thumbs: true, thumbPx: 256 });
    expect(p["advanced.thumbs.inDetails"]).toBe(true);
    expect(p["advanced.thumbs.inList"]).toBe(true);
    expect(p["advanced.thumbs.highPerfAlways"]).toBe(true);
    expect(p["advanced.thumbs.videoPrewarm"]).toBe("auto");
  });
  it("every preset carries the view switches, and no level keeps them on without thumbnails", () => {
    for (const preset of PERF_PRESETS) {
      expect("advanced.thumbs.inMedium" in preset.patch).toBe(true);
      if (preset.patch["performance.thumbnails"] === false) {
        expect(preset.patch["advanced.thumbs.inMedium"]).toBe(false);
        expect(preset.patch["advanced.thumbs.highPerfAlways"]).toBe(false);
      }
    }
  });
});
