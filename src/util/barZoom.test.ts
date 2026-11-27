import { describe, expect, it } from "vitest";
import { BAR_SCALE_LIMITS, UI_ICON_LIMITS } from "./limits";
import { barAtElement, stepBarScale, stepIconSize } from "./barZoom";

function mockEl(matches: string[]): Element {
  return {
    closest(selector: string) {
      const parts = selector.split(",").map((s) => s.trim());
      return parts.some((p) => matches.includes(p)) ? ({} as Element) : null;
    },
  } as unknown as Element;
}

describe("barAtElement", () => {
  it("recognizes top bar and left bar containers while excluding terminal and dialogs", () => {
    expect(barAtElement(mockEl([".win10-ribbon"]))).toBe("top");
    expect(barAtElement(mockEl([".toolbar"]))).toBe("top");
    expect(barAtElement(mockEl([".caption-strip"]))).toBe("top");
    expect(barAtElement(mockEl([".sidebar-wrap:not([data-horizontal])"]))).toBe("left");
    expect(barAtElement(mockEl([".dock-left .sidebar-wrap"]))).toBe("left");
    expect(barAtElement(mockEl([".term-body"]))).toBeNull();
    expect(barAtElement(mockEl([".win10-ribbon", ".context-menu"]))).toBeNull();
    expect(barAtElement(null)).toBeNull();
  });
});

describe("stepBarScale", () => {
  it("steps and clamps within BAR_SCALE_LIMITS and resets on 0", () => {
    expect(stepBarScale(100, 1)).toBe(105);
    expect(stepBarScale(100, -1)).toBe(95);
    expect(stepBarScale(BAR_SCALE_LIMITS.max, 2)).toBe(BAR_SCALE_LIMITS.max);
    expect(stepBarScale(BAR_SCALE_LIMITS.min, -2)).toBe(BAR_SCALE_LIMITS.min);
    expect(stepBarScale(140, 0)).toBe(100);
  });
});

describe("stepIconSize", () => {
  it("steps and clamps within UI_ICON_LIMITS and resets on 0", () => {
    expect(stepIconSize(16, 1)).toBe(18);
    expect(stepIconSize(20, -1)).toBe(18);
    expect(stepIconSize(UI_ICON_LIMITS.max, 5)).toBe(UI_ICON_LIMITS.max);
    expect(stepIconSize(UI_ICON_LIMITS.min, -5)).toBe(UI_ICON_LIMITS.min);
    expect(stepIconSize(28, 0)).toBe(UI_ICON_LIMITS.default);
  });
});
