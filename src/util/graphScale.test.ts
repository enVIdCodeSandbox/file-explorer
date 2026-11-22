import { describe, expect, it } from "vitest";
import { filesLabel, graphEnd, niceMax, robustMax } from "./graphScale";

describe("graph time axis", () => {
  it("ends where the card says the job will end", () => {
    // 2 min 20 s in, 21 minutes left: the axis runs to 23 min 20 s, not 1.6 times what has passed.
    expect(graphEnd(140, 21 * 60, false)).toBe(140 + 1260);
  });
  it("ends at the last moment once the job is over", () => {
    expect(graphEnd(138, 0, true, 140)).toBe(140);
    expect(graphEnd(138, null, true)).toBe(138);
  });
  it("leaves a little room when there is no estimate yet", () => {
    expect(graphEnd(100, null, false)).toBeCloseTo(115);
    expect(graphEnd(0, null, false)).toBe(5);
  });
});

describe("graph speed axis", () => {
  it("is not flattened by one start-up burst", () => {
    const speeds = [2000, ...Array.from({ length: 99 }, () => 150)];
    const top = niceMax(robustMax(speeds, 1));
    expect(top).toBeLessThanOrEqual(200);
  });
  it("keeps a floor and handles nothing", () => {
    expect(robustMax([], 1)).toBe(1);
    expect(robustMax([0, 0], 0.1)).toBe(0.1);
  });
  it("labels small file rates with the decimals they need", () => {
    expect(filesLabel(0, 0.1)).toBe("0");
    expect(filesLabel(0.05, 0.1)).toBe("0.05");
    expect(filesLabel(0.5, 2)).toBe("0.5");
    expect(filesLabel(40, 50)).toBe("40");
  });
});

import { MIN_SPAN, panView, timeLabel, viewRange, zoomView } from "./graphScale";

describe("zooming the graph", () => {
  const now = 140;
  const end = 1400;
  it("shows everything until zoomed", () => {
    expect(viewRange(null, now, end, false)).toEqual([0, end]);
  });
  it("zooming in on the latest speeds follows them, and stops at one second", () => {
    let v = zoomView(null, 0.5, 0.02, now, end, false); // pointer on an earlier moment
    expect(v?.start).not.toBeNull();
    // Pointer in the empty room after "now" (frac near 1): follows the latest.
    let live = zoomView(null, 0.5, 0.95, now, end, false);
    expect(live).toEqual({ span: 700, start: null });
    for (let i = 0; i < 30; i++) live = zoomView(live, 0.5, 0.5, now, end, false);
    expect(live).toEqual({ span: MIN_SPAN, start: null });
    expect(viewRange(live, now, end, false)).toEqual([now - 1, now]);
    // As new speeds arrive the window keeps its size and moves along.
    expect(viewRange(live, now + 3, end, false)).toEqual([now + 2, now + 3]);
  });
  it("zooming all the way out shows everything again", () => {
    expect(zoomView({ span: 900, start: null }, 2, 0.5, now, end, false)).toBeNull();
  });
  it("zooms an earlier stretch around the pointer", () => {
    const v = zoomView(null, 0.5, 0.25, now, end, true)!;
    expect(v.span).toBe(700);
    expect(v.start).toBeCloseTo(350 - 0.25 * 700, 5);
  });
  it("panning away from the latest stops following; panning back catches up again", () => {
    const live = { span: 20, start: null } as const;
    const away = panView(live, -30, now, end, false)!;
    expect(away.start).toBe(90);
    expect(panView(away, 5, now, end, false)).toEqual({ span: 20, start: 95 });
    expect(panView(away, 100, now, end, false)).toEqual({ span: 20, start: null });
  });
  it("a finished job is never followed", () => {
    expect(viewRange({ span: 10, start: null }, 100, 100, true)).toEqual([90, 100]);
    expect(panView({ span: 10, start: 50 }, 100, 100, 100, true)).toEqual({ span: 10, start: 90 });
  });
  it("labels moments by the size of the view", () => {
    expect(timeLabel(0.5, 1)).toBe("0.5 sec");
    expect(timeLabel(12, 60)).toBe("12 sec");
    expect(timeLabel(128, 300)).toBe("2 min 8 sec");
  });
});

import { thinSeries } from "./graphScale";

describe("long series", () => {
  it("thins the old part and keeps the recent samples whole", () => {
    const pts = Array.from({ length: 6001 }, (_, i) => i);
    const thin = thinSeries(pts);
    expect(thin.length).toBeLessThan(5000);
    expect(thin.slice(-3000)).toEqual(pts.slice(-3000));
    expect(thinSeries([1, 2, 3])).toEqual([1, 2, 3]);
  });
});
