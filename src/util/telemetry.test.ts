import { describe, expect, it } from "vitest";
import {
  clampRefreshMs,
  computeFrameStats,
  formatDurationSecs,
  formatMs,
  percentile95,
  snapshotToCsvHeader,
  snapshotToCsvRow,
} from "./telemetry";
import type { MetricsSnapshot } from "../api";

describe("telemetry helpers", () => {
  it("computes p95 and jank counts from frame deltas", () => {
    const frames = [...Array(19).fill(8.0), 40.0, 62.0];
    const stats = computeFrameStats(frames, [55.5, 81.2], 420, {
      usedJSHeapSize: 12_000_000,
      totalJSHeapSize: 20_000_000,
      jsHeapSizeLimit: 2_000_000_000,
    });
    expect(stats.domNodes).toBe(420);
    expect(stats.jankCount).toBe(2);
    expect(stats.severeJankCount).toBe(1);
    expect(stats.longTaskCount).toBe(2);
    expect(stats.longTaskMaxMs).toBeCloseTo(81.2);
    expect(stats.frameMaxMs).toBeCloseTo(62.0);
    expect(stats.frameP95Ms).toBeGreaterThanOrEqual(40.0);
    expect(stats.fps).toBeGreaterThan(60);
    expect(stats.jsHeapUsedBytes).toBe(12_000_000);
  });

  it("clamps refresh intervals cleanly between 10ms and 5000ms", () => {
    expect(clampRefreshMs(1)).toBe(10);
    expect(clampRefreshMs(10)).toBe(10);
    expect(clampRefreshMs(100)).toBe(100);
    expect(clampRefreshMs(99999)).toBe(5000);
    expect(clampRefreshMs(Number.NaN)).toBe(100);
  });

  it("formats durations and millisecond latencies predictably", () => {
    expect(percentile95([10, 20, 30, 40, 50])).toBe(50);
    expect(formatMs(0.35)).toBe("0.35 ms");
    expect(formatMs(12.44)).toBe("12.4 ms");
    expect(formatMs(254.9)).toBe("255 ms");
    expect(formatDurationSecs(3665)).toBe("1h 1m 5s");
  });

  it("serializes CSV header and snapshot row with aligned columns", () => {
    const fakeSnap: MetricsSnapshot = {
      sampledAtMs: 1700000000000,
      isElevated: false,
      canElevate: true,
      osName: "Windows 11",
      kernelVersion: "10.0",
      uptimeSecs: 1200,
      loadAvg: [0.5, 0.4, 0.3],
      cpuPercent: 18.5,
      perCoreCpu: [15, 22],
      cpuFreqMhz: 3800,
      physicalCores: 2,
      logicalCores: 4,
      memoryTotalBytes: 16_000_000_000,
      memoryUsedBytes: 8_000_000_000,
      memoryAvailableBytes: 8_000_000_000,
      swapTotalBytes: 2_000_000_000,
      swapUsedBytes: 100_000_000,
      appPid: 1234,
      appUptimeSecs: 90,
      appCpuPercent: 2.5,
      appMemoryBytes: 140_000_000,
      appVirtualBytes: 400_000_000,
      appThreads: 14,
      appHandles: 380,
      appDiskReadPerSec: 1024,
      appDiskWritePerSec: 512,
      appDiskReadTotal: 10000,
      appDiskWriteTotal: 5000,
      netRxPerSec: 4096,
      netTxPerSec: 2048,
      netTotalRx: 100000,
      netTotalTx: 50000,
      interfaces: [],
      remoteReadPerSec: 8192,
      remoteWritePerSec: 0,
      remoteReadTotal: 16384,
      remoteWriteTotal: 0,
      remotes: [],
      indexEntries: 5000,
      indexMemoryBytes: 1_500_000,
      indexState: "ready",
      listingsCached: 10,
      listingsMemoryBytes: 400_000,
      folderSizesCached: 5,
      folderSizesMemoryBytes: 10_000,
      activeTransfers: 1,
      activeDeletes: 0,
      activeAnalyses: 0,
      activeShares: 0,
      watchedDirs: 3,
      openTerminals: 1,
      thumbWorkersBusy: 1,
      thumbWorkersTotal: 4,
      slowWorkersBusy: 0,
      slowWorkersTotal: 2,
      disks: [],
      temperatures: [],
      telemetry: {
        operations: [
          {
            op: "list_dir",
            count: 3,
            errors: 0,
            lastMs: 4.5,
            avgMs: 4.1,
            p95Ms: 5.2,
            maxMs: 5.2,
            expectedMs: 4.8,
            samplesMs: [3.8, 4.1, 4.5],
          },
        ],
        recent: [],
      },
    };
    const render = computeFrameStats([8, 8], [], 210);
    const cols = snapshotToCsvHeader().split(",");
    const vals = snapshotToCsvRow(fakeSnap, render).split(",");
    expect(vals.length).toBe(cols.length);
    expect(vals[cols.indexOf("expectedListDirMs")]).toBe("4.80");
  });
});
