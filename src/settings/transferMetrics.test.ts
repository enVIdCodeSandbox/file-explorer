import { describe, expect, it } from "vitest";
import { ADVANCED } from "./advanced";
import { TRANSFER_METRICS, metricOn } from "./transferMetrics";

describe("copy window lines", () => {
  it("each has a setting with the same default", () => {
    for (const m of TRANSFER_METRICS) {
      const def = ADVANCED.find((a) => a.key === `transfer.show.${m.id}`);
      expect(def, m.id).toBeTruthy();
      expect(def?.default).toBe(m.on);
    }
  });
  it("the current ones, the file list and the method are on; the extras are off", () => {
    for (const id of ["name", "activeFiles", "method", "speed", "timeRemaining", "graph", "peakSpeed"]) expect(metricOn(null, id)).toBe(true);
    for (const id of ["finishAt", "startedAt", "confidence", "destFolder", "totalSize"]) expect(metricOn(null, id)).toBe(false);
    expect(TRANSFER_METRICS.filter((m) => !m.on).length).toBeGreaterThanOrEqual(5);
  });
  it("a saved choice wins", () => {
    expect(metricOn({ advanced: { "transfer.show.speed": false } }, "speed")).toBe(false);
    expect(metricOn({ advanced: { "transfer.show.finishAt": true } }, "finishAt")).toBe(true);
  });
});
