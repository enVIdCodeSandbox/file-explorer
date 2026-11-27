import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useRef, useState } from "react";
import { api, type MetricsSnapshot, type PrivilegedMetrics } from "../api";
import { adv } from "../settings/advanced";
import { paneCount as countPanes, useSplit } from "../split/state";
import { useStore } from "../store";
import { formatSize } from "../util/format";
import {
  clampRefreshMs,
  computeFrameStats,
  formatDurationSecs,
  formatMs,
  serializeJsonlLine,
  serializeSnapshotJson,
  snapshotToCsvHeader,
  snapshotToCsvRow,
  startFrameMonitor,
  type FrameStats,
} from "../util/telemetry";
import { Icon } from "./Icons";

type SectionTab = "all" | "rendering" | "processing" | "network" | "system" | "admin";

const HISTORY_LEN = 60;
const PRESET_INTERVALS = [10, 25, 50, 100, 250, 1000] as const;

const rate = (bytesPerSec: number) => `${formatSize(Math.max(0, bytesPerSec))}/s`;

function stampForFile(d = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}`;
}

function Sparkline({ values, max, label, color = "var(--accent)" }: { values: number[]; max?: number; label: string; color?: string }) {
  const w = 156;
  const h = 30;
  const padded = values.length < HISTORY_LEN ? [...Array(HISTORY_LEN - values.length).fill(0), ...values] : values.slice(-HISTORY_LEN);
  const top = Math.max(1, max ?? Math.max(...padded, 1));
  const pts = padded.map((v, i) => `${(i / (HISTORY_LEN - 1)) * w},${h - Math.min(1, Math.max(0, v) / top) * (h - 3) - 1.5}`).join(" ");
  return (
    <svg className="metrics-spark" width={w} height={h} viewBox={`0 0 ${w} ${h}`} role="img" aria-label={label}>
      <polyline points={pts} fill="none" stroke={color} strokeWidth="1.6" strokeLinejoin="round" />
    </svg>
  );
}

function opTitle(op: string): string {
  switch (op) {
    case "list_dir":
      return "Folder open (list_dir)";
    case "thumbnail":
      return "Thumbnail decode";
    case "search":
      return "Search query";
    case "open_item":
      return "Open item / launch";
    case "open_choices":
      return "Resolve app choices";
    case "installed_apps":
      return "Enumerate installed apps";
    default:
      return op;
  }
}

export function MetricsPanel({
  clientId,
  detached = false,
  autoStart = true,
  onClose,
}: {
  clientId: string;
  detached?: boolean;
  autoStart?: boolean;
  onClose?: () => void;
}) {
  const settings = useStore((s) => s.settings);
  const info = useStore((s) => s.info);
  const { updateSettings, showToast } = useStore.getState();

  const tabCount = useStore((s) => s.tabs.length);
  const paneCount = useSplit((s) => countPanes(s.tree));
  const activeNav = useStore((s) => s.tabs.find((t) => t.id === s.activeTab)?.location ?? "");

  const configuredRefreshMs = clampRefreshMs(settings ? adv<number>(settings, "metrics.refreshMs") : 100);
  const configuredLogDir = settings ? String(adv<string>(settings, "metrics.logDir") ?? "") : "";

  const [started, setStarted] = useState(autoStart);
  const [paused, setPaused] = useState(false);
  const [refreshInput, setRefreshInput] = useState(String(configuredRefreshMs));
  const [section, setSection] = useState<SectionTab>("all");

  const [snap, setSnap] = useState<MetricsSnapshot | null>(null);
  const [renderStats, setRenderStats] = useState<FrameStats>(() => computeFrameStats([], [], 0));
  const renderRef = useRef<FrameStats>(renderStats);

  const [cpuHist, setCpuHist] = useState<number[]>([]);
  const [fpsHist, setFpsHist] = useState<number[]>([]);
  const [memHist, setMemHist] = useState<number[]>([]);
  const [netRxHist, setNetRxHist] = useState<number[]>([]);
  const [netTxHist, setNetTxHist] = useState<number[]>([]);
  const [diskHist, setDiskHist] = useState<number[]>([]);

  const [logging, setLogging] = useState(false);
  const [logCount, setLogCount] = useState(0);
  const [lastSavedPath, setLastSavedPath] = useState<string | null>(null);
  const logFileRef = useRef<string>(`metrics-session-${stampForFile()}.jsonl`);
  const loggingRef = useRef(false);
  const logDirRef = useRef(configuredLogDir);
  const adminRef = useRef<PrivilegedMetrics | null>(null);

  const [adminData, setAdminData] = useState<PrivilegedMetrics | null>(null);
  const [adminBusy, setAdminBusy] = useState(false);
  const [adminError, setAdminError] = useState<string | null>(null);

  useEffect(() => {
    setRefreshInput(String(configuredRefreshMs));
  }, [configuredRefreshMs]);

  useEffect(() => {
    logDirRef.current = configuredLogDir;
  }, [configuredLogDir]);

  useEffect(() => {
    loggingRef.current = logging;
  }, [logging]);

  useEffect(() => {
    adminRef.current = adminData;
  }, [adminData]);

  // Turn backend + frame instrumentation ON only while active & unpaused.
  const activelySampling = started && !paused;

  useEffect(() => {
    if (!activelySampling) {
      void api.metricsSetActive(clientId, false).catch(() => {});
      return;
    }

    let cancelled = false;
    void api.metricsSetActive(clientId, true).catch(() => {});

    const stopFrames = startFrameMonitor((stats) => {
      if (cancelled) return;
      renderRef.current = stats;
      setRenderStats(stats);
    });

    let inFlight = false;
    let timer: number | null = null;
    let writeQueue = Promise.resolve();

    const tick = async () => {
      if (cancelled) return;
      if (inFlight) {
        timer = window.setTimeout(tick, configuredRefreshMs);
        return;
      }
      inFlight = true;
      try {
        const next = await api.metricsSnapshot();
        if (cancelled) return;
        setSnap(next);
        setCpuHist((h) => [...h.slice(-(HISTORY_LEN - 1)), next.cpuPercent]);
        setFpsHist((h) => [...h.slice(-(HISTORY_LEN - 1)), renderRef.current.fps]);
        setMemHist((h) => [...h.slice(-(HISTORY_LEN - 1)), next.appMemoryBytes]);
        setNetRxHist((h) => [...h.slice(-(HISTORY_LEN - 1)), next.netRxPerSec]);
        setNetTxHist((h) => [...h.slice(-(HISTORY_LEN - 1)), next.netTxPerSec]);
        setDiskHist((h) => [...h.slice(-(HISTORY_LEN - 1)), next.appDiskReadPerSec + next.appDiskWritePerSec]);

        if (loggingRef.current && logDirRef.current.trim()) {
          const line = serializeJsonlLine({
            snapshot: next,
            render: renderRef.current,
            privileged: adminRef.current,
          });
          const folder = logDirRef.current.trim();
          const fname = logFileRef.current;
          writeQueue = writeQueue
            .then(() => api.metricsSaveFile(folder, fname, line, true))
            .then((saved) => {
              if (!cancelled) {
                setLogCount((c) => c + 1);
                setLastSavedPath(saved);
              }
            })
            .catch((err) => {
              if (!cancelled) {
                setLogging(false);
                showToast(`Metrics log stopped: ${String(err)}`);
              }
            });
        }
      } catch {
        /* ignore transient sampling errors */
      } finally {
        inFlight = false;
        if (!cancelled) {
          timer = window.setTimeout(tick, configuredRefreshMs);
        }
      }
    };

    void tick();

    return () => {
      cancelled = true;
      if (timer != null) window.clearTimeout(timer);
      stopFrames();
      void api.metricsSetActive(clientId, false).catch(() => {});
    };
  }, [activelySampling, clientId, configuredRefreshMs, showToast]);

  const setRefreshMs = (raw: number) => {
    const clamped = clampRefreshMs(raw);
    setRefreshInput(String(clamped));
    if (settings) {
      updateSettings((s) => {
        s.advanced = { ...s.advanced, "metrics.refreshMs": clamped };
      });
    }
  };

  const setLogFolder = (folder: string) => {
    if (settings) {
      updateSettings((s) => {
        s.advanced = { ...s.advanced, "metrics.logDir": folder };
      });
    }
  };

  const chooseLogFolder = async () => {
    try {
      const picked = await openDialog({
        directory: true,
        multiple: false,
        defaultPath: configuredLogDir || info?.home || undefined,
        title: "Choose folder to save and log metrics",
      });
      if (typeof picked === "string" && picked.trim()) {
        setLogFolder(picked.trim());
      }
    } catch (err) {
      showToast(String(err));
    }
  };

  const ensureFolder = async (): Promise<string | null> => {
    if (configuredLogDir.trim()) return configuredLogDir.trim();
    const picked = await openDialog({
      directory: true,
      multiple: false,
      defaultPath: info?.home || undefined,
      title: "Choose folder to save metrics",
    }).catch(() => null);
    if (typeof picked === "string" && picked.trim()) {
      setLogFolder(picked.trim());
      return picked.trim();
    }
    return null;
  };

  const saveSnapshot = async (kind: "json" | "csv") => {
    if (!snap) {
      showToast("Start live metrics first so there is a snapshot to save.");
      return;
    }
    const folder = await ensureFolder();
    if (!folder) return;
    const filename = `metrics-${stampForFile()}.${kind}`;
    const body =
      kind === "json"
        ? serializeSnapshotJson({ snapshot: snap, render: renderStats, privileged: adminData })
        : `${snapshotToCsvHeader()}\n${snapshotToCsvRow(snap, renderStats)}\n`;
    try {
      const saved = await api.metricsSaveFile(folder, filename, body, false);
      setLastSavedPath(saved);
      showToast(`Saved ${filename}`);
    } catch (err) {
      showToast(String(err));
    }
  };

  const toggleLiveLog = async () => {
    if (logging) {
      setLogging(false);
      return;
    }
    const folder = await ensureFolder();
    if (!folder) return;
    logFileRef.current = `metrics-session-${stampForFile()}.jsonl`;
    setLogCount(0);
    setLogging(true);
  };

  const resetCounters = async () => {
    await api.metricsReset().catch(() => {});
    setCpuHist([]);
    setFpsHist([]);
    setMemHist([]);
    setNetRxHist([]);
    setNetTxHist([]);
    setDiskHist([]);
    showToast("Reset telemetry counters");
  };

  const collectAdmin = async () => {
    setAdminBusy(true);
    setAdminError(null);
    try {
      const res = await api.metricsCollectAdmin();
      setAdminData(res);
    } catch (err) {
      setAdminError(String(err));
    } finally {
      setAdminBusy(false);
    }
  };

  const popOut = () => {
    void api
      .openMetricsWindow()
      .then(() => {
        if (onClose) onClose();
      })
      .catch((err) => showToast(String(err)));
  };

  const expectedByOp = useMemo(() => {
    const map = new Map<string, number>();
    for (const o of snap?.telemetry.operations ?? []) map.set(o.op, o.expectedMs);
    return map;
  }, [snap]);

  if (!started) {
    return (
      <div className="metrics-dormant">
        <div className="metrics-dormant-icon">
          <Icon name="chart" size={28} />
        </div>
        <div>
          <h4>Live Metrics & Telemetry is paused</h4>
          <p className="muted">
            To avoid using CPU, memory, or I/O in the background, frame timing, backend operation tracing, network counters, and hardware probes only run when you open them.
          </p>
        </div>
        <div className="button-row">
          <button className="primary" onClick={() => setStarted(true)}>
            <Icon name="play" size={14} /> Start live metrics ({configuredRefreshMs} ms)
          </button>
          {!detached && (
            <button className="secondary" onClick={popOut}>
              <Icon name="popout" size={14} /> Pop out window
            </button>
          )}
        </div>
      </div>
    );
  }

  const showSection = (s: Exclude<SectionTab, "all">) => section === "all" || section === s;
  const isWin = info?.os === "windows";
  const elevateVerb = isWin ? "administrator" : "sudo / root";

  return (
    <div className="metrics-root" data-detached={detached || undefined}>
      {/* Top command & sampling toolbar */}
      <div className="metrics-toolbar">
        <div className="metrics-title-group">
          <Icon name="chart" size={16} />
          <strong>App & System Metrics</strong>
          <span className="metrics-badge" data-live={activelySampling || undefined}>
            {paused ? "Paused" : `Live · ${configuredRefreshMs} ms`}
          </span>
        </div>

        <div className="metrics-controls">
          <label className="metrics-rate-control" title="Update interval in milliseconds (10 ms to 5000 ms)">
            <span className="muted">Rate</span>
            <input
              type="number"
              min={10}
              max={5000}
              step={10}
              value={refreshInput}
              onChange={(e) => setRefreshInput(e.target.value)}
              onBlur={() => setRefreshMs(Number(refreshInput))}
              onKeyDown={(e) => {
                if (e.key === "Enter") setRefreshMs(Number(refreshInput));
              }}
            />
            <span className="muted">ms</span>
          </label>

          <div className="metrics-presets" role="group" aria-label="Interval presets">
            {PRESET_INTERVALS.map((ms) => (
              <button
                key={ms}
                type="button"
                className="metrics-chip"
                data-on={configuredRefreshMs === ms || undefined}
                onClick={() => setRefreshMs(ms)}
              >
                {ms}ms
              </button>
            ))}
          </div>

          <button type="button" className="secondary" onClick={() => setPaused((p) => !p)}>
            <Icon name={paused ? "play" : "pause"} size={13} /> {paused ? "Resume" : "Pause"}
          </button>

          <button type="button" className="secondary" onClick={() => void resetCounters()}>
            <Icon name="refresh" size={13} /> Reset
          </button>

          {!detached && (
            <button type="button" className="secondary" onClick={popOut} title="Open Metrics in its own window">
              <Icon name="popout" size={13} /> Pop out
            </button>
          )}
        </div>
      </div>

      {/* Logging & Save bar */}
      <div className="metrics-log-bar">
        <span className="muted metrics-log-label">Log / save folder:</span>
        <input
          type="text"
          className="metrics-log-path"
          value={configuredLogDir}
          placeholder="Choose a folder where snapshots and logs will be saved…"
          onChange={(e) => setLogFolder(e.target.value)}
        />
        <button type="button" className="secondary" onClick={() => void chooseLogFolder()}>
          <Icon name="folderOpen" size={13} /> Choose folder…
        </button>
        <button type="button" className="secondary" onClick={() => void saveSnapshot("json")} disabled={!snap}>
          Save JSON
        </button>
        <button type="button" className="secondary" onClick={() => void saveSnapshot("csv")} disabled={!snap}>
          Save CSV
        </button>
        <button
          type="button"
          className={logging ? "primary" : "secondary"}
          onClick={() => void toggleLiveLog()}
          title="Continuously write one JSONL sample per tick into the chosen folder"
        >
          {logging ? `Stop logging (${logCount})` : "Record live log (.jsonl)"}
        </button>
        {lastSavedPath && (
          <span className="muted metrics-saved-hint" title={lastSavedPath}>
            Saved: {lastSavedPath.split(/[\\/]/).pop()}
          </span>
        )}
      </div>

      {/* Category tabs */}
      <div className="metrics-tabs" role="tablist" aria-label="Metrics categories">
        {(
          [
            ["all", "All metrics"],
            ["rendering", "Rendering & UI"],
            ["processing", "App latencies & queue"],
            ["network", "Network & caches"],
            ["system", "System & hardware"],
            ["admin", isWin ? "Kernel & admin" : "Kernel & root"],
          ] as const
        ).map(([id, label]) => (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={section === id}
            className="metrics-tab"
            data-on={section === id || undefined}
            onClick={() => setSection(id)}
          >
            {label}
          </button>
        ))}
      </div>

      {/* Body */}
      <div className="metrics-scroll">
        {!snap ? (
          <p className="muted" style={{ padding: 12 }}>
            Sampling first telemetry frame…
          </p>
        ) : (
          <>
            {/* Key gauges + Expected Latencies */}
            {showSection("rendering") && (
              <section className="metrics-section">
                <div className="metrics-kpis">
                  <div className="metrics-kpi">
                    <span className="muted">UI Render Rate</span>
                    <strong>{renderStats.fps.toFixed(0)} FPS</strong>
                    <span className="muted">
                      avg {formatMs(renderStats.frameAvgMs)} · p95 {formatMs(renderStats.frameP95Ms)}
                    </span>
                    <Sparkline values={fpsHist} max={Math.max(120, ...fpsHist)} label="FPS history" color="#3fd08b" />
                  </div>

                  <div className="metrics-kpi">
                    <span className="muted">System / App CPU</span>
                    <strong>
                      {snap.cpuPercent.toFixed(1)}% <span className="muted">/ app {snap.appCpuPercent.toFixed(1)}%</span>
                    </strong>
                    <span className="muted">
                      {snap.physicalCores}P/{snap.logicalCores}L cores · {snap.cpuFreqMhz} MHz
                    </span>
                    <Sparkline values={cpuHist} max={100} label="CPU history" color="var(--accent)" />
                  </div>

                  <div className="metrics-kpi">
                    <span className="muted">App Memory (RSS)</span>
                    <strong>{formatSize(snap.appMemoryBytes)}</strong>
                    <span className="muted">
                      sys {formatSize(snap.memoryUsedBytes)} / {formatSize(snap.memoryTotalBytes)}
                    </span>
                    <Sparkline values={memHist} max={Math.max(1, ...memHist) * 1.25} label="App RAM history" color="#5aa9ff" />
                  </div>

                  <div className="metrics-kpi">
                    <span className="muted">Network Traffic</span>
                    <strong>↓ {rate(snap.netRxPerSec)}</strong>
                    <span className="muted">
                      ↑ {rate(snap.netTxPerSec)} · remote {rate(snap.remoteReadPerSec + snap.remoteWritePerSec)}
                    </span>
                    <Sparkline
                      values={netRxHist}
                      max={Math.max(1024, ...netRxHist, ...netTxHist)}
                      label="Network RX history"
                      color="#c678dd"
                    />
                  </div>

                  <div className="metrics-kpi">
                    <span className="muted">App Disk I/O</span>
                    <strong>{rate(snap.appDiskReadPerSec + snap.appDiskWritePerSec)}</strong>
                    <span className="muted">
                      R {rate(snap.appDiskReadPerSec)} · W {rate(snap.appDiskWritePerSec)}
                    </span>
                    <Sparkline values={diskHist} max={Math.max(1024, ...diskHist)} label="Disk I/O history" color="#e5c07b" />
                  </div>

                  <div className="metrics-kpi">
                    <span className="muted">Expected Latencies</span>
                    <strong>
                      {expectedByOp.has("list_dir") ? formatMs(expectedByOp.get("list_dir")!) : "—"}
                    </strong>
                    <span className="muted">
                      open: {expectedByOp.has("open_item") ? formatMs(expectedByOp.get("open_item")!) : "—"} · search:{" "}
                      {expectedByOp.has("search") ? formatMs(expectedByOp.get("search")!) : "—"}
                    </span>
                    <span className="muted">
                      thumb: {expectedByOp.has("thumbnail") ? formatMs(expectedByOp.get("thumbnail")!) : "—"}
                    </span>
                  </div>
                </div>

                <div className="metrics-card">
                  <h4>Rendering & UI Pipeline</h4>
                  <div className="metrics-grid-kv">
                    <div>
                      <span className="muted">Frame avg / p95 / max</span>
                      <strong>
                        {formatMs(renderStats.frameAvgMs)} / {formatMs(renderStats.frameP95Ms)} / {formatMs(renderStats.frameMaxMs)}
                      </strong>
                    </div>
                    <div>
                      <span className="muted">Frame jank (&gt;16.7ms / &gt;50ms)</span>
                      <strong>
                        {renderStats.jankCount} / {renderStats.severeJankCount}
                      </strong>
                    </div>
                    <div>
                      <span className="muted">Long tasks (main thread)</span>
                      <strong>
                        {renderStats.longTaskCount} (max {formatMs(renderStats.longTaskMaxMs)})
                      </strong>
                    </div>
                    <div>
                      <span className="muted">Live DOM nodes</span>
                      <strong>{renderStats.domNodes.toLocaleString()}</strong>
                    </div>
                    <div>
                      <span className="muted">JS heap used / total</span>
                      <strong>
                        {renderStats.jsHeapUsedBytes != null
                          ? `${formatSize(renderStats.jsHeapUsedBytes)} / ${formatSize(renderStats.jsHeapTotalBytes ?? 0)}`
                          : "WebView private"}
                      </strong>
                    </div>
                    <div>
                      <span className="muted">Open tabs / panes</span>
                      <strong>
                        {tabCount} tabs · {paneCount} pane{paneCount === 1 ? "" : "s"}
                      </strong>
                    </div>
                    <div>
                      <span className="muted">Active route</span>
                      <strong className="metrics-mono" title={activeNav}>
                        {activeNav || "(home)"}
                      </strong>
                    </div>
                    <div>
                      <span className="muted">App PID / threads / handles</span>
                      <strong>
                        {snap.appPid} · {snap.appThreads} threads · {snap.appHandles} handles
                      </strong>
                    </div>
                    <div>
                      <span className="muted">App uptime / virtual RAM</span>
                      <strong>
                        {formatDurationSecs(snap.appUptimeSecs)} · {formatSize(snap.appVirtualBytes)}
                      </strong>
                    </div>
                    <div>
                      <span className="muted">Workers busy (fast / slow thumbs)</span>
                      <strong>
                        {snap.thumbWorkersBusy}/{snap.thumbWorkersTotal} · {snap.slowWorkersBusy}/{snap.slowWorkersTotal}
                      </strong>
                    </div>
                    <div>
                      <span className="muted">Active jobs (copy / del / analyze / shares)</span>
                      <strong>
                        {snap.activeTransfers} / {snap.activeDeletes} / {snap.activeAnalyses} / {snap.activeShares}
                      </strong>
                    </div>
                    <div>
                      <span className="muted">Watched folders / SSH terminals</span>
                      <strong>
                        {snap.watchedDirs} / {snap.openTerminals}
                      </strong>
                    </div>
                  </div>
                </div>
              </section>
            )}

            {/* App Processing & Operation Latencies */}
            {showSection("processing") && (
              <section className="metrics-section">
                <div className="metrics-card">
                  <h4>Operation Processing Speed & Expected Latency</h4>
                  {snap.telemetry.operations.length === 0 ? (
                    <p className="muted">
                      Navigate folders, search, load thumbnails, or open a file while this window is active to measure exact latency expectations.
                    </p>
                  ) : (
                    <div className="metrics-table-wrap">
                      <table className="metrics-table">
                        <thead>
                          <tr>
                            <th>Operation</th>
                            <th>Calls</th>
                            <th>Err</th>
                            <th>Last</th>
                            <th>Avg</th>
                            <th>p95</th>
                            <th>Max</th>
                            <th>Expected</th>
                          </tr>
                        </thead>
                        <tbody>
                          {snap.telemetry.operations.map((o) => (
                            <tr key={o.op}>
                              <td>{opTitle(o.op)}</td>
                              <td>{o.count}</td>
                              <td className={o.errors > 0 ? "metrics-err" : undefined}>{o.errors}</td>
                              <td>{formatMs(o.lastMs)}</td>
                              <td>{formatMs(o.avgMs)}</td>
                              <td>{formatMs(o.p95Ms)}</td>
                              <td>{formatMs(o.maxMs)}</td>
                              <td>
                                <strong>{formatMs(o.expectedMs)}</strong>
                              </td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  )}
                </div>

                {snap.telemetry.recent.length > 0 && (
                  <div className="metrics-card">
                    <h4>Recent Completed Operations (newest first)</h4>
                    <div className="metrics-table-wrap">
                      <table className="metrics-table">
                        <thead>
                          <tr>
                            <th>#</th>
                            <th>Op</th>
                            <th>Duration</th>
                            <th>Status</th>
                            <th>Age</th>
                            <th>Detail</th>
                          </tr>
                        </thead>
                        <tbody>
                          {snap.telemetry.recent.slice(0, 18).map((ev) => (
                            <tr key={ev.seq}>
                              <td>{ev.seq}</td>
                              <td>{ev.op}</td>
                              <td>
                                <strong>{formatMs(ev.durationMs)}</strong>
                              </td>
                              <td className={ev.ok ? "metrics-ok" : "metrics-err"}>{ev.ok ? "ok" : "err"}</td>
                              <td className="muted">{ev.ageMs} ms ago</td>
                              <td className="metrics-mono" title={ev.detail}>
                                {ev.detail}
                              </td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  </div>
                )}
              </section>
            )}

            {/* Network, Remote Endpoints & Caches */}
            {showSection("network") && (
              <section className="metrics-section">
                <div className="metrics-two-col">
                  <div className="metrics-card">
                    <h4>Remote App Throughput & Expected Server RTT</h4>
                    <div className="metrics-grid-kv">
                      <div>
                        <span className="muted">Remote transfer read speed</span>
                        <strong>
                          {rate(snap.remoteReadPerSec)} <span className="muted">({formatSize(snap.remoteReadTotal)} total)</span>
                        </strong>
                      </div>
                      <div>
                        <span className="muted">Remote transfer write speed</span>
                        <strong>
                          {rate(snap.remoteWritePerSec)} <span className="muted">({formatSize(snap.remoteWriteTotal)} total)</span>
                        </strong>
                      </div>
                      <div>
                        <span className="muted">System net RX / TX</span>
                        <strong>
                          ↓ {rate(snap.netRxPerSec)} · ↑ {rate(snap.netTxPerSec)}
                        </strong>
                      </div>
                      <div>
                        <span className="muted">Cumulative system net</span>
                        <strong>
                          ↓ {formatSize(snap.netTotalRx)} · ↑ {formatSize(snap.netTotalTx)}
                        </strong>
                      </div>
                    </div>
                    {snap.remotes.length > 0 ? (
                      <div className="metrics-table-wrap" style={{ marginTop: 8 }}>
                        <table className="metrics-table">
                          <thead>
                            <tr>
                              <th>Connected Server</th>
                              <th>Idle Pool</th>
                              <th>Expected RTT</th>
                            </tr>
                          </thead>
                          <tbody>
                            {snap.remotes.map((r) => (
                              <tr key={r.url}>
                                <td className="metrics-mono">
                                  {r.host}:{r.port}
                                </td>
                                <td>{r.idleConnections}</td>
                                <td>{r.rttMs != null ? formatMs(r.rttMs) : "measuring…"}</td>
                              </tr>
                            ))}
                          </tbody>
                        </table>
                      </div>
                    ) : (
                      <p className="muted" style={{ marginTop: 8 }}>
                        No active SSH/SFTP/FTP endpoints connected right now.
                      </p>
                    )}
                  </div>

                  <div className="metrics-card">
                    <h4>Internal Caches & Memory Footprint</h4>
                    <div className="metrics-grid-kv">
                      <div>
                        <span className="muted">Search index state / entries</span>
                        <strong>
                          {snap.indexState} · {snap.indexEntries.toLocaleString()} items ({formatSize(snap.indexMemoryBytes)})
                        </strong>
                      </div>
                      <div>
                        <span className="muted">Folder listing cache RAM</span>
                        <strong>{formatSize(snap.listingsMemoryBytes)}</strong>
                      </div>
                      <div>
                        <span className="muted">Folder size cache RAM</span>
                        <strong>{formatSize(snap.folderSizesMemoryBytes)}</strong>
                      </div>
                      <div>
                        <span className="muted">App cumulative disk R/W</span>
                        <strong>
                          {formatSize(snap.appDiskReadTotal)} R / {formatSize(snap.appDiskWriteTotal)} W
                        </strong>
                      </div>
                    </div>
                  </div>
                </div>

                {snap.interfaces.length > 0 && (
                  <div className="metrics-card">
                    <h4>Network Interfaces</h4>
                    <div className="metrics-table-wrap">
                      <table className="metrics-table">
                        <thead>
                          <tr>
                            <th>Interface</th>
                            <th>RX / s</th>
                            <th>TX / s</th>
                            <th>Total RX</th>
                            <th>Total TX</th>
                            <th>Packets RX/TX</th>
                            <th>Errors</th>
                          </tr>
                        </thead>
                        <tbody>
                          {snap.interfaces.map((iface) => (
                            <tr key={iface.name}>
                              <td>{iface.name}</td>
                              <td>{rate(iface.rxPerSec)}</td>
                              <td>{rate(iface.txPerSec)}</td>
                              <td>{formatSize(iface.totalRx)}</td>
                              <td>{formatSize(iface.totalTx)}</td>
                              <td className="muted">
                                {iface.packetsRx.toLocaleString()} / {iface.packetsTx.toLocaleString()}
                              </td>
                              <td className={iface.errorsRx + iface.errorsTx > 0 ? "metrics-err" : undefined}>
                                {iface.errorsRx + iface.errorsTx}
                              </td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  </div>
                )}
              </section>
            )}

            {/* System Hardware, Per-Core CPU, Volumes & Sensors */}
            {showSection("system") && (
              <section className="metrics-section">
                <div className="metrics-card">
                  <h4>System Overview & Per-Core CPU</h4>
                  <div className="metrics-grid-kv">
                    <div>
                      <span className="muted">OS / Kernel</span>
                      <strong>
                        {snap.osName} ({snap.kernelVersion})
                      </strong>
                    </div>
                    <div>
                      <span className="muted">OS uptime / load avg (1/5/15m)</span>
                      <strong>
                        {formatDurationSecs(snap.uptimeSecs)} · {snap.loadAvg.map((x) => x.toFixed(2)).join(" / ")}
                      </strong>
                    </div>
                    <div>
                      <span className="muted">RAM used / free / total</span>
                      <strong>
                        {formatSize(snap.memoryUsedBytes)} / {formatSize(snap.memoryAvailableBytes)} / {formatSize(snap.memoryTotalBytes)}
                      </strong>
                    </div>
                    <div>
                      <span className="muted">Swap used / total</span>
                      <strong>
                        {formatSize(snap.swapUsedBytes)} / {formatSize(snap.swapTotalBytes)}
                      </strong>
                    </div>
                  </div>

                  <div className="metrics-core-grid">
                    {snap.perCoreCpu.map((pct, i) => (
                      <div key={i} className="metrics-core">
                        <div className="metrics-core-head">
                          <span>CPU {i}</span>
                          <span>{pct.toFixed(0)}%</span>
                        </div>
                        <div className="metrics-bar">
                          <div className="metrics-bar-fill" style={{ width: `${Math.min(100, Math.max(0, pct))}%` }} />
                        </div>
                      </div>
                    ))}
                  </div>
                </div>

                <div className="metrics-two-col">
                  <div className="metrics-card">
                    <h4>Mounted Volumes</h4>
                    <div className="metrics-table-wrap">
                      <table className="metrics-table">
                        <thead>
                          <tr>
                            <th>Mount</th>
                            <th>FS / Kind</th>
                            <th>Free / Total</th>
                          </tr>
                        </thead>
                        <tbody>
                          {snap.disks.map((d) => (
                            <tr key={`${d.mount}-${d.name}`}>
                              <td>
                                <strong>{d.mount}</strong> <span className="muted">{d.name}</span>
                              </td>
                              <td className="muted">
                                {d.fs} · {d.kind}
                                {d.readOnly ? " (RO)" : ""}
                              </td>
                              <td>
                                {formatSize(d.availableBytes)} / {formatSize(d.totalBytes)}
                              </td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  </div>

                  <div className="metrics-card">
                    <h4>Temperature Sensors</h4>
                    {snap.temperatures.length === 0 ? (
                      <p className="muted">No unprivileged hardware temperature sensors exposed by OS (try Kernel & {elevateVerb} section below).</p>
                    ) : (
                      <div className="metrics-table-wrap">
                        <table className="metrics-table">
                          <thead>
                            <tr>
                              <th>Sensor</th>
                              <th>Temp</th>
                              <th>Max</th>
                            </tr>
                          </thead>
                          <tbody>
                            {snap.temperatures.map((t) => (
                              <tr key={t.label}>
                                <td>{t.label}</td>
                                <td>
                                  <strong>{t.celsius.toFixed(1)} °C</strong>
                                </td>
                                <td className="muted">{t.maxCelsius != null ? `${t.maxCelsius.toFixed(1)} °C` : "—"}</td>
                              </tr>
                            ))}
                          </tbody>
                        </table>
                      </div>
                    )}
                  </div>
                </div>
              </section>
            )}

            {/* Privileged Kernel & Hardware Telemetry (Admin / sudo / root gated) */}
            {showSection("admin") && (
              <section className="metrics-section">
                <div className="metrics-card metrics-admin-card">
                  <div className="metrics-admin-head">
                    <div>
                      <h4>
                        <Icon name="shield" size={14} /> Privileged Kernel, Physical Disk Latency, Sockets & GPU Telemetry
                      </h4>
                      <p className="muted">
                        Kernel pool allocations, page faults, interrupts/DPCs, physical storage queue depth & ms latency, system TCP/UDP socket states, ACPI thermal zones, and GPU VRAM usage require {elevateVerb} rights and are only queried when you request them.
                      </p>
                    </div>
                    {snap.canElevate && (
                      <button type="button" className="primary" disabled={adminBusy} onClick={() => void collectAdmin()}>
                        <Icon name="shield" size={14} />{" "}
                        {adminBusy
                          ? "Waiting for permission…"
                          : adminData
                            ? `Refresh (${elevateVerb})`
                            : snap.isElevated
                              ? "Load kernel telemetry"
                              : `Unlock with ${elevateVerb} rights`}
                      </button>
                    )}
                  </div>

                  {adminError && (
                    <p className="metrics-err" role="alert">
                      {adminError}
                    </p>
                  )}

                  {adminData && (
                    <div className="metrics-admin-body">
                      <div className="metrics-grid-kv">
                        <div>
                          <span className="muted">Elevated context</span>
                          <strong>{adminData.elevatedIdentity}</strong>
                        </div>
                        <div>
                          <span className="muted">Kernel Non-Paged / Paged Pool</span>
                          <strong>
                            {adminData.kernel.nonPagedPoolBytes != null ? formatSize(adminData.kernel.nonPagedPoolBytes) : "—"} /{" "}
                            {adminData.kernel.pagedPoolBytes != null ? formatSize(adminData.kernel.pagedPoolBytes) : "—"}
                          </strong>
                        </div>
                        <div>
                          <span className="muted">Commit Charge / Limit</span>
                          <strong>
                            {adminData.kernel.commitBytes != null ? formatSize(adminData.kernel.commitBytes) : "—"} /{" "}
                            {adminData.kernel.commitLimitBytes != null ? formatSize(adminData.kernel.commitLimitBytes) : "—"}
                          </strong>
                        </div>
                        <div>
                          <span className="muted">Page faults / Context switches / Syscalls (/s)</span>
                          <strong>
                            {adminData.kernel.pageFaultsPerSec ?? "—"} / {adminData.kernel.contextSwitchesPerSec ?? "—"} /{" "}
                            {adminData.kernel.systemCallsPerSec ?? "—"}
                          </strong>
                        </div>
                        <div>
                          <span className="muted">Interrupt / DPC CPU time</span>
                          <strong>
                            {adminData.kernel.interruptPercent != null ? `${adminData.kernel.interruptPercent.toFixed(2)}%` : "—"} /{" "}
                            {adminData.kernel.dpcPercent != null ? `${adminData.kernel.dpcPercent.toFixed(2)}%` : "—"}
                          </strong>
                        </div>
                        <div>
                          <span className="muted">Sockets (TCP est / listen / TW · UDP)</span>
                          <strong>
                            {adminData.sockets.tcpEstablished} / {adminData.sockets.tcpListening} / {adminData.sockets.tcpTimeWait} · UDP{" "}
                            {adminData.sockets.udpEndpoints}
                          </strong>
                        </div>
                      </div>

                      {adminData.physicalDisks.length > 0 && (
                        <div className="metrics-table-wrap" style={{ marginTop: 10 }}>
                          <table className="metrics-table">
                            <thead>
                              <tr>
                                <th>Physical Disk</th>
                                <th>Queue Depth</th>
                                <th>Avg Read Latency</th>
                                <th>Avg Write Latency</th>
                                <th>Busy %</th>
                              </tr>
                            </thead>
                            <tbody>
                              {adminData.physicalDisks.map((d) => (
                                <tr key={d.name}>
                                  <td>{d.name}</td>
                                  <td>{d.queueLength.toFixed(2)}</td>
                                  <td>{d.avgReadMs.toFixed(2)} ms</td>
                                  <td>{d.avgWriteMs.toFixed(2)} ms</td>
                                  <td>{d.busyPercent.toFixed(1)}%</td>
                                </tr>
                              ))}
                            </tbody>
                          </table>
                        </div>
                      )}

                      {(adminData.gpuAdapters.length > 0 || adminData.thermalZones.length > 0) && (
                        <div className="metrics-two-col" style={{ marginTop: 10 }}>
                          {adminData.gpuAdapters.length > 0 && (
                            <div className="metrics-table-wrap">
                              <table className="metrics-table">
                                <thead>
                                  <tr>
                                    <th>GPU</th>
                                    <th>Dedicated VRAM</th>
                                    <th>Shared</th>
                                    <th>Engine %</th>
                                  </tr>
                                </thead>
                                <tbody>
                                  {adminData.gpuAdapters.map((g) => (
                                    <tr key={g.name}>
                                      <td>{g.name}</td>
                                      <td>{formatSize(g.dedicatedUsedBytes)}</td>
                                      <td>{formatSize(g.sharedUsedBytes)}</td>
                                      <td>{g.enginePercent != null ? `${g.enginePercent.toFixed(1)}%` : "—"}</td>
                                    </tr>
                                  ))}
                                </tbody>
                              </table>
                            </div>
                          )}

                          {adminData.thermalZones.length > 0 && (
                            <div className="metrics-table-wrap">
                              <table className="metrics-table">
                                <thead>
                                  <tr>
                                    <th>ACPI Thermal Zone</th>
                                    <th>Temperature</th>
                                  </tr>
                                </thead>
                                <tbody>
                                  {adminData.thermalZones.map((z) => (
                                    <tr key={z.name}>
                                      <td>{z.name}</td>
                                      <td>{z.celsius.toFixed(1)} °C</td>
                                    </tr>
                                  ))}
                                </tbody>
                              </table>
                            </div>
                          )}
                        </div>
                      )}

                      {adminData.notes.map((n, i) => (
                        <p key={i} className="muted" style={{ marginTop: 6 }}>
                          {n}
                        </p>
                      ))}
                    </div>
                  )}
                </div>
              </section>
            )}
          </>
        )}
      </div>
    </div>
  );
}

export function MetricsSettingsSection() {
  return <MetricsPanel clientId="settings" autoStart={false} />;
}

export function MetricsModal() {
  const open = useStore((s) => s.metricsOpen);
  const close = () => useStore.getState().closeMetrics();
  if (!open) return null;
  return (
    <div className="settings-backdrop" onMouseDown={close}>
      <div
        className="settings metrics-modal"
        role="dialog"
        aria-modal="true"
        aria-label="App and System Metrics"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") close();
          e.stopPropagation();
        }}
      >
        <div className="metrics-modal-close">
          <button type="button" className="icon-button" aria-label="Close metrics" onClick={close}>
            <Icon name="close" />
          </button>
        </div>
        <MetricsPanel clientId="modal" autoStart onClose={close} />
      </div>
    </div>
  );
}

export function DetachedMetrics() {
  return (
    <div className="settings metrics-modal" data-detached>
      <MetricsPanel clientId="window" detached autoStart />
    </div>
  );
}
