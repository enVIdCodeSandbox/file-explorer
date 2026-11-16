import { describe, expect, it } from "vitest";
import { applyStats, changedSince, intervalFor, liveKey, liveSummary, maxFollowed, rateOf, type LiveFile } from "./liveFiles";

const stat = (path: string, size: number, modified: number | null = 1) => ({ path, exists: true, size, modified });

describe("rateOf", () => {
  it("is zero with fewer than two samples", () => expect(rateOf([[0, 5]], 0)).toBe(0));
  it("is bytes per second over the window", () => expect(rateOf([[1000, 0], [3000, 2000]], 3000)).toBe(1000));
  it("ignores samples older than the window", () => expect(rateOf([[0, 0], [9000, 100], [10000, 300]], 10000)).toBe(200));
});

describe("applyStats", () => {
  it("does not call a file growing on first sight", () => {
    const { files } = applyStats({}, [stat("/a", 10)], 0, 4000);
    expect(files["/a"].growing).toBe(false);
  });
  it("follows growth, then calls it finished after the quiet time", () => {
    let r = applyStats({}, [stat("/a", 10)], 0, 4000);
    r = applyStats(r.files, [stat("/a", 1010)], 1000, 4000);
    expect(r.files["/a"].growing).toBe(true);
    expect(r.files["/a"].rate).toBe(1000);
    r = applyStats(r.files, [stat("/a", 1010)], 3000, 4000);
    expect(r.files["/a"].growing).toBe(true); // paused, not finished
    expect(r.finished).toEqual([]);
    r = applyStats(r.files, [stat("/a", 1010)], 6000, 4000);
    expect(r.files["/a"].growing).toBe(false);
    expect(r.finished).toEqual(["/a"]);
  });
  it("forgets a file that is gone", () => {
    let r = applyStats({}, [stat("/a", 10)], 0, 4000);
    r = applyStats(r.files, [{ path: "/a", exists: false, size: 0, modified: null }], 1000, 4000);
    expect(r.files["/a"]).toBeUndefined();
  });
  it("keeps the fastest rate", () => {
    let r = applyStats({}, [stat("/a", 0)], 0, 4000);
    r = applyStats(r.files, [stat("/a", 5000)], 1000, 4000);
    r = applyStats(r.files, [stat("/a", 5100)], 2000, 4000);
    expect(r.files["/a"].peak).toBeGreaterThan(r.files["/a"].rate);
  });
});

describe("changedSince", () => {
  const e = (name: string, size: number, modified: number | null, isDir = false) => ({ name, size, modified, isDir });
  it("picks recently touched files when there is no earlier listing", () => {
    expect(changedSince("/d", [e("new.mp4", 5, 99_000), e("old.mp4", 5, 1_000), e("dir", 0, 99_500, true)], null, 15_000, 100_000)).toEqual(["/d/new.mp4"]);
  });
  it("picks only files that differ from the earlier listing", () => {
    const before = new Map([["a", { size: 1, modified: 1 }], ["b", { size: 2, modified: 1 }]]);
    expect(changedSince("/d", [e("a", 1, 1), e("b", 9, 2), e("c", 3, 1)], before, 15_000, 100_000)).toEqual(["/d/b", "/d/c"]);
  });
});

describe("settings by performance preset", () => {
  const s = (o: Record<string, unknown>) => ({ advanced: o }) as never;
  it("interval follows the preset on auto", () => {
    expect(intervalFor(s({ "live.interval": "auto" }), "full")).toBe(500);
    expect(intervalFor(s({ "live.interval": "auto" }), "balanced")).toBe(1000);
    expect(intervalFor(s({ "live.interval": "auto" }), "saver")).toBe(3000);
  });
  it("a chosen interval wins over the preset", () => expect(intervalFor(s({ "live.interval": "250" }), "saver")).toBe(250));
  it("saver and balanced cap how many files", () => {
    expect(maxFollowed(s({ "live.maxFiles": 40 }), "saver")).toBe(4);
    expect(maxFollowed(s({ "live.maxFiles": 40 }), "balanced")).toBe(12);
    expect(maxFollowed(s({ "live.maxFiles": 40 }), "full")).toBe(40);
  });
});

describe("liveKey and liveSummary", () => {
  const mk = (path: string, growing: boolean, size: number): LiveFile => ({ path, since: 0, startSize: 0, size, modified: 1, lastChange: 4000, rate: 0, peak: 0, growing, samples: [] });
  it("lists only growing selected files", () => {
    const files = { "/a": mk("/a", true, 5), "/b": mk("/b", false, 6), "/c": mk("/c", true, 7) };
    expect(liveKey(files, ["/a", "/b"])).toBe("/a:5:1|");
  });
  it("summarises", () => {
    const r = liveSummary(mk("/a", true, 10000), 5000);
    expect(r.added).toBe(10000);
    expect(r.average).toBe(2000);
    expect(r.sinceChange).toBe(1);
  });
});
