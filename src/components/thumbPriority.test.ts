import { describe, expect, it } from "vitest";
import { priorityParallelism } from "./Thumb";

describe("thumbnail priority", () => {
  it("makes more at once as the priority rises", () => {
    const n = [1, 2, 3, 4, 5].map((p) => priorityParallelism(p, 4));
    expect(n).toEqual([1, 3, 5, 7, 8]);
    expect([1, 2, 3, 4, 5].map((p) => priorityParallelism(p, 1))).toEqual([1, 1, 2, 4, 8]);
  });
});
