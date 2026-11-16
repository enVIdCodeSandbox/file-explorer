// A finished copy, move, delete… written out as a file: a page with the speed graph, plain text, or data.

import type { DeleteProgress, Progress } from "../api";
import { formatDuration, formatSize } from "./format";
import type { Sample } from "../store";

export type ReportJob = ({ kind: "transfer"; p: Progress } | { kind: "delete"; p: DeleteProgress }) & { at?: number };

export interface Report {
  title: string;
  facts: [string, string][];
  samples: Sample[];
}

const pad2 = (n: number) => String(n).padStart(2, "0");
function stamp(ms: number): string {
  const d = new Date(ms);
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())} ${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}`;
}

const ACTION: Record<string, string> = {
  delete: "Delete", compress: "Compress", extract: "Extract", upload: "Upload", download: "Download", open: "Download to open",
  unpack: "Unpack", pack: "Pack", "remote-delete": "Delete on a server", update: "Update", "rename-copy": "Rename by copying",
};

/** What to say about a job: a title, label/value facts, and the speed samples behind its graph. */
export function reportOf(job: ReportJob, samples: Sample[], op: string = "copy"): Report {
  const end = job.at ?? Date.now();
  const f: [string, string][] = [];
  if (job.kind === "transfer") {
    const p = job.p;
    f.push(["Operation", op === "move" ? "Move" : "Copy"]);
    f.push(["To", p.destDir]);
    f.push(["Files", `${p.filesDone} of ${p.filesTotal}`]);
    f.push(["Size", `${formatSize(p.bytesDone)} of ${formatSize(p.bytesTotal)}`]);
    f.push(["Result", p.phase === "done" ? "Completed" : p.phase === "cancelled" ? "Cancelled" : p.phase]);
    f.push(["Started", stamp(end - p.elapsedSecs * 1000)]);
    f.push(["Finished", stamp(end)]);
    f.push(["Elapsed", formatDuration(p.elapsedSecs)]);
    f.push(["Average speed", `${formatSize(p.avgBytesPerSec)}/s, ${p.avgFilesPerSec.toFixed(1)} files/s`]);
    f.push(["Peak speed", `${formatSize(p.peakBytesPerSec)}/s, ${p.peakFilesPerSec.toFixed(1)} files/s`]);
    if (p.method) f.push(["Method", p.method]);
    if (p.strategy) f.push(["Algorithm", p.strategy]);
    if (p.route) f.push(["Route", `${p.route.srcName || p.route.src || "this computer"} (${p.route.srcMedia}) to ${p.route.dstName || p.route.dst || "this computer"} (${p.route.dstMedia})`]);
    if (p.plan) f.push(["Streams", `${p.plan.smallWorkers} small-file, ${p.plan.largeWorkers} large-file, ${formatSize(p.plan.bufferBytes)} buffer`]);
    f.push(["Skipped", String(p.skipped)]);
    f.push(["Failed", String(p.errorCount)]);
    const title = `${op === "move" ? "Move" : "Copy"} to ${p.destDir}`;
    return { title, facts: f, samples };
  }
  const p = job.p;
  const what = ACTION[p.action] ?? p.action;
  f.push(["Operation", what + (p.action === "delete" ? (p.permanent ? " (permanent)" : " (to the Recycle Bin or Trash)") : "")]);
  if (p.firstName) f.push(["First item", p.firstName]);
  f.push(["Items", `${p.itemsDone} of ${p.itemsTotal}`]);
  if (p.bytesTotal > 0) f.push(["Size", `${formatSize(p.bytesDone)} of ${formatSize(p.bytesTotal)}`]);
  f.push(["Result", p.phase === "done" ? "Completed" : p.phase === "cancelled" ? "Cancelled" : p.phase]);
  f.push(["Started", stamp(end - p.elapsedSecs * 1000)]);
  f.push(["Finished", stamp(end)]);
  f.push(["Elapsed", formatDuration(p.elapsedSecs)]);
  if (p.stats) {
    f.push(["Average speed", `${formatSize(p.stats.avgBytesPerSec)}/s, ${p.stats.avgItemsPerSec.toFixed(1)} items/s`]);
    f.push(["Peak speed", `${formatSize(p.stats.peakBytesPerSec)}/s, ${p.stats.peakItemsPerSec.toFixed(1)} items/s`]);
  }
  if (p.method) f.push(["Method", p.method]);
  if (p.result) f.push(["Result file", p.result]);
  f.push(["Skipped", String(p.skipped ?? 0)]);
  f.push(["Failed", String(p.errorCount)]);
  return { title: p.firstName ? `${what}: ${p.firstName}${p.selected > 1 ? ` and ${p.selected - 1} more` : ""}` : what, facts: f, samples };
}

export function reportText(r: Report): string {
  const w = Math.max(...r.facts.map(([k]) => k.length));
  const lines = [r.title, "=".repeat(Math.min(r.title.length, 78)), "", ...r.facts.map(([k, v]) => `${(k + ":").padEnd(w + 2)}${v}`)];
  if (r.samples.length) {
    lines.push("", "Speed over time", "seconds  files/s  bytes/s");
    for (const s of r.samples) lines.push(`${s.t.toFixed(1).padStart(7)}  ${s.filesPerSec.toFixed(1).padStart(7)}  ${Math.round(s.bytesPerSec).toString().padStart(9)}`);
  }
  return lines.join("\n") + "\n";
}

export function reportJson(r: Report): string {
  return JSON.stringify({ title: r.title, facts: Object.fromEntries(r.facts), speed: r.samples.map((s) => ({ seconds: s.t, filesPerSec: s.filesPerSec, bytesPerSec: s.bytesPerSec })) }, null, 2) + "\n";
}

const esc = (t: string) => t.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

/** One self-contained page: the facts and the speed graph. */
export function reportHtml(r: Report): string {
  const W = 720;
  const H = 220;
  let svg = "";
  if (r.samples.length >= 2) {
    const tMax = Math.max(...r.samples.map((s) => s.t), 0.1);
    const line = (pick: (s: Sample) => number, color: string) => {
      const max = Math.max(...r.samples.map(pick), 1e-9);
      const pts = r.samples.map((s) => `${((s.t / tMax) * W).toFixed(1)},${(H - (pick(s) / max) * (H - 10) - 2).toFixed(1)}`).join(" ");
      return `<polyline points="${pts}" fill="none" stroke="${color}" stroke-width="2"/>`;
    };
    svg = `<svg viewBox="0 0 ${W} ${H}" width="100%" role="img" aria-label="Speed over time" style="background:#fafafa;border:1px solid #ddd;border-radius:6px">${line((s) => s.bytesPerSec, "#2b7de9")}${line((s) => s.filesPerSec, "#e08a00")}</svg>
<p style="font-size:12px;color:#555"><span style="color:#2b7de9">&#9679; Megabytes per second</span> &nbsp; <span style="color:#e08a00">&#9679; Files per second</span> &nbsp; (each line scaled to its own peak; the time axis runs 0 to ${esc(formatDuration(Math.max(...r.samples.map((s) => s.t))))})</p>`;
  }
  const rows = r.facts.map(([k, v]) => `<tr><th>${esc(k)}</th><td>${esc(v)}</td></tr>`).join("\n");
  return `<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><title>${esc(r.title)}</title>
<style>body{font:14px system-ui,sans-serif;max-width:760px;margin:32px auto;padding:0 16px;color:#222}h1{font-size:20px}table{border-collapse:collapse;width:100%;margin-top:16px}th{text-align:left;width:160px;color:#555;font-weight:600}th,td{padding:5px 8px;border-bottom:1px solid #eee;vertical-align:top;word-break:break-word}</style></head>
<body><h1>${esc(r.title)}</h1>
${svg}
<table>
${rows}
</table>
<p style="color:#888;font-size:12px;margin-top:24px">Saved from FAST File Explorer.</p>
</body></html>
`;
}

/** What to offer in a Save dialog for a finished job. */
export function reportFiles(r: Report) {
  return [
    { label: "Web page with graph", ext: "html", contents: reportHtml(r) },
    { label: "Text report", ext: "txt", contents: reportText(r) },
    { label: "Data (JSON)", ext: "json", contents: reportJson(r) },
  ];
}
