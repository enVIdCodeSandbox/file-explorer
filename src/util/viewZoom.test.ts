import { describe, expect, it } from "vitest";
import { ZOOM_PCT, nearestStep, resetSize, scaleOf, zoomBy, type ZoomState } from "./viewZoom";

const at = (level: number, details = 100, list = 100): ZoomState => ({ level, details, list });

describe("zooming the folder view", () => {
  it("makes Details bigger a step at a time before changing the view", () => {
    let s = at(0);
    const seen: number[] = [];
    for (let i = 0; i < 4; i++) {
      s = zoomBy(s, 1, 5);
      seen.push(s.details);
      expect(s.level).toBe(0);
    }
    expect(seen).toEqual([115, 130, 150, 175]);
  });
  it("moves on to List only after Details is as big as it goes, starting List small", () => {
    const s = zoomBy(at(0, 175), 1, 5);
    expect(s.level).toBe(1);
    expect(s.list).toBe(ZOOM_PCT[0]);
    expect(s.details).toBe(175);
  });
  it("moves on from List to the icon views after List is as big as it goes", () => {
    expect(zoomBy(at(1, 100, 150), 1, 5)).toMatchObject({ level: 1, list: 175 });
    expect(zoomBy(at(1, 100, 175), 1, 5).level).toBe(2);
  });
  it("zooms out the same way, and comes back into a view at its biggest", () => {
    expect(zoomBy(at(2), -1, 5)).toMatchObject({ level: 1, list: 175 });
    expect(zoomBy(at(1, 100, 70), -1, 5)).toMatchObject({ level: 0, details: 175 });
    expect(zoomBy(at(1, 100, 100), -1, 5)).toMatchObject({ level: 1, list: 90 });
  });
  it("stops at the smallest Details and the largest icons", () => {
    expect(zoomBy(at(0, 70), -1, 5)).toEqual(at(0, 70));
    expect(zoomBy(at(5), 1, 5)).toEqual(at(5));
  });
  it("takes several notches at once, and a saved size that is not a step", () => {
    expect(zoomBy(at(0), 3, 5).details).toBe(150);
    expect(zoomBy(at(0), -3, 5).details).toBe(70);
    expect(nearestStep(104)).toBe(3);
    expect(zoomBy(at(0, 104), 1, 5).details).toBe(115);
  });
  it("walks from Details through List to the icons and back", () => {
    let s = at(0, 70);
    for (let i = 0; i < 40; i++) s = zoomBy(s, 1, 5);
    expect(s.level).toBe(5);
    for (let i = 0; i < 40; i++) s = zoomBy(s, -1, 5);
    expect(s).toMatchObject({ level: 0, details: 70 });
  });
  it("resets the size of the view in use, and gives the scale", () => {
    expect(resetSize(at(0, 150)).details).toBe(100);
    expect(resetSize(at(1, 100, 80)).list).toBe(100);
    expect(resetSize(at(3, 150, 150))).toEqual(at(3, 150, 150));
    expect(scaleOf(at(0, 150))).toBe(1.5);
    expect(scaleOf(at(1, 100, 80))).toBe(0.8);
    expect(scaleOf(at(3, 150))).toBe(1);
  });
});
