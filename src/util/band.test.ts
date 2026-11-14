import { describe, expect, it } from "vitest";
import { touchedIndexes, type BandGeometry } from "./band";

const list: BandGeometry = { count: 100, details: true, rowHeight: 34, headerHeight: 28, rowsWidth: 500, columnsPerRow: 1, tileW: 0, tileH: 0 };
const grid: BandGeometry = { count: 10, details: false, rowHeight: 100, headerHeight: 0, rowsWidth: 0, columnsPerRow: 3, tileW: 90, tileH: 96 };

describe("selection box", () => {
  it("touches the rows it crosses in a list, even far down", () => {
    expect(touchedIndexes(list, 600, 28 + 34 * 2 + 5, 700, 28 + 34 * 4 + 5)).toEqual([]); // right of the rows: nothing
    expect(touchedIndexes(list, 100, 28 + 34 * 2 + 5, 700, 28 + 34 * 4 + 5)).toEqual([2, 3, 4]);
    expect(touchedIndexes(list, 100, 28 + 34 * 90, 200, 28 + 34 * 150)).toEqual(Array.from({ length: 10 }, (_, i) => 90 + i));
  });
  it("ignores the header", () => {
    expect(touchedIndexes(list, 100, 0, 200, 10)).toEqual([]);
  });
  it("touches tiles by their boxes, not the gaps between", () => {
    // first tile is at x 8..98, y 8..104; the next row starts at y 108
    expect(touchedIndexes(grid, 20, 20, 30, 30)).toEqual([0]);
    expect(touchedIndexes(grid, 20, 20, 120, 30)).toEqual([0, 1]);
    expect(touchedIndexes(grid, 20, 105, 30, 107)).toEqual([]); // in the gap
    expect(touchedIndexes(grid, 20, 20, 120, 150)).toEqual([0, 1, 3, 4]);
  });
  it("stops at the last item", () => {
    expect(touchedIndexes(grid, 0, 0, 1000, 1000)).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
  });
});
