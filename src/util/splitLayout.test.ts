import { describe, expect, it } from "vitest";
import { drag, evenOut, insertPane, layout, leaves, minShare, parseTree, presetsFor, removePane, setShares, sharesOf, zoneAt, type Branch, type Node } from "./splitLayout";

const sum = (a: number[]) => a.reduce((x, y) => x + y, 0);
const row = (kids: Node[], shares?: number[]): Branch => ({ dir: "row", kids, shares });
const col = (kids: Node[], shares?: number[]): Branch => ({ dir: "col", kids, shares });

describe("presets", () => {
  it("has layouts for 2 to 8 panes, each using every pane once", () => {
    for (let n = 2; n <= 8; n++) {
      const list = presetsFor(n);
      expect(list.length).toBeGreaterThanOrEqual(2);
      for (const p of list) {
        const slots = Array.from({ length: n }, (_, i) => i);
        expect(leaves(p.build(slots)).sort()).toEqual(slots);
      }
      expect(new Set(list.map((p) => p.id)).size).toBe(list.length);
    }
    expect(presetsFor(1)).toEqual([]);
    expect(presetsFor(9)).toEqual([]);
  });
  it("puts the first pane in the large place", () => {
    const left = presetsFor(3).find((p) => p.id === "3-big-left")!.build([0, 5, 6]);
    expect(leaves(left)).toEqual([0, 5, 6]);
    const right = presetsFor(3).find((p) => p.id === "3-big-right")!.build([0, 5, 6]);
    expect(leaves(right)).toEqual([5, 6, 0]);
  });
  it("offers a grid of four, and none that look alike", () => {
    expect(presetsFor(4).some((p) => p.label === "Grid, 2 across")).toBe(true);
  });
});

describe("insertPane and removePane", () => {
  it("splits only the pane it is put beside", () => {
    const t = insertPane(row([0, 1], [0.7, 0.3]), 0, "right", 2) as Branch;
    expect(t.kids).toEqual([0, 2, 1]);
    expect(sharesOf(t).map((x) => +x.toFixed(2))).toEqual([0.35, 0.35, 0.3]);
  });
  it("makes a new branch when the direction differs", () => {
    const t = insertPane(0, 0, "bottom", 1) as Branch;
    expect(t).toEqual({ dir: "col", kids: [0, 1] });
    const u = insertPane(row([0, 1]), 1, "top", 2) as Branch;
    expect(leaves(u)).toEqual([0, 2, 1]);
    expect((u.kids[1] as Branch).dir).toBe("col");
  });
  it("puts it before on the left and top", () => {
    expect(leaves(insertPane(row([0, 1]), 1, "left", 2))).toEqual([0, 2, 1]);
  });
  it("removes a pane and gives its room to the others", () => {
    const t = removePane(row([0, 1, 2], [0.5, 0.25, 0.25]), 1) as Branch;
    expect(t.kids).toEqual([0, 2]);
    expect(sum(sharesOf(t))).toBeCloseTo(1);
    expect(sharesOf(t)[0]).toBeCloseTo(2 / 3);
  });
  it("dissolves a branch left with one child, and folds same-way branches", () => {
    expect(removePane(row([0, 1]), 1)).toBe(0);
    const t = removePane(col([row([0, 1]), 2]), 2);
    expect(t).toEqual({ dir: "row", kids: [0, 1], shares: [0.5, 0.5] });
    const u = removePane(row([0, col([1, row([2, 3])])]), 1) as Branch;
    expect(leaves(u)).toEqual([0, 2, 3]);
  });
  it("can build up to eight panes", () => {
    let t: Node = 0;
    for (let s = 1; s < 8; s++) t = insertPane(t, s - 1, s % 2 ? "right" : "bottom", s);
    expect(leaves(t).sort()).toEqual([0, 1, 2, 3, 4, 5, 6, 7]);
  });
});

describe("shares", () => {
  it("sets and evens out shares by path", () => {
    const t = row([0, col([1, 2])]);
    const a = setShares(t, "1", [0.8, 0.2]) as Branch;
    expect(sharesOf(a.kids[1] as Branch)).toEqual([0.8, 0.2]);
    expect(sharesOf(evenOut(a) as Branch)).toEqual([0.5, 0.5]);
    expect(sharesOf((evenOut(a) as Branch).kids[1] as Branch)).toEqual([0.5, 0.5]);
  });
  it("ignores shares that do not fit", () => {
    expect(sharesOf(row([0, 1], [1]))).toEqual([0.5, 0.5]);
    expect(sharesOf(row([0, 1], [0, 1]))).toEqual([0.5, 0.5]);
  });
});

describe("layout", () => {
  it("places panes and handles with the gaps", () => {
    const { panes, handles } = layout(row([0, col([1, 2])], [0.5, 0.5]), { x: 0, y: 0, w: 1006, h: 500 }, 6);
    expect(panes[0]).toEqual({ x: 0, y: 0, w: 500, h: 500 });
    expect(panes[1]).toEqual({ x: 506, y: 0, w: 500, h: 247 });
    expect(panes[2].y).toBe(253);
    expect(handles.map((h) => [h.path, h.index, h.dir])).toEqual([["", 0, "row"], ["1", 0, "col"]]);
    expect(handles[0].room).toBe(1000);
  });
});

describe("drag", () => {
  it("moves share between neighbours and keeps the total", () => {
    const out = drag([0.5, 0.3, 0.2], 0, 0.1, 0.1);
    expect(out[0]).toBeCloseTo(0.6);
    expect(out[1]).toBeCloseTo(0.2);
    expect(out[2]).toBe(0.2);
  });
  it("stops at the minimum both ways", () => {
    expect(drag([0.5, 0.5], 0, 5, 0.15)[1]).toBeCloseTo(0.15);
    expect(drag([0.5, 0.5], 0, -5, 0.15)[0]).toBeCloseTo(0.15);
  });
  it("does not push up a pane that is already below the minimum", () => {
    const out = drag([0.1, 0.9], 0, -1, 0.15);
    expect(out[0]).toBeCloseTo(0.1);
  });
  it("ignores a divider that is not there", () => {
    expect(drag([0.5, 0.5], 3, 0.1, 0.1)).toEqual([0.5, 0.5]);
  });
});

describe("minShare", () => {
  it("uses the percent, the pixel floor, and never more than the panes can share", () => {
    expect(minShare(15, 2, 2000)).toBeCloseTo(0.15);
    expect(minShare(10, 2, 400)).toBeCloseTo(0.3);
    expect(minShare(40, 8, 1000)).toBeLessThan(1 / 8);
  });
});

describe("zoneAt", () => {
  const rects = { 0: { left: 0, top: 0, width: 400, height: 200 }, 1: { left: 400, top: 0, width: 400, height: 200 } };
  const can = { split: () => true, merge: () => true };
  it("merges in the middle and splits by side elsewhere", () => {
    expect(zoneAt(200, 100, rects, can)).toEqual({ target: 0, side: "merge" });
    expect(zoneAt(10, 100, rects, can)).toEqual({ target: 0, side: "left" });
    expect(zoneAt(790, 100, rects, can)).toEqual({ target: 1, side: "right" });
    expect(zoneAt(600, 5, rects, can)).toEqual({ target: 1, side: "top" });
    expect(zoneAt(600, 195, rects, can)).toEqual({ target: 1, side: "bottom" });
  });
  it("falls back to merging when no more panes may be made, and to nothing when neither", () => {
    expect(zoneAt(10, 100, rects, { split: () => false, merge: () => true })).toEqual({ target: 0, side: "merge" });
    expect(zoneAt(10, 100, rects, { split: () => false, merge: () => false })).toBeNull();
    expect(zoneAt(900, 100, rects, can)).toBeNull();
  });
});

describe("parseTree", () => {
  it("accepts a sound layout and refuses the rest", () => {
    const t = row([0, col([1, 2])]);
    expect(parseTree(JSON.stringify(t))).toEqual(t);
    expect(parseTree("not json")).toBe(0);
    expect(parseTree(JSON.stringify(row([1, 2])))).toBe(0);
    expect(parseTree(JSON.stringify(row([0, 0])))).toBe(0);
    expect(parseTree(JSON.stringify(row([0, 9])))).toBe(0);
    expect(parseTree(JSON.stringify(row([0])))).toBe(0);
    expect(parseTree("")).toBe(0);
  });
});
