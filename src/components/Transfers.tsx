// Transfers and deletes. By default each job is a small card; the expand
// button turns the stack into one larger window that holds every job, with a
// speed graph for the selected one.

import { useEffect, useMemo, useRef, useState } from "react";
import { api, type DeleteProgress, type Progress } from "../api";
import { retryTransferAsAdmin, useStore, type Sample } from "../store";
import { reportFiles, reportOf } from "../util/jobReport";
import { formatCount, formatDuration, formatEta, formatRate, formatSize } from "../util/format";
import { baseName } from "../util/path";
import { adv } from "../settings/advanced";
import { metricOn } from "../settings/transferMetrics";
import { METHOD_NAME } from "../util/methods";
import { Icon } from "./Icons";

const STRATEGY_LABEL: Record<NonNullable<Progress["strategy"]>, string> = {
  rename: "Instant move (same drive)",
  largeSequential: "Large-file mode: few streams, big buffers",
  smallParallel: "Many-small-files mode: many parallel streams",
  mixed: "Mixed mode: large files sequentially, small ones in parallel",
};

const MEDIA_LABEL: Record<string, string> = { ssd: "SSD", hdd: "hard disk", network: "network", removable: "removable", unknown: "unknown drive" };

/** `at` is when a finished job ended (jobs shown from the finished list have it). */
type Job = ({ kind: "transfer"; p: Progress } | { kind: "delete"; p: DeleteProgress }) & { at?: number };

function isActive(j: Job): boolean {
  return j.p.phase === "running" || j.p.phase === "scanning" || (j.kind === "transfer" && j.p.phase === "queued") || (j.kind === "transfer" && j.p.phase === "paused");
}

function titleOf(j: Job): string {
  if (j.kind === "delete") {
    const p = j.p;
    const what = p.selected === 1 ? `"${p.firstName}"` : `${formatCount(p.selected)} items`;
    if (p.action === "update") return p.phase === "done" ? "Update downloaded" : p.phase === "cancelled" ? "Update download cancelled" : p.phase === "failed" ? "Could not download the update" : "Downloading the update";
    if (p.action === "rename-copy") return p.phase === "done" ? `Renamed ${what}` : p.phase === "cancelled" ? "Rename cancelled" : p.phase === "failed" ? `Could not rename ${what}` : p.phase === "scanning" ? `Preparing to rename ${what}…` : `Renaming ${what} by copying`;
    if (p.action === "compress") return p.phase === "done" ? `Compressed ${what}` : p.phase === "cancelled" ? "Compressing cancelled" : p.phase === "failed" ? `Could not compress ${what}` : `Compressing ${what}`;
    if (p.action === "extract") return p.phase === "done" ? `Extracted ${what}` : p.phase === "cancelled" ? "Extracting cancelled" : p.phase === "failed" ? `Could not extract ${what}` : `Extracting ${what}`;
    if (p.action === "upload" || p.action === "download") {
      const [ing, ed, to] = p.action === "upload" ? ["Uploading", "Uploaded", "Upload"] : ["Downloading", "Downloaded", "Download"];
      return p.phase === "done" ? `${ed} ${what}` : p.phase === "cancelled" ? `${to} cancelled` : p.phase === "failed" ? `Could not ${to.toLowerCase()} ${what}` : p.phase === "scanning" ? `Preparing to ${to.toLowerCase()} ${what}…` : `${ing} ${what}`;
    }
    if (p.action === "pack") return p.phase === "done" ? `Put ${what} in the archive` : p.phase === "cancelled" ? "Changing the archive cancelled" : p.phase === "failed" ? `Could not put ${what} in the archive` : p.phase === "scanning" ? `Preparing ${what} for the archive…` : `Putting ${what} in the archive`;
    if (p.action === "unpack") return p.phase === "done" ? `Copied ${what} out of the archive` : p.phase === "cancelled" ? "Copying out cancelled" : p.phase === "failed" ? `Could not copy ${what} out of the archive` : p.phase === "scanning" ? `Preparing to copy ${what} out of the archive…` : `Copying ${what} out of the archive`;
    if (p.action === "open") return p.phase === "failed" ? `Could not open ${what}` : p.phase === "cancelled" ? "Opening cancelled" : p.phase === "done" ? `Opened ${what}` : `Fetching ${what} to open it`;
    if (p.action === "remote-delete") return p.phase === "done" ? `Deleted ${what} from the server` : p.phase === "cancelled" ? "Delete cancelled" : p.phase === "failed" ? `Could not delete ${what}` : `Deleting ${what} from the server`;
    if (p.phase === "scanning") return `Preparing to delete ${what}…`;
    if (p.phase === "done") return `Deleted ${what}`;
    if (p.phase === "cancelled") return "Delete cancelled";
    if (p.phase === "failed") return `Could not delete ${what}`;
    return `${p.permanent ? "Deleting" : "Moving to the bin"}: ${what}`;
  }
  const p = j.p;
  const dest = baseName(p.destDir);
  const verb = p.op === "move" ? ["Moving", "Moved"] : ["Copying", "Copied"];
  if (p.phase === "queued") return `${p.op === "move" ? "Move" : "Copy"} waiting for its turn${p.queuedAhead ? ` (${p.queuedAhead} ahead)` : ""}`;
  if (p.phase === "scanning") return `Preparing to ${p.op}…`;
  if (p.phase === "done") return `${verb[1]} ${formatCount(p.filesDone)} item${p.filesDone === 1 ? "" : "s"} to ${dest}`;
  if (p.phase === "cancelled") return `${p.op === "move" ? "Move" : "Copy"} cancelled`;
  if (p.phase === "failed") return `Could not ${p.op} to ${dest}`;
  return `${verb[0]} ${formatCount(p.filesTotal)} item${p.filesTotal === 1 ? "" : "s"} to ${dest}`;
}

function useFraction(j: Job): number {
  return useStore((s) => s.shown[j.p.id] ?? 0);
}

function cancel(j: Job, force = false) {
  const s = useStore.getState();
  if (force) s.forceStop(j.kind, j.p.id);
  else s.requestCancel(j.kind, j.p.id);
}

/** Shown once Cancel has been asked for: a stuck job can be let go of right away. */
function ForceStop({ j }: { j: Job }) {
  const asked = useStore((s) => s.cancelling[j.p.id]);
  if (!asked) return null;
  return (
    <div className="force-stop">
      <span className="muted">Cancelling… waiting for the drive to respond.</span>
      <button className="secondary" title="Stop right now without waiting. What was already copied stays where it is." onClick={(e) => (e.stopPropagation(), cancel(j, true))}>
        Force stop
      </button>
    </div>
  );
}

function Bar({ fraction, indeterminate }: { fraction: number; indeterminate?: boolean }) {
  if (indeterminate) return <div className="loading-bar" />;
  return (
    <div className="meter" role="progressbar" aria-valuenow={Math.round(fraction * 100)} aria-valuemin={0} aria-valuemax={100}>
      <div className="meter-fill" style={{ width: `${fraction * 100}%` }} />
    </div>
  );
}

function Issues({ p, retry }: { p: Progress | DeleteProgress; retry?: () => void }) {
  if (p.errorCount === 0 && !("skipped" in p && (p.skipped ?? 0) > 0)) return null;
  const skipped = ("skipped" in p ? p.skipped : 0) ?? 0;
  return (
    <div className="transfer-issues">
      {skipped > 0 && <span>{formatCount(skipped)} skipped</span>}
      {p.errorCount > 0 && (
        <details>
          <summary>
            {formatCount(p.errorCount)} item{p.errorCount === 1 ? "" : "s"} failed
          </summary>
          <ul>
            {p.errors.slice(0, 20).map((e, i) => (
              <li key={i} title={e.path}>
                <span className="path">{baseName(e.path)}</span>: {e.message}
              </li>
            ))}
          </ul>
        </details>
      )}
      {retry && (
        <button className="secondary" onClick={retry}>
          Retry as administrator
        </button>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Small card
// ---------------------------------------------------------------------------

function Card({ job }: { job: Job }) {
  const fraction = useFraction(job);
  const spec = useStore((s) => (job.kind === "transfer" ? s.specs[job.p.id] : undefined));
  const { dismissTransfer, dismissDelete } = useStore.getState();
  const active = isActive(job);
  const queued = job.kind === "transfer" && job.p.phase === "queued";
  const counting = job.kind === "transfer" && !!job.p.counting;
  const scanning = job.p.phase === "scanning" || queued;
  const p = job.p;
  const needsAdmin = !active && p.errors.some((e) => e.needsElevation);
  const dismiss = () => (job.kind === "delete" ? dismissDelete(p.id) : dismissTransfer(p.id));

  return (
    <div className="transfer-card" data-phase={p.phase}>
      <div className="transfer-head">
        <strong>{titleOf(job)}</strong>
        <div className="transfer-actions">
          {queued && (
            <button className="link-button" onClick={() => void api.transferAdjust(p.id, "startNow")} title="Skip the queue and begin now">
              Start now
            </button>
          )}
          {job.kind === "transfer" && active && !scanning && (
            <button
              className="icon-button"
              aria-label={job.p.phase === "paused" ? "Resume" : "Pause"}
              title={job.p.phase === "paused" ? "Resume" : "Pause"}
              onClick={() => void api.transferControl(p.id, job.p.phase === "paused" ? "resume" : "pause")}
            >
              <Icon name={job.p.phase === "paused" ? "play" : "pause"} size={12} />
            </button>
          )}
          {active && (
            <button className="icon-button" aria-label="Expand" title="Show more details" onClick={() => (useStore.getState().detached.transfers ? void api.openTransfersWindow() : useStore.setState({ transfersOpen: true }))}>
              <Icon name="expand" size={12} />
            </button>
          )}
          <button className="icon-button" data-action={active ? "cancel" : "dismiss"} aria-label={active ? "Cancel" : "Dismiss"} title={active ? "Cancel (Shift-click to force stop)" : "Dismiss"} onClick={(e) => (active ? cancel(job, e.shiftKey) : dismiss())}>
            <Icon name="close" size={12} />
          </button>
        </div>
      </div>
      {active && <ForceStop j={job} />}
      {active && (
        <>
          <Bar fraction={fraction} indeterminate={scanning || counting} />
          {job.kind === "transfer" ? (
            <>
              <div className="transfer-stats">
                <span className="eta-line">{job.p.phase === "paused" ? "Paused" : counting ? "Counting files…" : formatEta(job.p.etaSecs)}<EtaNote p={job.p} /></span>
                <span className="transfer-rate">{job.p.phase === "running" ? formatRate(job.p) : ""}</span>
              </div>
              <div className="transfer-stats muted">
                <span>{formatCount(job.p.filesDone)} / {formatCount(job.p.filesTotal)}{counting ? "+" : ""} items</span>
                <span>{formatSize(job.p.bytesDone)} / {formatSize(job.p.bytesTotal)}{counting ? "+" : ""}</span>
              </div>
            </>
          ) : (
            <>
              {job.p.stats && !scanning && (
                <div className="transfer-stats">
                  <span className="eta-line">{formatEta(job.p.stats.etaSecs)}<EtaNote p={{ phase: job.p.phase, etaSecs: job.p.stats.etaSecs, etaConfidence: job.p.stats.etaConfidence }} /></span>
                  <span className="transfer-rate">{jobRate(job.p.stats)}</span>
                </div>
              )}
              <div className="transfer-stats muted">
                {job.p.bytesTotal > 0 ? <span>{formatSize(job.p.bytesDone)} / {formatSize(job.p.bytesTotal)}</span> : <span>{formatCount(job.p.itemsDone)} / {formatCount(job.p.itemsTotal)} items{scanning ? " found so far" : ""}</span>}
                <span>{formatDuration(job.p.elapsedSecs)}</span>
              </div>
            </>
          )}
        </>
      )}
      <Issues p={p} retry={needsAdmin && spec ? () => { dismiss(); void retryTransferAsAdmin(spec); } : undefined} />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Graph: files/s and MB/s over time, the time axis fitted to the job so far
// ---------------------------------------------------------------------------

function niceMax(v: number): number {
  if (v <= 0) return 1;
  const pow = 10 ** Math.floor(Math.log10(v));
  const n = v / pow;
  return (n <= 1 ? 1 : n <= 2 ? 2 : n <= 5 ? 5 : 10) * pow;
}

const METHOD_LABEL: Record<string, string> = { robocopy: "Robocopy", rsync: "rsync", rclone: "rclone", smb: "SMB-tuned copy", nfs: "NFS-tuned rsync" };

/** Speed of a delete, archive or server job: items per second and, when known, bytes per second. */
function jobRate(s: { itemsPerSec: number; bytesPerSec: number }): string {
  const items = `${s.itemsPerSec >= 10 ? formatCount(s.itemsPerSec) : s.itemsPerSec.toFixed(1)} items/s`;
  return s.bytesPerSec > 0 ? `${formatSize(s.bytesPerSec)}/s · ${items}` : items;
}

export function SpeedGraph({ samples, total, etaSecs, bytes = true }: { samples: Sample[]; total: number | null; etaSecs?: number | null; bytes?: boolean }) {
  const W = 520;
  const H = 150;
  const pad = { l: 46, r: 56, t: 10, b: 22 };
  const w = W - pad.l - pad.r;
  const h = H - pad.t - pad.b;
  const now = samples.at(-1)?.t ?? 0;
  // Leave empty room on the right (up to the expected finish) so the streak has somewhere to run.
  const end = Math.max(5, now * 1.15, now + Math.min(etaSecs ?? 0, now * 0.6 + 5), 1);
  const last = samples.at(-1);
  const maxFiles = niceMax(Math.max(1, ...samples.map((s) => s.filesPerSec)));
  const maxBytes = niceMax(Math.max(1, ...samples.map((s) => s.bytesPerSec)));
  const x = (t: number) => pad.l + (t / end) * w;
  const line = (pick: (s: Sample) => number, max: number) =>
    samples.map((s, i) => `${i ? "L" : "M"}${x(s.t).toFixed(1)} ${(pad.t + h - (pick(s) / max) * h).toFixed(1)}`).join(" ");
  const ticks = [0, 0.25, 0.5, 0.75, 1];
  return (
    <svg className="speed-graph" viewBox={`0 0 ${W} ${H}`} role="img" aria-label="Transfer speed over time">
      {ticks.map((f) => (
        <g key={f}>
          <line x1={pad.l} x2={pad.l + w} y1={pad.t + h - f * h} y2={pad.t + h - f * h} className="graph-grid" />
          <text x={pad.l - 6} y={pad.t + h - f * h + 3} textAnchor="end" className="graph-label files">
            {f === 0 ? "0" : formatCount(maxFiles * f)}
          </text>
          {bytes && (
            <text x={pad.l + w + 6} y={pad.t + h - f * h + 3} className="graph-label bytes">
              {f === 0 ? "0" : formatSize(maxBytes * f).replace(" ", "")}
            </text>
          )}
        </g>
      ))}
      {[0, 0.5, 1].map((f) => (
        <text key={f} x={pad.l + f * w} y={H - 6} textAnchor={f === 0 ? "start" : f === 1 ? "end" : "middle"} className="graph-label">
          {formatDuration(end * f) || "0 sec"}
        </text>
      ))}
      {samples.length > 1 && (
        <>
          {bytes && <path d={line((s) => s.bytesPerSec, maxBytes)} className="graph-line bytes" />}
          <path d={line((s) => s.filesPerSec, maxFiles)} className="graph-line files" />
        </>
      )}
      {last && samples.length > 1 && (
        <>
          {([["bytes", last.bytesPerSec, maxBytes], ["files", last.filesPerSec, maxFiles]] as const).filter(([k]) => bytes || k === "files").map(([k, v, max]) => {
            const y = pad.t + h - (v / max) * h;
            return (
              <g key={k}>
                <line x1={x(last.t)} x2={pad.l + w} y1={y} y2={y} className={`graph-streak ${k}`} />
                <circle cx={x(last.t)} cy={y} r={2.5} className={`graph-head ${k}`} />
              </g>
            );
          })}
        </>
      )}
      {total != null && total > 0 && total < end * 1.5 && <line x1={x(Math.min(total, end))} x2={x(Math.min(total, end))} y1={pad.t} y2={pad.t + h} className="graph-typical" />}
      <text x={pad.l} y={pad.t - 1} className="graph-label files" fontSize="9">files/s</text>
      {bytes && <text x={pad.l + w} y={pad.t - 1} textAnchor="end" className="graph-label bytes" fontSize="9">MB/s</text>}
    </svg>
  );
}

// ---------------------------------------------------------------------------
// Expanded window
// ---------------------------------------------------------------------------

/** Says how far to trust the time left: rough at first, then improving, then briefly "settled" before it goes away. */
function EtaNote({ p }: { p: Pick<Progress, "phase" | "etaSecs" | "etaConfidence"> }) {
  const conf = p.etaConfidence ?? 1;
  const [showSettled, setShowSettled] = useState(false);
  const wasRough = useRef(false);
  const settled = conf >= 0.85;
  useEffect(() => {
    if (!settled) {
      wasRough.current = true;
      setShowSettled(false);
      return;
    }
    if (!wasRough.current) return;
    setShowSettled(true);
    const t = setTimeout(() => setShowSettled(false), 3500);
    return () => clearTimeout(t);
  }, [settled]);
  if (p.phase !== "running" || p.etaSecs == null) return null;
  if (settled) return showSettled ? <span className="eta-note" data-level="good">Estimate settled</span> : null;
  return (
    <span className="eta-note" data-level={conf < 0.5 ? "rough" : "improving"} title="The time left is a guess until the transfer speed settles.">
      {conf < 0.5 ? "Rough estimate" : "Getting more accurate"}
    </span>
  );
}

const PRIORITY = ["low", "normal", "high"] as const;

/** Speed limit, priority and time limit of one transfer, changeable while it runs. */
function TransferControls({ p }: { p: Progress }) {
  const [speed, setSpeed] = useState(String(Math.round((p.limitBps ?? 0) / 1024)));
  const [minutes, setMinutes] = useState(String(Math.round((p.maxSecs ?? 0) / 60)));
  const [stop, setStop] = useState(useStore.getState().settings?.transfer.onTimeLimit === "stop");
  const apply = (what: "speed" | "time", v: string) => void api.transferAdjust(p.id, what, String(Math.max(0, Math.round(Number(v) || 0))), what === "time" ? stop : undefined);
  return (
    <dl className="details-list transfer-facts transfer-controls">
      <dt>Priority</dt>
      <dd>
        <select aria-label="Priority of this transfer" value={PRIORITY[p.priority ?? 1]} onChange={(e) => void api.transferAdjust(p.id, "priority", e.target.value)}>
          <option value="high">High: first in the queue</option>
          <option value="normal">Normal</option>
          <option value="low">Low: last, and gives way to other programs</option>
        </select>
      </dd>
      <dt>Speed limit</dt>
      <dd>
        <input type="number" min={0} step={256} aria-label="Speed limit in kilobytes per second" value={speed} onChange={(e) => setSpeed(e.target.value)} onBlur={() => apply("speed", speed)} onKeyDown={(e) => e.key === "Enter" && apply("speed", speed)} />
        <span className="muted"> KB/s ({Number(speed) > 0 ? `${(Number(speed) / 1024).toFixed(1)} MB/s` : "no limit"})</span>
      </dd>
      <dt>Time limit</dt>
      <dd>
        <input type="number" min={0} aria-label="Time limit in minutes" value={minutes} onChange={(e) => setMinutes(e.target.value)} onBlur={() => apply("time", minutes)} onKeyDown={(e) => e.key === "Enter" && apply("time", minutes)} />
        <span className="muted"> min, then </span>
        <select aria-label="At the time limit" value={stop ? "stop" : "pause"} onChange={(e) => { setStop(e.target.value === "stop"); void api.transferAdjust(p.id, "time", String(Math.round(Number(minutes) || 0)), e.target.value === "stop"); }}>
          <option value="pause">pause</option>
          <option value="stop">stop</option>
        </select>
        {p.timedOut && <span className="muted"> · limit reached</span>}
      </dd>
    </dl>
  );
}

/** A value that is looked at no more often than every `ms` milliseconds, so a fast stream of updates can be shown calmly. */
function useSampled<T>(value: T, ms: number): T {
  const [shown, setShown] = useState(value);
  const latest = useRef(value);
  latest.current = value;
  useEffect(() => {
    const t = setInterval(() => setShown(latest.current), ms);
    return () => clearInterval(t);
  }, [ms]);
  return shown;
}

/** Milliseconds between refreshes of the list of files: the setting, or what the performance preset says. */
function activeRefreshMs(settings: ReturnType<typeof useStore.getState>["settings"], tier: string | undefined): number {
  const chosen = adv<string>(settings, "transfer.activeFilesRefreshMs");
  if (chosen && chosen !== "auto") return Math.max(50, Number(chosen) || 250);
  return tier === "full" ? 200 : tier === "saver" ? 1000 : 400;
}

const clock = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });

function ActiveFiles({ p }: { p: Progress }) {
  const settings = useStore((s) => s.settings);
  const tier = useStore((s) => s.budget?.tier);
  const files = useSampled(p.activeFiles ?? [], activeRefreshMs(settings, tier));
  if (!files.length) return null;
  return (
    <ul className="active-files" aria-label="Files being copied now">
      {files.map((f) => (
        <li key={f.path} title={f.path}>
          <span className="file-name">{baseName(f.path)}</span>
          <span className="muted">{formatSize(f.size)}</span>
        </li>
      ))}
    </ul>
  );
}

function TransferDetail({ p, endedAt }: { p: Progress; endedAt?: number }) {
  const fraction = useStore((s) => s.shown[p.id] ?? 0);
  const series = useStore((s) => s.series[p.id]) ?? [];
  const settings = useStore((s) => s.settings);
  const on = (id: string) => metricOn(settings, id);
  const [more, setMore] = useState(true);
  const active = isActive({ kind: "transfer", p });
  const remainingItems = Math.max(0, p.filesTotal - p.filesDone);
  const counting = !!p.counting;
  const typical = p.prediction?.typicalSecs;
  const started = useRef((endedAt ?? Date.now()) - p.elapsedSecs * 1000);
  const methodName = p.method ? METHOD_NAME[p.method] : null;
  const current = p.current || p.activeFiles?.[0]?.path || "";
  const finishAt = active && p.phase === "running" && p.etaSecs != null ? clock(Date.now() + p.etaSecs * 1000) : null;
  const byFiles = p.filesTotal > 0 ? Math.round((p.filesDone / p.filesTotal) * 100) : 0;
  const byBytes = p.bytesTotal > 0 ? Math.round((p.bytesDone / p.bytesTotal) * 100) : 0;
  return (
    <div className="transfer-detail">
      <div className="percent">
        <strong>{counting ? `Counting files… ${formatCount(p.filesTotal)} found` : `${Math.round(fraction * 100)}% complete`}</strong>
        <span className="muted">{active ? (p.phase === "paused" ? "Paused" : formatRate(p)) : ""}</span>
      </div>
      <Bar fraction={fraction} indeterminate={p.phase === "scanning" || counting} />
      {on("graph") && (
        <>
          <SpeedGraph samples={series} total={typical ?? null} etaSecs={p.etaSecs} />
          <div className="graph-legend">
            <span className="files">● Files per second</span>
            <span className="bytes">● Megabytes per second</span>
            {typical ? <span className="typical">┆ Typical finish</span> : null}
          </div>
        </>
      )}
      {active && on("controls") && <TransferControls p={p} />}
      {on("activeFiles") && active && <ActiveFiles p={p} />}
      <dl className="details-list transfer-facts">
        {on("method") && methodName && (
          <>
            <dt>Method</dt>
            <dd>{methodName}</dd>
          </>
        )}
        {on("stage") && (
          <>
            <dt>Stage</dt>
            <dd>{p.phase === "scanning" ? "Counting files" : p.phase === "queued" ? "Waiting for its turn" : p.phase === "paused" ? "Paused" : p.phase === "running" ? "Copying" : p.phase}</dd>
          </>
        )}
        {on("name") && (
          <>
            <dt>Name</dt>
            <dd className="path">{current ? baseName(current) : "—"}</dd>
          </>
        )}
        {on("timeRemaining") && (
          <>
            <dt>Time remaining</dt>
            <dd className="eta-line">{counting ? "Counting files first…" : active ? formatEta(p.etaSecs).replace(" left", "") : "—"}<EtaNote p={p} /></dd>
          </>
        )}
        {on("finishAt") && (
          <>
            <dt>Finishes at</dt>
            <dd>{finishAt ?? "—"}</dd>
          </>
        )}
        {on("copied") && (
          <>
            <dt>Copied</dt>
            <dd>{formatSize(p.bytesDone)} of {formatSize(p.bytesTotal)}{counting ? " so far" : ""}</dd>
          </>
        )}
        {on("itemsRemaining") && (
          <>
            <dt>Items remaining</dt>
            <dd>{counting ? `${formatCount(remainingItems)} found so far` : `${formatCount(remainingItems)} (${formatSize(Math.max(0, p.bytesTotal - p.bytesDone))})`}</dd>
          </>
        )}
        {on("filesDone") && (
          <>
            <dt>Files done</dt>
            <dd>{formatCount(p.filesDone)} of {formatCount(p.filesTotal)}</dd>
          </>
        )}
        {on("progressByFiles") && (
          <>
            <dt>Progress</dt>
            <dd>{byFiles}% of the files · {byBytes}% of the data</dd>
          </>
        )}
        {on("speed") && (
          <>
            <dt>Speed</dt>
            <dd>{p.phase === "running" ? `${formatSize(p.bytesPerSec)}/s · ${p.filesPerSec >= 10 ? formatCount(p.filesPerSec) : p.filesPerSec.toFixed(1)} files/s` : "—"}</dd>
          </>
        )}
      </dl>
      <button className="link-button" onClick={() => setMore(!more)}>
        {more ? "Fewer details" : "More details"}
      </button>
      {more && (
        <dl className="details-list transfer-facts">
          {on("algorithm") && (
            <>
              <dt>Algorithm</dt>
              <dd>{p.strategy ? STRATEGY_LABEL[p.strategy] : "Choosing…"}{p.method && p.method !== "native" ? ` · via ${METHOD_LABEL[p.method] ?? p.method}` : ""}</dd>
            </>
          )}
          {on("streams") && p.plan && (
            <>
              <dt>Streams</dt>
              <dd>
                {p.plan.smallWorkers} small-file · {p.plan.largeWorkers} large-file · {formatSize(p.plan.bufferBytes)} buffer
                {p.plan.tryClone ? " · instant copy-on-write when possible" : ""}
              </dd>
            </>
          )}
          {on("from") && p.route && (
            <>
              <dt>From</dt>
              <dd>{p.route.srcName || p.route.src || "this computer"} <span className="muted">({MEDIA_LABEL[p.route.srcMedia]})</span></dd>
            </>
          )}
          {on("to") && p.route && (
            <>
              <dt>To</dt>
              <dd>{p.route.dstName || p.route.dst || "this computer"} <span className="muted">({MEDIA_LABEL[p.route.dstMedia]})</span></dd>
            </>
          )}
          {on("destFolder") && (
            <>
              <dt>Destination folder</dt>
              <dd className="path">{p.destDir}</dd>
            </>
          )}
          {on("totalSize") && (
            <>
              <dt>Total size</dt>
              <dd>{formatSize(p.bytesTotal)} in {formatCount(p.filesTotal)} files</dd>
            </>
          )}
          {on("avgFileSize") && p.filesTotal > 0 && (
            <>
              <dt>Average file size</dt>
              <dd>{formatSize(p.bytesTotal / p.filesTotal)}</dd>
            </>
          )}
          {on("startedAt") && (
            <>
              <dt>Started at</dt>
              <dd>{clock(started.current)}</dd>
            </>
          )}
          {on("typicalTime") && (
            <>
              <dt>Typical time</dt>
              <dd>
                {p.prediction
                  ? `${formatDuration(p.prediction.typicalSecs)} for similar transfers (${p.prediction.samples} past${p.prediction.sameRoute ? ", including this route" : ""})`
                  : "No earlier transfer on this route yet; this one will be remembered"}
              </dd>
            </>
          )}
          {on("confidence") && (
            <>
              <dt>Estimate confidence</dt>
              <dd>{Math.round((p.etaConfidence ?? 0) * 100)}%</dd>
            </>
          )}
          {on("averageSpeed") && (
            <>
              <dt>Average speed</dt>
              <dd>{formatSize(p.avgBytesPerSec)}/s · {p.avgFilesPerSec.toFixed(1)} files/s</dd>
            </>
          )}
          {on("peakSpeed") && (
            <>
              <dt>Peak speed</dt>
              <dd>{formatSize(p.peakBytesPerSec)}/s · {p.peakFilesPerSec.toFixed(1)} files/s</dd>
            </>
          )}
          {on("timeSplit") && (
            <>
              <dt>Time spent on</dt>
              <dd>{Math.round(p.overheadShare * 100)}% per-file work, {Math.round((1 - p.overheadShare) * 100)}% moving data</dd>
            </>
          )}
          {on("skippedErrors") && (
            <>
              <dt>Skipped / errors</dt>
              <dd>{formatCount(p.skipped)} skipped · {formatCount(p.errorCount)} failed</dd>
            </>
          )}
          {on("elapsed") && (
            <>
              <dt>Elapsed</dt>
              <dd>{formatDuration(p.elapsedSecs)}</dd>
            </>
          )}
          {on("nowAt") && current && (
            <>
              <dt>Now at</dt>
              <dd className="path">{current}</dd>
            </>
          )}
        </dl>
      )}
    </div>
  );
}

function DeleteDetail({ p }: { p: DeleteProgress }) {
  const fraction = useStore((s) => s.shown[p.id] ?? 0);
  const series = useStore((s) => s.series[p.id]) ?? [];
  const [more, setMore] = useState(true);
  const stats = p.stats;
  const active = isActive({ kind: "delete", p });
  const typical = stats?.typicalSecs ?? null;
  const hasBytes = p.bytesTotal > 0 || (stats?.peakBytesPerSec ?? 0) > 0;
  const noun = p.action === "delete" || p.action === "remote-delete" ? "Items" : p.action === "compress" || p.action === "extract" ? "Files" : "Items";
  return (
    <div className="transfer-detail">
      <div className="percent">
        <strong>{p.phase === "scanning" ? (p.action === "delete" || p.action === "remote-delete" ? "Counting items…" : "Getting ready…") : `${Math.round(fraction * 100)}% complete`}</strong>
        <span className="muted">{active && stats && p.phase === "running" ? jobRate(stats) : ""}</span>
      </div>
      <Bar fraction={fraction} indeterminate={p.phase === "scanning"} />
      {stats && (
        <>
          <SpeedGraph samples={series} total={typical} etaSecs={stats.etaSecs} bytes={hasBytes} />
          <div className="graph-legend">
            <span className="files">● Items per second</span>
            {hasBytes && <span className="bytes">● Megabytes per second</span>}
            {typical ? <span className="typical">┆ Typical finish</span> : null}
          </div>
        </>
      )}
      <dl className="details-list transfer-facts">
        {stats && (
          <>
            <dt>Time remaining</dt>
            <dd className="eta-line">{active ? formatEta(stats.etaSecs).replace(" left", "") : "—"}<EtaNote p={{ phase: p.phase, etaSecs: stats.etaSecs, etaConfidence: stats.etaConfidence }} /></dd>
          </>
        )}
        {p.bytesTotal > 0 && (
          <>
            <dt>{p.action === "delete" ? "Removed" : "Copied"}</dt>
            <dd>{formatSize(p.bytesDone)} of {formatSize(p.bytesTotal)}</dd>
          </>
        )}
        {(p.action === "delete" || p.action === "remote-delete") && (
          <>
            <dt>Items</dt>
            <dd>{formatCount(p.itemsDone)} of {formatCount(p.itemsTotal)}</dd>
            {p.action === "delete" && (
              <>
                <dt>Method</dt>
                <dd>{p.permanent ? "Permanent delete" : "Moved to the Recycle Bin / Trash"}</dd>
              </>
            )}
          </>
        )}
        {(p.action === "compress" || p.action === "extract") && p.itemsTotal > 0 && (
          <>
            <dt>{noun}</dt>
            <dd>{formatCount(p.itemsDone)} of {formatCount(p.itemsTotal)}</dd>
          </>
        )}
        <dt>Elapsed</dt>
        <dd>{formatDuration(p.elapsedSecs)}</dd>
        {p.current && (
          <>
            <dt>Now at</dt>
            <dd className="path">{p.current}</dd>
          </>
        )}
      </dl>
      {stats && (
        <>
          <button className="link-button" onClick={() => setMore(!more)}>
            {more ? "Fewer details" : "More details"}
          </button>
          {more && (
            <dl className="details-list transfer-facts">
              <dt>Typical time</dt>
              <dd>
                {typical != null
                  ? `${formatDuration(typical)} for similar jobs (${stats.samples} past)`
                  : "No similar jobs recorded yet; this one will be remembered"}
              </dd>
              <dt>Average speed</dt>
              <dd>{jobRate({ itemsPerSec: stats.avgItemsPerSec, bytesPerSec: stats.avgBytesPerSec })}</dd>
              <dt>Peak speed</dt>
              <dd>{jobRate({ itemsPerSec: stats.peakItemsPerSec, bytesPerSec: stats.peakBytesPerSec })}</dd>
            </dl>
          )}
        </>
      )}
    </div>
  );
}

function ReattachButton() {
  return (
    <button className="secondary reattach" title="Put this back inside the main window" onClick={() => void api.reattach("transfers").catch((e) => useStore.getState().showToast(String(e)))}>
      <Icon name="popin" size={13} /> Reattach to window
    </button>
  );
}

/** Save a finished job as a file: a page with its graph, a text report, or data. */
async function saveReport(j: Job) {
  const st = useStore.getState();
  const op = j.kind === "transfer" ? st.specs[j.p.id]?.op : undefined;
  const r = reportOf(j, st.series[j.p.id] ?? [], op);
  try {
    const where = await api.saveTextFile(r.title.slice(0, 60), reportFiles(r));
    if (where) st.showToast(`Saved to ${where}`);
  } catch (e) {
    st.showToast(String(e));
  }
}

function Window({ jobs, detached, past }: { jobs: Job[]; detached?: boolean; past?: boolean }) {
  const [picked, setPicked] = useState<string | null>(past ? useStore.getState().finishedPick : null);
  const spec = useStore((s) => s.specs);
  const { dismissTransfer, dismissDelete, forgetFinished } = useStore.getState();
  const keyOf = (j: Job) => `${j.kind}-${j.p.id}`;
  const open = jobs.find((j) => keyOf(j) === picked) ?? jobs.find(isActive) ?? jobs[0];
  const title = past ? `Finished (${jobs.length})` : jobs.length === 1 ? titleOf(jobs[0]) : `${jobs.length} jobs`;
  return (
    <div className="transfer-window" data-detached={detached || undefined} role="dialog" aria-label="Transfers">
      <div className="transfer-window-head">
        <strong>{title}</strong>
        {detached && <ReattachButton />}
        {past && (
          <button className="link-button" onClick={() => forgetFinished()} title="Forget these until the app is closed anyway">
            Clear
          </button>
        )}
        {!detached && (
          <span className="transfer-window-buttons">
            {!past && <button
              className="icon-button"
              aria-label="Open in its own window"
              title="Open in its own window: move it anywhere and resize it"
              onClick={() => {
                useStore.setState({ transfersOpen: false });
                void api.openTransfersWindow().catch((e) => useStore.getState().showToast(String(e)));
              }}
            >
              <Icon name="popout" size={13} />
            </button>}
            <button className="icon-button" aria-label={past ? "Close" : "Collapse to small cards"} title={past ? "Close" : "Collapse"} onClick={() => useStore.setState(past ? { finishedOpen: false } : { transfersOpen: false })}>
              <Icon name="collapse" size={13} />
            </button>
          </span>
        )}
      </div>
      <div className="transfer-window-body">
        {jobs.map((j) => {
          const expanded = keyOf(j) === keyOf(open);
          const active = isActive(j);
          return (
            <section key={keyOf(j)} className="transfer-section" data-open={expanded || undefined}>
              <div className="transfer-row" onClick={() => setPicked(keyOf(j))}>
                <Icon name={expanded ? "expand" : "chevron"} size={11} />
                <span className="grow file-name">{titleOf(j)}</span>
                {j.at && <span className="muted transfer-ended" title="When it finished">{new Date(j.at).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}</span>}
                {!expanded && active && <SmallBar job={j} />}
                {j.kind === "transfer" && active && j.p.phase !== "scanning" && j.p.phase !== "queued" && (
                  <button
                    className="icon-button"
                    aria-label={j.p.phase === "paused" ? "Resume" : "Pause"}
                    title={j.p.phase === "paused" ? "Resume" : "Pause"}
                    onClick={(e) => (e.stopPropagation(), void api.transferControl(j.p.id, j.p.phase === "paused" ? "resume" : "pause"))}
                  >
                    <Icon name={j.p.phase === "paused" ? "play" : "pause"} size={12} />
                  </button>
                )}
                {j.at && (
                  <button className="link-button" aria-label="Save as a file" title="Save this as a file: a page with the graph, a text report, or data" onClick={(e) => (e.stopPropagation(), void saveReport(j))}>
                    Save…
                  </button>
                )}
                <button
                  className="icon-button"
                  data-action={active ? "cancel" : "dismiss"}
                  aria-label={active ? "Cancel" : "Dismiss"}
                  title={active ? "Cancel (Shift-click to force stop)" : j.at ? "Forget this" : "Dismiss"}
                  onClick={(e) => (e.stopPropagation(), active ? cancel(j, e.shiftKey) : j.at ? forgetFinished(keyOf(j)) : j.kind === "delete" ? dismissDelete(j.p.id) : dismissTransfer(j.p.id))}
                >
                  <Icon name="close" size={12} />
                </button>
              </div>
              {active && <ForceStop j={j} />}
              {expanded && (j.kind === "transfer" ? <TransferDetail p={j.p} endedAt={j.at} /> : <DeleteDetail p={j.p} />)}
              <Issues
                p={j.p}
                retry={j.kind === "transfer" && !active && j.p.errors.some((e) => e.needsElevation) && spec[j.p.id] ? () => { const s = spec[j.p.id]; dismissTransfer(j.p.id); void retryTransferAsAdmin(s); } : undefined}
              />
            </section>
          );
        })}
      </div>
    </div>
  );
}

function SmallBar({ job }: { job: Job }) {
  const f = useFraction(job);
  return (
    <div className="mini-meter">
      <div style={{ width: `${f * 100}%` }} />
    </div>
  );
}

function useJobs(): Job[] {
  const transfers = useStore((s) => s.transfers);
  const deletes = useStore((s) => s.deletes);
  return useMemo<Job[]>(
    () =>
      [
        ...Object.values(transfers).map((p) => ({ kind: "transfer" as const, p })),
        ...Object.values(deletes).map((p) => ({ kind: "delete" as const, p })),
      ].sort((a, b) => a.p.id - b.p.id),
    [transfers, deletes],
  );
}

/** Jobs that ended cleanly, newest first. They stay in memory until the app is closed. */
function useFinished(): Job[] {
  const finished = useStore((s) => s.finished);
  return useMemo<Job[]>(() => Object.values(finished).sort((a, b) => b.at - a.at) as Job[], [finished]);
}

/** The content of the separate transfers window. */
export function DetachedTransfers() {
  const jobs = useJobs();
  const past = useFinished();
  if (!jobs.length && past.length) return <Window jobs={past} detached past />;
  if (!jobs.length) {
    return (
      <div className="transfer-window" data-detached>
        <div className="transfer-window-head">
          <strong>Transfers</strong>
          <ReattachButton />
        </div>
        <p className="muted" style={{ padding: 16 }}>
          Nothing is copying or deleting right now. Jobs appear here as they start and stay until finished.
        </p>
      </div>
    );
  }
  return <Window jobs={jobs} detached />;
}

/** Pastes waiting for a delete they would collide with. */
function Waiting() {
  const queued = useStore((s) => s.queuedJobs);
  const starting = useStore((s) => s.startingJobs);
  const entries = Object.entries(queued);
  if (!entries.length && !Object.keys(starting).length) return null;
  return (
    <div className="transfer-window transfer-queued" role="status">
      {Object.entries(starting).map(([id, q]) => (
        <section key={`s${id}`} className="transfer-section">
          <div className="transfer-row">
            <div className="loading-bar" style={{ width: 28 }} />
            <span className="grow file-name">
              {q.title}
              <span className="muted"> · Starting…</span>
            </span>
          </div>
        </section>
      ))}
      {entries.map(([id, q]) => (
        <section key={id} className="transfer-section">
          <div className="transfer-row">
            <Icon name="info" size={12} />
            <span className="grow file-name">
              {q.title}
              <span className="muted"> · {q.detail}</span>
            </span>
            <button className="icon-button" aria-label="Remove from the line" title="Remove from the line" onClick={() => useStore.getState().unqueue(Number(id))}>
              <Icon name="close" size={12} />
            </button>
          </div>
        </section>
      ))}
    </div>
  );
}

export function Transfers() {
  const open = useStore((s) => s.transfersOpen);
  const detached = useStore((s) => s.detached.transfers);
  const waiting = useStore((s) => Object.keys(s.queuedJobs).length + Object.keys(s.startingJobs).length > 0);
  const jobs = useJobs();
  const past = useFinished();
  const pastOpen = useStore((s) => s.finishedOpen);
  if (detached) return null;
  // Nothing running: just a small way back to what finished, until the app is closed.
  if (!jobs.length && !waiting) {
    if (!past.length) return null;
    return (
      <div className="transfers" data-open={pastOpen || undefined}>
        {pastOpen ? (
          <Window jobs={past} past />
        ) : (
          <button className="transfer-recent" onClick={() => useStore.setState({ finishedOpen: true })} title="Look again at the graphs and details of what finished">
            <Icon name="clock" size={12} /> Finished ({past.length})
          </button>
        )}
      </div>
    );
  }
  return (
    <div className="transfers" data-open={open || undefined} aria-live="polite">
      {waiting && <Waiting />}
      {!jobs.length ? null : open ? <Window jobs={jobs} /> : jobs.map((j) => <Card key={`${j.kind}-${j.p.id}`} job={j} />)}
    </div>
  );
}
