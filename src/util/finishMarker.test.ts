import { describe, expect, it } from "vitest";
import { finishMark } from "./finishMarker";

describe("finishMark", () => {
  it("starts on the typical time", () => {
    expect(finishMark(12, 3, 40, 0.05)).toBeCloseTo(12, 5);
    expect(finishMark(12, 0, null, 0)).toBe(12);
  });
  it("moves toward the live finish as the transfer goes on", () => {
    const a = finishMark(12, 60, 240, 0.25);
    const b = finishMark(12, 120, 200, 0.5);
    expect(a).toBeGreaterThan(12);
    expect(b).toBeGreaterThan(a);
    expect(b).toBeLessThan(320);
  });
  it("sits on the live finish by three quarters", () => {
    expect(finishMark(12, 260, 87, 0.75)).toBeCloseTo(347, 5);
    expect(finishMark(12, 300, 47, 0.9)).toBeCloseTo(347, 5);
  });
  it("ends exactly on the finish", () => {
    expect(finishMark(12, 347, null, 1)).toBe(347);
  });
  it("falls back to the pace so far when no time left is known", () => {
    expect(finishMark(12, 100, null, 0.8)).toBeCloseTo(125, 5);
  });
});
