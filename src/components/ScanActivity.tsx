import { useEffect, useMemo, useRef, useState } from "react";
import type { AnalysisProgress } from "../api";
import { useStore } from "../store";
import { formatCount, formatDuration, formatSize } from "../util/format";
import { estimateTotal, expectedRate, pushLimited, recentRate, secondsLeft, sparkPath, type ScanSample } from "../util/scanStats";

const HISTORY = 60; // about 15 seconds of samples

function Spark({ values, label, ceiling }: { values: number[]; label: string; ceiling?: number }) {
  const d = sparkPath(values, 120, 28, ceiling);
  return (
    <svg className="spark" viewBox="0 0 120 28" width={120} height={28} role="img" aria-label={label}>
      <line x1="0" y1="27.5" x2="120" y2="27.5" className="spark-base" />
      {d && <path d={`${d} L120,28 L0,28 Z`} className="spark-fill" />}
      {d && <path d={d} className="spark-line" />}
    </svg>
  );
}

/**
 * Live numbers while a disk-usage scan runs: how hard the drive is working,
 * how many files per second are being found, and what to expect. Shown only
 * when the backend sends drive activity (Settings: live drive activity).
 */
export function ScanActivity({ path, progress }: { path: string; progress: AnalysisProgress }) {
  const drives = useStore((s) => s.drives);
  const drive = useMemo(() => {
    const p = path.toLowerCase();
    return drives
      .filter((d) => p.startsWith(d.mount.toLowerCase().replace(/[\\/]+$/, "")))
      .sort((a, b) => b.mount.length - a.mount.length)[0];
  }, [drives, path]);

  const samples = useRef<ScanSample[]>([]);
  const [io, setIo] = useState<number[]>([]);
  const [rates, setRates] = useState<number[]>([]);
  const peak = useRef(0);

  // A new scan starts from nothing.
  useEffect(() => {
    samples.current = [];
    setIo([]);
    setRates([]);
    peak.current = 0;
  }, [path]);

  const rate = (() => {
    const list = samples.current;
    return recentRate(list);
  })();

  useEffect(() => {
    if (progress.path !== path || !progress.activity) return;
    if (progress.elapsedSecs < (samples.current.at(-1)?.t ?? -1)) {
      samples.current = []; // the scan restarted
      peak.current = 0;
    }
    samples.current = pushLimited(samples.current, { t: progress.elapsedSecs, entries: progress.entries, bytes: progress.bytes }, HISTORY);
    const bps = progress.activity.readBps + progress.activity.writeBps;
    peak.current = Math.max(peak.current, bps);
    setIo((v) => pushLimited(v, bps, HISTORY));
    setRates((v) => pushLimited(v, recentRate(samples.current), HISTORY));
  }, [progress, path]);

  if (!progress.activity) return null;
  const bps = io.at(-1) ?? 0;
  const media = drive?.media ?? "unknown";
  const expected = expectedRate(media, progress.entries, progress.elapsedSecs);
  const used = drive ? Math.max(0, drive.total - drive.available) : 0;
  // The estimate is for a whole drive; for a folder it would be far too high.
  const wholeDrive = !!drive && path.replace(/[\\/]+$/, "").toLowerCase() === drive.mount.replace(/[\\/]+$/, "").toLowerCase();
  const total = wholeDrive ? estimateTotal(used, progress.entries, progress.bytes) : null;
  const left = secondsLeft(total, progress.entries, rate || expected);

  return (
    <div className="scan-activity" aria-label="Live drive activity">
      <div className="scan-metric">
        <Spark values={io} ceiling={peak.current} label="Drive throughput over the last seconds" />
        <div>
          <strong>{formatSize(bps)}/s</strong>
          <span className="muted">drive busy · peak {formatSize(peak.current)}/s</span>
        </div>
      </div>
      <div className="scan-metric">
        <Spark values={rates} label="Files found per second over the last seconds" />
        <div>
          <strong>{formatCount(Math.round(rate))} files/s</strong>
          <span className="muted">
            expected ~{formatCount(Math.round(expected))}/s
            {total != null && ` · about ${formatCount(total)} items in all${left != null ? `, ${formatDuration(left)} left` : ""}`}
          </span>
        </div>
      </div>
    </div>
  );
}
