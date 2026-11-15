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
