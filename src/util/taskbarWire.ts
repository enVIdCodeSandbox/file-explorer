// Feeds the app's own progress (transfers, deletes and archive jobs, and automated tasks) to the icon.

import { getCurrentWindow, ProgressBarStatus } from "@tauri-apps/api/window";
import { on } from "../api";
import { useStore } from "../store";
import { barFor, sameBar, type Bar, type Work } from "./taskbar";

const STATUS: Record<Bar["status"], ProgressBarStatus> = {
  none: ProgressBarStatus.None,
  indeterminate: ProgressBarStatus.Indeterminate,
  normal: ProgressBarStatus.Normal,
  paused: ProgressBarStatus.Paused,
  error: ProgressBarStatus.Error,
};

export function startTaskbarProgress(): () => void {
  if (!("__TAURI_INTERNALS__" in window)) return () => {};
  const win = getCurrentWindow();
  const tasks = new Map<string, Work>();
  let shown: Bar | null = null;
  let active = new Set<string>();
  let errorUntil = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const update = () => {
    const s = useStore.getState();
    const work: Work[] = [];
    const ids = new Set<string>();
    for (const t of Object.values(s.transfers)) {
      if (t.phase !== "queued" && t.phase !== "scanning" && t.phase !== "running" && t.phase !== "paused") continue;
      ids.add(`t${t.id}`);
      const bytes = t.bytesTotal > 0;
      work.push({ done: bytes ? t.bytesDone : t.filesDone, total: bytes ? t.bytesTotal : t.filesTotal, paused: t.phase === "paused" });
    }
    for (const d of Object.values(s.deletes)) {
      if (d.phase !== "scanning" && d.phase !== "running") continue;
      ids.add(`d${d.id}`);
      work.push({ done: d.itemsDone, total: d.itemsTotal });
    }
    for (const [id, w] of tasks) (ids.add(`k${id}`), work.push(w));
    // Something that was running has ended: red for a few seconds if it failed.
    for (const id of active) {
      if (ids.has(id) || !id.startsWith("t")) continue;
      if (s.transfers[Number(id.slice(1))]?.phase === "failed") errorUntil = Date.now() + 5000;
    }
    active = ids;
    const failed = !work.length && Date.now() < errorUntil;
    if (failed && !timer) timer = setTimeout(() => ((timer = undefined), update()), errorUntil - Date.now() + 50);
    const bar = barFor(work, failed);
    if (sameBar(shown, bar)) return;
    shown = bar;
    void win.setProgressBar({ status: STATUS[bar.status], progress: bar.progress }).catch(() => {});
  };

  const offs = [
    useStore.subscribe((s, prev) => (s.transfers !== prev.transfers || s.deletes !== prev.deletes) && update()),
    on("task-started", (e) => (tasks.set(e.id, { done: 0, total: 0 }), update())),
    on("task-progress", (e) => {
      const p = e.progress;
      tasks.set(e.id, p.bytesTotal > 0 ? { done: p.bytesDone, total: p.bytesTotal } : p.itemsTotal > 0 ? { done: p.itemsDone, total: p.itemsTotal } : { done: p.step, total: p.steps });
      update();
    }),
    on("task-finished", (e) => (tasks.delete(e.taskId), update())),
  ];
  update();
  return () => {
    offs.forEach((o) => (typeof o === "function" ? o() : void o.then((f) => f())));
    if (timer) clearTimeout(timer);
    void win.setProgressBar({ status: ProgressBarStatus.None }).catch(() => {});
  };
}
