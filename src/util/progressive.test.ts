import { describe, expect, it } from "vitest";
import { etaSeconds, loadingNotice } from "./progressive";

const p = (done: number, total: number) => ({ path: "/x", done, total, startedAt: 0 });

describe("progressive loading notice", () => {
  it("predicts from the pace so far", () => {
    expect(etaSeconds(p(1000, 4000), 2000)).toBeCloseTo(6);
  });
  it("gives no prediction when the total is unknown, nothing is in, it is finished, or it is too early", () => {
    expect(etaSeconds(p(10, 0), 5000)).toBeNull();
    expect(etaSeconds(p(0, 100), 5000)).toBeNull();
    expect(etaSeconds(p(100, 100), 5000)).toBeNull();
    expect(etaSeconds(p(10, 100), 100)).toBeNull();
  });
  it("words the notice", () => {
    const n = loadingNotice(p(1000, 4000), 2000, (s) => `${s} sec`);
    expect(n.title).toMatch(/still loading/i);
    expect(n.detail).toContain("1,000 of 4,000 items");
    expect(n.detail).toContain("About 6 sec left.");
    expect(loadingNotice(p(5, 0), 2000, (s) => `${s}`).detail).toContain("5 items so far");
  });
});

import { createPacer, forgetVisits, nextInterval, recallVisit, refreshLevel, refreshMinMs, rememberVisit } from "./progressive";

describe("a folder that has sent nothing yet", () => {
  it("says it is being read, and goes by how long it took last time", () => {
    const q = { path: "/x", done: 0, total: 0, startedAt: 0, lastMs: 8000 };
    expect(etaSeconds(q, 3000)).toBeCloseTo(5);
    const n = loadingNotice(q, 3000, (s) => `${s} sec`);
    expect(n.title).toMatch(/reading/i);
    expect(n.detail).toContain("About 5 sec left.");
    expect(n.fraction).toBeNull();
  });
  it("tells what to expect when it is not yet known how long", () => {
    expect(etaSeconds({ done: 0, total: 0, startedAt: 0 }, 3000)).toBeNull();
    const late = loadingNotice({ path: "/x", done: 0, total: 0, startedAt: 0, lastMs: 6000 }, 9000, (s) => `${s} sec`).detail;
    expect(late).toContain("Last time it took about 6 sec");
    expect(late).toContain("Taking longer than last time.");
  });
  it("shows a bar once items arrive, and says when the count is from last time", () => {
    const n = loadingNotice({ path: "/x", done: 250, total: 1000, totalFrom: "last-visit", startedAt: 0 }, 1000, (s) => `${s} s`);
    expect(n.fraction).toBeCloseTo(0.25);
    expect(n.detail).toContain("as it was last time");
  });
});

describe("remembering a folder's last visit", () => {
  it("keeps the newest few hundred", () => {
    forgetVisits();
    rememberVisit("/a", 10, 900);
    expect(recallVisit("/a")).toEqual({ count: 10, ms: 900 });
    for (let i = 0; i < 400; i++) rememberVisit(`/f${i}`, i, 1);
    expect(recallVisit("/a")).toBeUndefined();
    expect(recallVisit("/f399")).toBeDefined();
  });
});

describe("redraw pace", () => {
  const st = (mb: number, loadRefresh?: string) => ({ performance: { maxCacheMb: mb }, advanced: loadRefresh ? { "perf.loadRefresh": loadRefresh } : {} }) as never;
  it("follows the performance level unless chosen", () => {
    expect(refreshLevel(st(4096))).toBe("fast");
    expect(refreshLevel(st(1024))).toBe("normal");
    expect(refreshLevel(st(128))).toBe("gentle");
    expect(refreshMinMs(st(128, "fast"))).toBe(60);
    expect(refreshLevel(st(4096, "nonsense"))).toBe("fast");
  });
  it("stretches when drawing is slow, within limits", () => {
    expect(nextInterval(150, 10)).toBe(150);
    expect(nextInterval(150, 200)).toBe(600);
    expect(nextInterval(150, 5000)).toBe(1500);
  });
  it("lets the first through at once, then one per interval, always ending on the latest", () => {
    let t = 0;
    const timers: { at: number; fn: () => void }[] = [];
    const pacer = createPacer(100, () => t, (fn, ms) => { const h = { at: t + ms, fn }; timers.push(h); return h; }, (h) => { timers.splice(timers.indexOf(h as never), 1); });
    const drawn: number[] = [];
    pacer.push(() => drawn.push(1));
    expect(drawn).toEqual([1]);
    t = 10; pacer.push(() => drawn.push(2));
    t = 20; pacer.push(() => drawn.push(3));
    expect(drawn).toEqual([1]);
    expect(timers).toHaveLength(1);
    t = 100; timers.shift()!.fn();
    expect(drawn).toEqual([1, 3]);
    // Finishing draws what is waiting without delay.
    t = 110; pacer.push(() => drawn.push(4));
    pacer.flush();
    expect(drawn).toEqual([1, 3, 4]);
    // A slow draw lengthens the wait.
    pacer.drew(200);
    expect(pacer.interval).toBe(600);
    // Cancelling drops what is waiting.
    t = 120; pacer.push(() => drawn.push(5));
    pacer.cancel();
    expect(timers).toHaveLength(0);
    expect(drawn).toEqual([1, 3, 4]);
  });
});

import { createNotice } from "./progressive";
import { vi } from "vitest";

describe("the slow-folder notice", () => {
  it("appears for a folder that is slow to open", () => {
    vi.useFakeTimers();
    const show = vi.fn();
    createNotice(show);
    vi.advanceTimersByTime(249);
    expect(show).not.toHaveBeenCalled();
    vi.advanceTimersByTime(2);
    expect(show).toHaveBeenCalledTimes(1);
    vi.useRealTimers();
  });
  it("never appears once the folder is complete, even if it opened faster than the delay", () => {
    vi.useFakeTimers();
    const show = vi.fn();
    const n = createNotice(show);
    vi.advanceTimersByTime(100);
    n.stop();
    vi.advanceTimersByTime(5000);
    expect(show).not.toHaveBeenCalled();
    expect(n.stopped).toBe(true);
    vi.useRealTimers();
  });
});
