// A delete, a compress or an unpack gets the same speed graph and time left as a transfer.

import { beforeEach, describe, expect, it } from "vitest";
import type { DeleteProgress, JobStats } from "./api";
import { useStore } from "./store";

const stats = (over: Partial<JobStats> = {}): JobStats => ({
  itemsPerSec: 500, bytesPerSec: 0, etaSecs: 20, etaConfidence: 0.4, fraction: 0.3,
  avgItemsPerSec: 450, avgBytesPerSec: 0, peakItemsPerSec: 600, peakBytesPerSec: 0, typicalSecs: 30, samples: 3, ...over,
});
const progress = (over: Partial<DeleteProgress>): DeleteProgress => ({
  id: 5, phase: "running", permanent: true, selected: 1, itemsTotal: 10_000, itemsDone: 3000, current: "", errors: [], errorCount: 0,
  elapsedSecs: 1, firstName: "big", action: "delete", result: "", bytesDone: 0, bytesTotal: 0, skipped: 0, stats: stats(), ...over,
});

beforeEach(() => {
  useStore.setState({
    info: { os: "linux" } as never,
    settings: { appearance: {}, general: {}, transfer: {}, advanced: {} } as never,
    deletes: {}, series: {}, shown: {}, notices: [],
  });
});

describe("the graph of a job that is not a copy", () => {
  it("collects a sample per update and follows the measured fraction", () => {
    const s = useStore.getState();
    s.onDeleteProgress(progress({ elapsedSecs: 1 }));
    s.onDeleteProgress(progress({ elapsedSecs: 1.02 })); // too soon after the last one: no new point
    s.onDeleteProgress(progress({ elapsedSecs: 2, stats: stats({ itemsPerSec: 800, fraction: 0.5 }) }));
    const { series, shown } = useStore.getState();
    expect(series[5].map((x) => x.t)).toEqual([1, 2]);
    expect(series[5][1].filesPerSec).toBe(800);
    expect(shown[5]).toBeCloseTo(0.5);
  });

  it("takes no samples while still counting, and never moves the bar back", () => {
    const s = useStore.getState();
    s.onDeleteProgress(progress({ phase: "scanning", stats: null, elapsedSecs: 0.5 }));
    expect(useStore.getState().series[5]).toBeUndefined();
    s.onDeleteProgress(progress({ elapsedSecs: 1, stats: stats({ fraction: 0.6 }) }));
    s.onDeleteProgress(progress({ elapsedSecs: 2, stats: stats({ fraction: 0.4 }) }));
    expect(useStore.getState().shown[5]).toBeCloseTo(0.6);
  });

  it("works for archive jobs too", () => {
    useStore.getState().onDeleteProgress(progress({ id: 9, action: "extract", elapsedSecs: 3 }));
    expect(useStore.getState().series[9]).toHaveLength(1);
  });
});

describe("the graph of a transfer that was over at once", () => {
  it("an instant move still gets a graph, from nothing to its speed", () => {
    const p = {
      id: 40, op: "move", phase: "done", strategy: "rename", destDir: "/b", filesTotal: 1, filesDone: 1, bytesTotal: 0, bytesDone: 0, skipped: 0,
      bytesPerSec: 0, filesPerSec: 0, etaSecs: null, etaConfidence: 1, display: "files", overheadShare: 0, current: "", errors: [], errorCount: 0,
      elapsedSecs: 0.004, fraction: 1, avgBytesPerSec: 0, avgFilesPerSec: 250, peakBytesPerSec: 0, peakFilesPerSec: 250, route: null, plan: null, prediction: null,
    } as never;
    useStore.getState().onProgress(p);
    const s = useStore.getState().series[40];
    expect(s).toHaveLength(2);
    expect(s[1].filesPerSec).toBe(250);
    expect(s[1].t).toBeGreaterThan(0);
  });
});
