// Pure helpers and on-demand renderer instrumentation for the App & System
// Telemetry / Metrics window.
//
// Nothing in this module registers observers or animation-frame timers unless
// `startFrameMonitor` is explicitly started by an open Metrics view.

import type { MetricsSnapshot, PrivilegedMetrics } from "../api";

export interface FrameStats {
  fps: number;
  frameAvgMs: number;
  frameP95Ms: number;
  frameMaxMs: number;
  jankCount: number;
  severeJankCount: number;
  longTaskCount: number;
  longTaskMaxMs: number;
  domNodes: number;
  jsHeapUsedBytes: number | null;
  jsHeapTotalBytes: number | null;
  jsHeapLimitBytes: number | null;
  sampleWindowMs: number;
}

export interface MetricsRecord {
  snapshot: MetricsSnapshot;
  render: FrameStats;
  privileged: PrivilegedMetrics | null;
}

export function percentile95(values: readonly number[]): number {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  const idx = Math.min(sorted.length - 1, Math.max(0, Math.ceil(sorted.length * 0.95) - 1));
  return sorted[idx];
}

export function computeFrameStats(
  frameDeltasMs: readonly number[],
  longTaskDurationsMs: readonly number[],
  domNodes: number,
  heap?: { usedJSHeapSize?: number; totalJSHeapSize?: number; jsHeapSizeLimit?: number } | null,
): FrameStats {
  const valid = frameDeltasMs.filter((d) => Number.isFinite(d) && d > 0 && d < 2000);
  const sampleWindowMs = valid.reduce((a, b) => a + b, 0);
  const frameAvgMs = valid.length > 0 ? sampleWindowMs / valid.length : 0;
  const fps = frameAvgMs > 0 ? Math.min(240, 1000 / frameAvgMs) : 0;
  const frameP95Ms = percentile95(valid);
  const frameMaxMs = valid.reduce((m, d) => Math.max(m, d), 0);
  const jankCount = valid.filter((d) => d > 16.7).length;
  const severeJankCount = valid.filter((d) => d > 50).length;
  const longTaskMaxMs = longTaskDurationsMs.reduce((m, d) => Math.max(m, d), 0);

  return {
    fps,
    frameAvgMs,
    frameP95Ms,
    frameMaxMs,
    jankCount,
    severeJankCount,
    longTaskCount: longTaskDurationsMs.length,
    longTaskMaxMs,
    domNodes,
    jsHeapUsedBytes: typeof heap?.usedJSHeapSize === "number" ? heap.usedJSHeapSize : null,
    jsHeapTotalBytes: typeof heap?.totalJSHeapSize === "number" ? heap.totalJSHeapSize : null,
    jsHeapLimitBytes: typeof heap?.jsHeapSizeLimit === "number" ? heap.jsHeapSizeLimit : null,
    sampleWindowMs,
  };
}

/**
 * Starts an on-demand `requestAnimationFrame` + `PerformanceObserver("longtask")`
 * sampler that computes FPS, frame times, jank counts, DOM nodes, and JS heap.
 * Returns a stop function that detaches all listeners cleanly.
 */
export function startFrameMonitor(onRead: (stats: FrameStats) => void): () => void {
  if (typeof window === "undefined" || typeof requestAnimationFrame === "undefined") {
    return () => {};
  }

  let stopped = false;
  let rafId = 0;
  let lastTs = performance.now();
  const frameDeltas: number[] = [];
  const longTasks: number[] = [];

  let observer: PerformanceObserver | null = null;
  if (typeof PerformanceObserver !== "undefined") {
    try {
      const supported = PerformanceObserver.supportedEntryTypes ?? [];
      if (supported.includes("longtask")) {
        observer = new PerformanceObserver((list) => {
          for (const entry of list.getEntries()) {
            longTasks.push(entry.duration);
            if (longTasks.length > 120) longTasks.shift();
          }
        });
        observer.observe({ entryTypes: ["longtask"] });
      }
    } catch {
      observer = null;
    }
  }

  let lastEmit = 0;
  const loop = (now: number) => {
    if (stopped) return;
    const dt = now - lastTs;
    lastTs = now;
    if (dt > 0 && dt < 2000) {
      frameDeltas.push(dt);
      if (frameDeltas.length > 90) frameDeltas.shift();
    }
    if (now - lastEmit >= 120) {
      lastEmit = now;
      const domNodes = typeof document !== "undefined" ? document.getElementsByTagName("*").length : 0;
      const mem = (performance as Performance & {
        memory?: { usedJSHeapSize?: number; totalJSHeapSize?: number; jsHeapSizeLimit?: number };
      }).memory;
      onRead(computeFrameStats(frameDeltas, longTasks, domNodes, mem ?? null));
    }
    rafId = requestAnimationFrame(loop);
  };
  rafId = requestAnimationFrame(loop);

  return () => {
    stopped = true;
    cancelAnimationFrame(rafId);
    try {
      observer?.disconnect();
    } catch {
      /* already stopped */
    }
  };
}

export function clampRefreshMs(ms: number): number {
  if (!Number.isFinite(ms)) return 100;
  return Math.max(10, Math.min(5000, Math.round(ms)));
}

export function formatMs(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return "0.00 ms";
  if (ms < 1) return `${ms.toFixed(2)} ms`;
  if (ms < 100) return `${ms.toFixed(1)} ms`;
  return `${Math.round(ms)} ms`;
}

export function formatDurationSecs(secs: number): string {
  if (!Number.isFinite(secs) || secs <= 0) return "0s";
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = Math.floor(secs % 60);
  if (d > 0) return `${d}d ${h}h ${m}m`;
  if (h > 0) return `${h}h ${m}m ${s}s`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}

const CSV_HEADER = [
  "sampledAtMs",
  "fps",
  "frameAvgMs",
  "frameP95Ms",
  "domNodes",
  "cpuPercent",
  "appCpuPercent",
  "appMemoryBytes",
  "appThreads",
  "appHandles",
  "appDiskReadPerSec",
  "appDiskWritePerSec",
  "netRxPerSec",
  "netTxPerSec",
  "remoteReadPerSec",
  "remoteWritePerSec",
  "expectedListDirMs",
  "expectedSearchMs",
  "expectedOpenItemMs",
  "expectedThumbMs",
].join(",");

function expectedFor(snap: MetricsSnapshot, op: string): number {
  return snap.telemetry.operations.find((o) => o.op === op)?.expectedMs ?? 0;
}

export function snapshotToCsvHeader(): string {
  return CSV_HEADER;
}

export function snapshotToCsvRow(snap: MetricsSnapshot, render: FrameStats): string {
  return [
    snap.sampledAtMs,
    render.fps.toFixed(1),
    render.frameAvgMs.toFixed(2),
    render.frameP95Ms.toFixed(2),
    render.domNodes,
    snap.cpuPercent.toFixed(2),
    snap.appCpuPercent.toFixed(2),
    snap.appMemoryBytes,
    snap.appThreads,
    snap.appHandles,
    snap.appDiskReadPerSec.toFixed(0),
    snap.appDiskWritePerSec.toFixed(0),
    snap.netRxPerSec.toFixed(0),
    snap.netTxPerSec.toFixed(0),
    snap.remoteReadPerSec.toFixed(0),
    snap.remoteWritePerSec.toFixed(0),
    expectedFor(snap, "list_dir").toFixed(2),
    expectedFor(snap, "search").toFixed(2),
    expectedFor(snap, "open_item").toFixed(2),
    expectedFor(snap, "thumbnail").toFixed(2),
  ].join(",");
}

export function serializeSnapshotJson(record: MetricsRecord): string {
  return JSON.stringify(record, null, 2);
}

export function serializeJsonlLine(record: MetricsRecord): string {
  return `${JSON.stringify(record)}\n`;
}
