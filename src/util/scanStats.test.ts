import { describe, expect, it } from "vitest";
import { estimateTotal, expectedRate, pushLimited, recentRate, secondsLeft, sparkPath } from "./scanStats";

describe("scan statistics", () => {
  it("measures the recent pace", () => {
    const s = [0, 1, 2, 3, 4, 5].map((t) => ({ t, entries: t * 1000, bytes: 0 }));
    expect(recentRate(s)).toBeCloseTo(1000, 0);
    // A burst at the end shows up, an old slow start does not.
    const burst = [{ t: 0, entries: 0, bytes: 0 }, { t: 5, entries: 100, bytes: 0 }, { t: 6, entries: 5100, bytes: 0 }, { t: 8, entries: 9100, bytes: 0 }];
    expect(recentRate(burst)).toBeGreaterThan(2000);
    expect(recentRate([])).toBe(0);
    expect(recentRate([{ t: 1, entries: 5, bytes: 0 }])).toBe(0);
  });
  it("expects a drive's usual pace first, then its own", () => {
    expect(expectedRate("ssd", 0, 0)).toBe(30000);
    expect(expectedRate("hdd", 0, 0)).toBeLessThan(expectedRate("ssd", 0, 0));
    expect(expectedRate("hdd", 200000, 20)).toBe(10000); // after 10 s only its own average counts
    const mid = expectedRate("hdd", 20000, 5); // halfway between the usual 4000 and its own 4000... own is 4000
    expect(mid).toBeCloseTo(4000, 0);
    expect(expectedRate("strange", 0, 0)).toBe(8000);
  });
  it("estimates the total only from a fair sample", () => {
    expect(estimateTotal(1_000_000, 100, 1000)).toBeNull();
    // 100 GB used; 10,000 items hold 1 GB, so about a million items in all.
    expect(estimateTotal(100e9, 10000, 1e9)).toBe(1_000_000);
    expect(estimateTotal(1e9, 10000, 5e9)).toBe(10000); // never fewer than already found
    expect(estimateTotal(0, 10000, 1e9)).toBeNull();
  });
  it("tells the time left", () => {
    expect(secondsLeft(1000, 400, 100)).toBe(6);
    expect(secondsLeft(null, 400, 100)).toBeNull();
    expect(secondsLeft(1000, 400, 0)).toBeNull();
    expect(secondsLeft(1000, 2000, 100)).toBe(0);
  });
  it("draws a line and keeps a bounded history", () => {
    expect(sparkPath([1], 100, 20)).toBe("");
    const p = sparkPath([0, 5, 10], 100, 21);
    expect(p.startsWith("M0.0,20.5")).toBe(true);
    expect(p.endsWith("L100.0,0.5")).toBe(true);
    expect(pushLimited([1, 2, 3], 4, 3)).toEqual([2, 3, 4]);
  });
});
