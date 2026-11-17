import { describe, expect, it } from "vitest";
import { barFor, sameBar } from "./taskbar";

describe("barFor", () => {
  it("shows nothing when nothing runs, and red when something just failed", () => {
    expect(barFor([])).toEqual({ status: "none", progress: 0 });
    expect(barFor([], true)).toEqual({ status: "error", progress: 100 });
  });
  it("fills up with the progress of one job", () => {
    expect(barFor([{ done: 25, total: 100 }])).toEqual({ status: "normal", progress: 25 });
    expect(barFor([{ done: 150, total: 100 }]).progress).toBe(100);
  });
  it("averages several jobs, ignoring those whose size is not known yet", () => {
    expect(barFor([{ done: 50, total: 100 }, { done: 0, total: 100 }, { done: 0, total: 0 }]).progress).toBe(25);
  });
  it("moves back and forth when no size is known", () => {
    expect(barFor([{ done: 0, total: 0 }])).toEqual({ status: "indeterminate", progress: 0 });
  });
  it("is yellow only when everything is paused", () => {
    expect(barFor([{ done: 10, total: 100, paused: true }])).toEqual({ status: "paused", progress: 10 });
    expect(barFor([{ done: 10, total: 100, paused: true }, { done: 10, total: 100 }]).status).toBe("normal");
  });
  it("compares bars", () => {
    expect(sameBar(null, { status: "none", progress: 0 })).toBe(false);
    expect(sameBar({ status: "normal", progress: 5 }, { status: "normal", progress: 5 })).toBe(true);
  });
});
