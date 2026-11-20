import { describe, it, expect } from "vitest";
import { PANEL_LIMITS, SIDEBAR_LIMITS, clampFloat, clampPanelSize, clampSidebar, panelRange } from "./limits";
import { resolveLayout } from "./layout";

describe("size limits", () => {
  it("every limit has a smallest below its largest, and the details pane never goes below what its labels need", () => {
    for (const l of Object.values(PANEL_LIMITS)) {
      expect(l!.side[0]).toBeLessThan(l!.side[1]);
      expect(l!.edge[0]).toBeLessThan(l!.edge[1]);
    }
    expect(PANEL_LIMITS.details!.side[0]).toBeGreaterThanOrEqual(240);
    expect(SIDEBAR_LIMITS.min).toBeLessThan(SIDEBAR_LIMITS.max);
  });

  it("clamps a panel to its range on the side it is on", () => {
    expect(clampPanelSize("details", "right", 50)).toBe(240);
    expect(clampPanelSize("details", "right", 5000)).toBe(640);
    expect(clampPanelSize("details", "left", 400)).toBe(400);
    expect(clampPanelSize("details", "bottom", 5000)).toBe(420);
    expect(clampPanelSize("terminal", "bottom", 30)).toBe(110);
  });

  it("never takes more than 60% of a small window, but never less than its own smallest size", () => {
    expect(clampPanelSize("details", "right", 640, 800)).toBe(480);
    expect(clampPanelSize("details", "right", 640, 300)).toBe(240);
  });

  it("panels without limits, or not docked on an edge, have none", () => {
    expect(panelRange("tabs", "top")).toBeNull();
    expect(panelRange("details", "float")).toBeNull();
    expect(clampPanelSize("tabs", "top", 1234)).toBe(1234);
  });

  it("keeps floating panels and the navigation pane in range", () => {
    expect(clampFloat(10, 5000)).toEqual({ w: 200, h: 1000 });
    expect(clampSidebar(20)).toBe(150);
    expect(clampSidebar(9000)).toBe(480);
  });

  it("a saved layout from a bigger screen is brought back into range when it is loaded", () => {
    const l = resolveLayout({ details: { dock: "right", order: 0, collapsed: false, x: 0, y: 0, w: 100, h: 100, size: 3900 } });
    expect(l.details.size).toBe(640);
    const f = resolveLayout({ details: { dock: "float", order: 0, collapsed: false, x: 0, y: 0, w: 9000, h: 2 } });
    expect(f.details.w).toBe(1400);
    expect(f.details.h).toBe(60);
  });
});
