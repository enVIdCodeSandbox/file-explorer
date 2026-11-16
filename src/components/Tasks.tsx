// The Tasks window: automated jobs on files and folders, like a cron job with a friendlier face.
// A list of tasks on the left; for the chosen one, an overview with its statistics, the editor, and
// the record of its runs. The scheduler lives in the backend (src-tauri/src/tasks.rs), so tasks run
// whether or not this window is open (while the app is).

import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { Fragment, useCallback, useEffect, useMemo, useState } from "react";
import { api, on } from "../api";
import { ask, useStore } from "../store";
import {
  ACTION_TYPES,
  OUTCOME_LABEL,
  TRIGGER_TYPES,
  WEEKDAYS,
  defaultOptions,
  describeAction,
  describeRun,
  describeTrigger,
  newAction,
  newTask,
  newTrigger,
  validate,
  type Action,
  type ActionType,
  type Conflict,
  type Filter,
  type Options,
  type Preview,
  type Progress,
  type RunRecord,
  type Stats,
  type StatsReply,
  type Task,
  type TasksListing,
  type Trigger,
  type TriggerType,
} from "../tasks/model";
import { formatDate, formatDuration, formatSize } from "../util/format";
import { Icon } from "./Icons";

type Tab = "overview" | "edit" | "history";

const say = (e: unknown) => useStore.getState().showToast(String(e));

/** Milliseconds as the value of a datetime-local box, and back. */
const toLocalInput = (ms: number) => {
  const d = new Date(ms - new Date(ms).getTimezoneOffset() * 60000);
  return d.toISOString().slice(0, 16);
};
const fromLocalInput = (v: string) => (v ? new Date(v).getTime() : Date.now());

const rate = (bps: number) => (bps > 0 ? `${formatSize(bps)}/s` : "—");
const pct = (x: number) => `${Math.round(x * 100)}%`;
const when = (ms: number | null | undefined) => (ms ? formatDate(ms) : "never");

export function TasksView() {
  const [listing, setListing] = useState<TasksListing | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("overview");
  const [draft, setDraft] = useState<Task | null>(null);
  const [live, setLive] = useState<Record<string, Progress>>({});
  const prefill = useStore((s) => s.tasksPrefill);
  const close = () => useStore.setState({ tasksOpen: false, tasksPrefill: null });

  const reload = useCallback(() => {
    api.tasksList().then(setListing).catch(say);
  }, []);

  useEffect(() => {
    reload();
    const offs = [
      on("tasks-changed", reload),
      on("task-finished", reload),
      on("task-started", reload),
      on("task-progress", (e) => setLive((l) => ({ ...l, [e.id]: e.progress }))),
    ];
    return () => {
      offs.forEach((p) => void p.then((f) => f()));
    };
  }, [reload]);

  // "Automate…" from a right-click menu arrives here with a task already started.
  useEffect(() => {
    if (!prefill) return;
    setDraft(prefill);
    setSelected("new");
    setTab("edit");
    useStore.setState({ tasksPrefill: null });
  }, [prefill]);

  const tasks = listing?.tasks ?? [];
  const running = useMemo(() => new Map((listing?.running ?? []).map((r) => [r.id, r])), [listing]);
  const current = selected === "new" ? draft : tasks.find((t) => t.id === selected) ?? null;

  const startNew = () => {
    setDraft(newTask());
    setSelected("new");
    setTab("edit");
  };
  const choose = (id: string) => {
    setSelected(id);
    setDraft(null);
    setTab("overview");
  };

  const exportAll = async () => {
    try {
      const file = await saveDialog({ defaultPath: "fast-tasks.json", filters: [{ name: "Task list", extensions: ["json"] }] });
      if (!file) return;
      const n = await api.tasksExport(file);
      useStore.getState().showToast(`Saved ${n} task${n === 1 ? "" : "s"}.`);
    } catch (e) {
      say(e);
    }
  };
  const importAll = async () => {
    try {
      const file = await openDialog({ multiple: false, directory: false, filters: [{ name: "Task list", extensions: ["json"] }] });
      if (typeof file !== "string") return;
      const n = await api.tasksImport(file);
      useStore.getState().showToast(`Added ${n} task${n === 1 ? "" : "s"}. They start turned off: check each one, then turn it on.`);
      reload();
    } catch (e) {
      say(e);
    }
  };

  const off = listing && !listing.enabled;

  return (
    <div className="modal-backdrop" onMouseDown={close}>
      <div
        className="tasks"
        role="dialog"
        aria-modal="true"
        aria-label="Tasks"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape" && !(e.target instanceof HTMLInputElement && e.target.type === "text")) close();
          e.stopPropagation();
        }}
      >
        <aside className="tasks-nav">
          <div className="tasks-nav-head">
            <h2>Tasks</h2>
            <button className="icon-button" aria-label="Close Tasks" onClick={close}>
              <Icon name="close" />
            </button>
          </div>
          <button className="primary tasks-new" onClick={startNew}>
            <Icon name="plus" size={14} /> New task
          </button>
          <button className={"side-item" + (selected === null ? " tasks-all" : "")} data-active={selected === null || undefined} onClick={() => (setSelected(null), setDraft(null))}>
            <Icon name="chart" size={14} /> All tasks
          </button>
          <div className="tasks-list" role="list">
            {tasks.length === 0 && !draft && <p className="muted tasks-empty">No tasks yet. A task does something with files on a schedule or when something happens: back up a folder every night, clear old downloads, move photos off a card when it is plugged in.</p>}
            {selected === "new" && draft && (
              <div className="task-row" data-active role="listitem">
                <span className="task-dot" data-state="draft" />
                <span className="task-row-text">
                  <strong>{draft.name || "New task"}</strong>
                  <span className="muted">Not saved yet</span>
                </span>
              </div>
            )}
            {tasks.map((t) => {
              const r = running.get(t.id);
              const last = listing?.last[t.id];
              const next = listing?.next[t.id];
              const state = r ? "running" : !t.enabled ? "off" : last?.outcome === "failed" || last?.outcome === "partial" ? "bad" : "ok";
              return (
                <button key={t.id} className="task-row" role="listitem" data-active={selected === t.id || undefined} onClick={() => choose(t.id)}>
                  <span className="task-dot" data-state={state} />
                  <span className="task-row-text">
                    <strong>{t.name}</strong>
                    <span className="muted">
                      {r ? `Running · ${live[t.id] ? progressText(live[t.id]) : "starting"}` : !t.enabled ? "Turned off" : t.resumePending ? "Will carry on at the next start" : next ? `Next: ${formatDate(next)}` : last ? describeRun(last) : "Waiting for its trigger"}
                    </span>
                  </span>
                </button>
              );
            })}
          </div>
          <div className="tasks-nav-foot">
            <button className="secondary" onClick={() => void importAll()}>Import…</button>
            <button className="secondary" disabled={tasks.length === 0} onClick={() => void exportAll()}>Export…</button>
            <button className="secondary" onClick={() => {
              useStore.setState({ tasksOpen: false, settingsQuery: "task" });
              useStore.getState().openSettings();
            }}>
              <Icon name="settings" size={13} /> Settings
            </button>
          </div>
        </aside>
        <section className="tasks-main">
          {off && (
            <div className="tasks-banner" role="status">
              <span>Automated tasks are turned off, so nothing will run by itself.</span>
              <button
                className="secondary"
                onClick={() => {
                  useStore.getState().updateSettings((s) => {
                    s.advanced["tasks.enabled"] = true;
                  });
                  window.setTimeout(reload, 400);
                }}
              >
                Turn on
              </button>
            </div>
          )}
          {selected === null ? (
            <AllTasks tasks={tasks} listing={listing} onPick={choose} />
          ) : current ? (
            <>
              <header className="tasks-head">
                <h2>{current.name || "New task"}</h2>
                <div className="tasks-tabs" role="tablist">
                  {(["overview", "edit", "history"] as Tab[]).map((t) => (
                    <button key={t} role="tab" aria-selected={tab === t} data-active={tab === t || undefined} disabled={selected === "new" && t !== "edit"} onClick={() => setTab(t)}>
                      {t === "overview" ? "Overview" : t === "edit" ? "Edit" : "History"}
                    </button>
                  ))}
                </div>
              </header>
              <div className="tasks-body">
                {tab === "overview" && selected !== "new" && <Overview task={current} running={running.get(current.id)} progress={live[current.id]} next={listing?.next[current.id]} onEdit={() => setTab("edit")} onChanged={reload} onGone={() => (setSelected(null), reload())} />}
                {tab === "edit" && (
                  <Editor
                    key={current.id || "new"}
                    task={current}
                    isNew={selected === "new"}
                    onChange={(t) => (selected === "new" ? setDraft(t) : setListing((l) => (l ? { ...l, tasks: l.tasks.map((x) => (x.id === t.id ? t : x)) } : l)))}
                    onSaved={(t, run) => {
                      setDraft(null);
                      setSelected(t.id);
                      setTab("overview");
                      reload();
                      if (run) void api.tasksRun(t.id).catch(say);
                    }}
                    onDiscard={() => {
                      if (selected === "new") (setDraft(null), setSelected(null));
                      else (reload(), setTab("overview"));
                    }}
                  />
                )}
                {tab === "history" && selected !== "new" && <History id={current.id} />}
              </div>
            </>
          ) : (
            <p className="muted tasks-empty">That task is gone.</p>
          )}
        </section>
      </div>
    </div>
  );
}

function progressText(p: Progress): string {
  if (p.bytesTotal > 0) return `${Math.min(100, Math.round((p.bytesDone / p.bytesTotal) * 100))}%`;
  if (p.itemsTotal > 0) return `${p.itemsDone} of ${p.itemsTotal}`;
  return "working";
}

// ---------------------------------------------------------------------------------------------
// All tasks: totals across everything
// ---------------------------------------------------------------------------------------------

function AllTasks({ tasks, listing, onPick }: { tasks: Task[]; listing: TasksListing | null; onPick: (id: string) => void }) {
  const [reply, setReply] = useState<StatsReply | null>(null);
  const [recent, setRecent] = useState<RunRecord[]>([]);
  useEffect(() => {
    api.tasksStats(null).then(setReply).catch(() => {});
    api.tasksHistory(null, 25).then(setRecent).catch(() => {});
  }, [listing]);
  const s = reply?.stats;
  return (
    <>
      <header className="tasks-head">
        <h2>All tasks</h2>
      </header>
      <div className="tasks-body">
        <div className="stat-grid">
          <Stat label="Tasks" value={`${tasks.filter((t) => t.enabled).length} on of ${tasks.length}`} />
          <Stat label="Running now" value={String(listing?.running.length ?? 0)} />
          <Stat label="Runs recorded" value={String(s?.runs ?? 0)} />
          <Stat label="Success rate" value={s && s.runs - s.skipped > 0 ? pct(s.successRate) : "—"} />
          <Stat label="Data moved" value={formatSize(s?.totalBytes ?? 0)} />
          <Stat label="Items handled" value={String(s?.totalItems ?? 0)} />
        </div>
        <h3>Latest runs</h3>
        {recent.length === 0 ? <p className="muted">Nothing has run yet.</p> : <RunTable records={recent} showName onPick={onPick} />}
      </div>
    </>
  );
}

function Stat({ label, value, note }: { label: string; value: string; note?: string }) {
  return (
    <div className="stat">
      <span className="muted">{label}</span>
      <strong>{value}</strong>
      {note && <span className="muted stat-note">{note}</span>}
    </div>
  );
}

// ---------------------------------------------------------------------------------------------
// Overview of one task
// ---------------------------------------------------------------------------------------------

function Overview({ task, running, progress, next, onEdit, onChanged, onGone }: { task: Task; running?: { started: number; reason: string; elevated: boolean }; progress?: Progress; next?: number; onEdit: () => void; onChanged: () => void; onGone: () => void }) {
  const [reply, setReply] = useState<StatsReply | null>(null);
  const [last, setLast] = useState<RunRecord[]>([]);
  useEffect(() => {
    api.tasksStats(task.id).then(setReply).catch(() => {});
    api.tasksHistory(task.id, 5).then(setLast).catch(() => {});
  }, [task.id, running, task.updated]);
  const s: Stats | undefined = reply?.stats;
  const est = reply?.estimate;

  const toggle = async () => {
    try {
      await api.tasksSave({ ...task, enabled: !task.enabled });
      onChanged();
    } catch (e) {
      say(e);
    }
  };
  const remove = async () => {
    const r = await ask({ title: `Delete "${task.name}"?`, message: "The task and its settings are removed. Its record of past runs stays until you clear the history.", buttons: [{ id: "delete", label: "Delete task", danger: true }, { id: "cancel", label: "Keep it", primary: true }] });
    if (r?.button !== "delete") return;
    await api.tasksDelete(task.id).catch(say);
    onGone();
  };
  const duplicate = async () => {
    try {
      await api.tasksSave({ ...task, id: "", name: `${task.name} (copy)`, enabled: false });
      onChanged();
    } catch (e) {
      say(e);
    }
  };

  return (
    <>
      <div className="task-actions">
        {running ? (
          <button className="primary" onClick={() => void api.tasksCancel(task.id)}>
            <Icon name="pause" size={14} /> Stop
          </button>
        ) : (
          <button className="primary" onClick={() => void api.tasksRun(task.id).catch(say)}>
            <Icon name="play" size={14} /> Run now
          </button>
        )}
        {task.needsAdmin && !running && (
          <button className="secondary" title="The last run was refused for lack of permission. Windows, macOS or your system will ask you to approve this run." onClick={() => void api.tasksRun(task.id, true).catch(say)}>
            <Icon name="shield" size={14} /> Run as administrator
          </button>
        )}
        <button className="secondary" onClick={onEdit}>Edit</button>
        <button className="secondary" onClick={() => void toggle()}>{task.enabled ? "Turn off" : "Turn on"}</button>
        <button className="secondary" onClick={() => void duplicate()}>Duplicate</button>
        <button className="secondary danger" onClick={() => void remove()}>Delete</button>
      </div>

      {running && (
        <div className="task-live">
          <div className="task-live-head">
            <strong>Running{running.elevated ? " as administrator" : ""}</strong>
            <span className="muted">started {formatDate(running.started)} · {running.reason}</span>
          </div>
          <div className="bar" role="progressbar" aria-valuenow={progress && progress.bytesTotal ? Math.round((progress.bytesDone / progress.bytesTotal) * 100) : undefined}>
            <div style={{ width: `${progress && progress.bytesTotal ? Math.min(100, (progress.bytesDone / progress.bytesTotal) * 100) : progress && progress.itemsTotal ? Math.min(100, (progress.itemsDone / progress.itemsTotal) * 100) : 5}%` }} />
          </div>
          {progress && (
            <div className="task-live-stats muted">
              <span>Step {progress.step} of {progress.steps}</span>
              <span>{progress.itemsDone} of {progress.itemsTotal} items</span>
              <span>{formatSize(progress.bytesDone)} of {formatSize(progress.bytesTotal)}</span>
              {progress.elapsedSecs > 1 && progress.bytesDone > 0 && <span>{rate(progress.bytesDone / progress.elapsedSecs)}</span>}
            </div>
          )}
          {progress?.current && <div className="task-current muted" title={progress.current}>{progress.current}</div>}
        </div>
      )}

      {task.resumePending && <p className="tasks-note">This task was cut short when the app closed. It carries on at the next start.</p>}
      {task.notes && <p className="muted">{task.notes}</p>}

      <h3>When</h3>
      <ul className="plain">
        {task.triggers.map((t, i) => (
          <li key={i}>{describeTrigger(t)}</li>
        ))}
      </ul>
      {next && task.enabled && <p className="muted">Next run: {formatDate(next)}{est ? ` · expected to take about ${formatDuration(est.secs)}` : ""}</p>}

      <h3>What</h3>
      <ol className="plain">
        {task.actions.map((a, i) => (
          <li key={i}>{describeAction(a)}</li>
        ))}
      </ol>

      <h3>Statistics</h3>
      {!s || s.runs === 0 ? (
        <p className="muted">No runs recorded yet. Once it has run, its speed, run time, successes and failures appear here, and the time estimates learn from them.</p>
      ) : (
        <>
          <div className="stat-grid">
            <Stat label="Runs" value={String(s.runs)} note={`${s.skipped} not run`} />
            <Stat label="Succeeded" value={String(s.successes)} note={s.partial ? `${s.partial} with problems` : undefined} />
            <Stat label="Failed" value={String(s.failures)} note={s.cancelled || s.interrupted ? `${s.cancelled} stopped, ${s.interrupted} interrupted` : undefined} />
            <Stat label="Success rate" value={s.runs - s.skipped > 0 ? pct(s.successRate) : "—"} />
            <Stat label="Average run time" value={s.avgSecs ? formatDuration(s.avgSecs) : "—"} note={s.avgSecs ? `median ${formatDuration(s.medianSecs)} · slowest 10% over ${formatDuration(s.p90Secs)}` : undefined} />
            <Stat label="Fastest / slowest" value={s.avgSecs ? `${formatDuration(s.minSecs)} / ${formatDuration(s.maxSecs)}` : "—"} />
            <Stat label="Average speed" value={rate(s.avgBytesPerSec)} note={s.speedTrend ? (s.speedTrend > 1.1 ? "getting faster lately" : s.speedTrend < 0.9 ? "slower lately" : "steady") : undefined} />
            <Stat label="Average amount moved" value={formatSize(s.avgBytes)} note={`${Math.round(s.avgItems)} item${Math.round(s.avgItems) === 1 ? "" : "s"} per run`} />
            <Stat label="Total moved" value={formatSize(s.totalBytes)} note={`${s.totalItems} items`} />
            <Stat label="Last success" value={when(s.lastSuccess)} />
            <Stat label="Last failure" value={when(s.lastFailure)} note={s.lastError ?? undefined} />
            <Stat label="Next run takes about" value={est ? formatDuration(est.secs) : "—"} note={est ? `from ${est.samples} past run${est.samples === 1 ? "" : "s"}, by ${est.basis === "speed" ? "speed and amount" : "run time"}` : undefined} />
          </div>
          <h3>Latest runs</h3>
          <RunTable records={last} />
        </>
      )}
    </>
  );
}

function RunTable({ records, showName, onPick }: { records: RunRecord[]; showName?: boolean; onPick?: (id: string) => void }) {
  const [open, setOpen] = useState<number | null>(null);
  return (
    <table className="run-table">
      <thead>
        <tr>
          <th>When</th>
          {showName && <th>Task</th>}
          <th>Result</th>
          <th>Took</th>
          <th>Items</th>
          <th>Size</th>
          <th>Speed</th>
          <th>Why it ran</th>
        </tr>
      </thead>
      <tbody>
        {records.map((r, i) => {
          const secs = Math.max(0, (r.ended - r.started) / 1000);
          const detail = [...r.errors, r.interruptedBy ? `Cut short: ${r.interruptedBy}` : ""].filter(Boolean);
          return (
            <Fragment key={`${r.taskId}-${r.started}-${i}`}>
              <tr data-outcome={r.outcome} data-clickable={detail.length > 0 || onPick ? true : undefined} onClick={() => (detail.length ? setOpen(open === i ? null : i) : onPick?.(r.taskId))}>
                <td>{formatDate(r.started)}</td>
                {showName && <td>{r.name}</td>}
                <td><span className="badge" data-outcome={r.outcome}>{OUTCOME_LABEL[r.outcome]}</span>{r.elevated ? " 🛡" : ""}</td>
                <td>{r.outcome === "skipped" ? "—" : formatDuration(secs)}</td>
                <td>{r.items || "—"}</td>
                <td>{r.bytes ? formatSize(r.bytes) : "—"}</td>
                <td>{r.bytes && secs >= 0.2 ? rate(r.bytes / secs) : "—"}</td>
                <td className="muted">{r.reason}</td>
              </tr>
              {open === i && detail.length > 0 && (
                <tr key={`d${i}`} className="run-detail">
                  <td colSpan={showName ? 8 : 7}>
                    <ul>{detail.map((d, j) => <li key={j}>{d}</li>)}</ul>
                  </td>
                </tr>
              )}
            </Fragment>
          );
        })}
      </tbody>
    </table>
  );
}

function History({ id }: { id: string }) {
  const [records, setRecords] = useState<RunRecord[] | null>(null);
  const load = useCallback(() => void api.tasksHistory(id, 500).then(setRecords).catch(say), [id]);
  useEffect(() => {
    load();
    const off = on("task-finished", load);
    return () => void off.then((f) => f());
  }, [load]);
  const clear = async () => {
    const r = await ask({ title: "Clear this task's history?", message: "Its statistics and learned time estimates start over. The task itself is not changed.", buttons: [{ id: "clear", label: "Clear history", danger: true }, { id: "cancel", label: "Cancel", primary: true }] });
    if (r?.button === "clear") await api.tasksClearHistory(id).then(load).catch(say);
  };
  if (!records) return <p className="muted">Loading…</p>;
  return (
    <>
      <div className="task-actions">
        <button className="secondary" disabled={records.length === 0} onClick={() => void clear()}>Clear history</button>
        <span className="muted">Click a run that had problems to see what went wrong.</span>
      </div>
      {records.length === 0 ? <p className="muted">No runs recorded. Records are kept while "Keep a record of every run" is on in Settings.</p> : <RunTable records={records} />}
    </>
  );
}

// ---------------------------------------------------------------------------------------------
// The editor
// ---------------------------------------------------------------------------------------------

function Editor({ task, isNew, onChange, onSaved, onDiscard }: { task: Task; isNew: boolean; onChange: (t: Task) => void; onSaved: (t: Task, run: boolean) => void; onDiscard: () => void }) {
  const [preview, setPreview] = useState<Preview | null>(null);
  const [busy, setBusy] = useState(false);
  const problem = validate(task);
  const set = (patch: Partial<Task>) => onChange({ ...task, ...patch });
  const confirmDeletes = useStore((s) => (s.settings?.advanced["tasks.confirmDelete"] as boolean | undefined) ?? true);

  const save = async (run: boolean) => {
    if (problem) return say(problem);
    const permanent = task.actions.some((a) => (a.type === "delete" && a.permanent) || (a.type === "sync" && a.mirror));
    if (permanent && confirmDeletes) {
      const r = await ask({
        title: "This task can delete things for good",
        message: "A step deletes without using the trash can (or mirrors a folder and removes extra files). Use “Preview” first to see what it would touch. Save it anyway?",
        buttons: [{ id: "save", label: "Save", danger: true }, { id: "cancel", label: "Cancel", primary: true }],
      });
      if (r?.button !== "save") return;
    }
    setBusy(true);
    try {
      onSaved(await api.tasksSave(task), run);
    } catch (e) {
      say(e);
    } finally {
      setBusy(false);
    }
  };
  const runPreview = async () => {
    setBusy(true);
    try {
      setPreview(await api.tasksPreview(task));
    } catch (e) {
      say(e);
    } finally {
      setBusy(false);
    }
  };
  useEffect(() => setPreview(null), [task.actions]);

  const move = (i: number, d: number) => {
    const a = [...task.actions];
    const j = i + d;
    if (j < 0 || j >= a.length) return;
    [a[i], a[j]] = [a[j], a[i]];
    set({ actions: a });
  };

  return (
    <div className="editor">
      <Field label="Name">
        <input type="text" value={task.name} autoFocus={isNew} placeholder="Back up my documents" onChange={(e) => set({ name: e.target.value })} />
      </Field>
      <Field label="Notes">
        <input type="text" value={task.notes} placeholder="Optional: a reminder of what this is for" onChange={(e) => set({ notes: e.target.value })} />
      </Field>
      <label className="check">
        <input type="checkbox" checked={task.enabled} onChange={(e) => set({ enabled: e.target.checked })} /> Turned on
      </label>

      <h3>When it runs</h3>
      <p className="muted">Any of these starts the task.</p>
      {task.triggers.map((t, i) => (
        <TriggerEditor key={i} trigger={t} onChange={(n) => set({ triggers: task.triggers.map((x, j) => (j === i ? n : x)) })} onRemove={() => set({ triggers: task.triggers.filter((_, j) => j !== i) })} />
      ))}
      <button className="secondary" onClick={() => set({ triggers: [...task.triggers, newTrigger("daily")] })}>
        <Icon name="plus" size={13} /> Add a trigger
      </button>

      <h3>What it does</h3>
      <p className="muted">Steps run one after the other. {task.actions.length > 1 ? "A step that fails stops the ones after it unless “Keep going after a failure” is chosen below." : ""}</p>
      {task.actions.map((a, i) => (
        <div className="step" key={i}>
          <div className="step-head">
            <span className="step-num">{i + 1}</span>
            <select
              value={a.type}
              aria-label={`Step ${i + 1} kind`}
              onChange={(e) => set({ actions: task.actions.map((x, j) => (j === i ? carryOver(x, e.target.value as ActionType) : x)) })}
            >
              {ACTION_TYPES.map((t) => (
                <option key={t.type} value={t.type}>{t.label}</option>
              ))}
            </select>
            <span className="muted step-hint">{ACTION_TYPES.find((t) => t.type === a.type)?.hint}</span>
            <span className="step-tools">
              <button className="icon-button" aria-label="Move up" disabled={i === 0} onClick={() => move(i, -1)}><Icon name="up" size={13} /></button>
              <button className="icon-button" aria-label="Move down" disabled={i === task.actions.length - 1} onClick={() => move(i, 1)}><Icon name="expand" size={11} /></button>
              <button className="icon-button" aria-label="Duplicate step" onClick={() => set({ actions: [...task.actions.slice(0, i + 1), structuredClone(a), ...task.actions.slice(i + 1)] })}><Icon name="copy" size={13} /></button>
              <button className="icon-button" aria-label="Remove step" onClick={() => set({ actions: task.actions.filter((_, j) => j !== i) })}><Icon name="trash" size={13} /></button>
            </span>
          </div>
          <ActionEditor action={a} onChange={(n) => set({ actions: task.actions.map((x, j) => (j === i ? n : x)) })} />
        </div>
      ))}
      <button className="secondary" onClick={() => set({ actions: [...task.actions, newAction("copy")] })}>
        <Icon name="plus" size={13} /> Add a step
      </button>

      <h3>Options</h3>
      <OptionsEditor options={task.options} onChange={(o) => set({ options: o })} />

      <h3>Check it first</h3>
      <div className="task-actions">
        <button className="secondary" disabled={busy || task.actions.length === 0} onClick={() => void runPreview()}>Preview what it would do</button>
        <span className="muted">Nothing is changed by a preview.</span>
      </div>
      {preview && <PreviewPanel preview={preview} />}

      <div className="editor-foot">
        {problem && <span className="problem" role="alert">{problem}</span>}
        <span className="grow" />
        <button className="secondary" onClick={onDiscard}>{isNew ? "Cancel" : "Discard changes"}</button>
        <button className="secondary" disabled={busy || !!problem} onClick={() => void save(true)}>Save and run now</button>
        <button className="primary" disabled={busy || !!problem} onClick={() => void save(false)}>Save</button>
      </div>
    </div>
  );
}

/** Switching a step to another kind keeps what still makes sense (sources, destination, filter). */
function carryOver(old: Action, type: ActionType): Action {
  if (old.type === type) return old;
  const fresh = newAction(type);
  const sources: string[] = "from" in old ? (Array.isArray(old.from) ? old.from : old.from ? [old.from] : []) : "paths" in old ? old.paths : [];
  const to = "to" in old ? old.to : "";
  const filter = "filter" in old ? old.filter : null;
  const out: Record<string, unknown> = { ...fresh };
  if ("from" in fresh) out.from = type === "sync" ? sources[0] ?? "" : sources;
  if ("paths" in fresh) out.paths = sources;
  if ("to" in fresh) out.to = to;
  if ("filter" in fresh && filter) out.filter = filter;
  return out as unknown as Action;
}

function Field({ label, children, hint }: { label: string; children: React.ReactNode; hint?: string }) {
  return (
    <label className="field">
      <span className="field-label">{label}</span>
      {children}
      {hint && <span className="muted field-hint">{hint}</span>}
    </label>
  );
}

function NumInput({ value, onChange, min = 0, max, step = 1, placeholder, label }: { value: number | null; onChange: (v: number | null) => void; min?: number; max?: number; step?: number; placeholder?: string; label?: string }) {
  return (
    <input
      type="number"
      className="num"
      aria-label={label}
      value={value ?? ""}
      min={min}
      max={max}
      step={step}
      placeholder={placeholder}
      onChange={(e) => onChange(e.target.value === "" ? null : Number(e.target.value))}
    />
  );
}

// ---- Triggers --------------------------------------------------------------------------------

function TriggerEditor({ trigger, onChange, onRemove }: { trigger: Trigger; onChange: (t: Trigger) => void; onRemove: () => void }) {
  const switchTo = (type: TriggerType) => onChange(newTrigger(type));
  const every = trigger.type === "every" ? trigger : null;
  const unit = every ? (every.minutes % 1440 === 0 ? 1440 : every.minutes % 60 === 0 ? 60 : 1) : 1;
  return (
    <div className="trigger">
      <select value={trigger.type} aria-label="Trigger kind" onChange={(e) => switchTo(e.target.value as TriggerType)}>
        {TRIGGER_TYPES.map((t) => (
          <option key={t.type} value={t.type}>{t.label}</option>
        ))}
      </select>
      <div className="trigger-fields">
        {trigger.type === "once" && <input type="datetime-local" value={toLocalInput(trigger.at)} aria-label="Date and time" onChange={(e) => onChange({ ...trigger, at: fromLocalInput(e.target.value) })} />}
        {every && (
          <>
            <span>every</span>
            <NumInput label="How many" min={1} value={every.minutes / unit} onChange={(v) => onChange({ ...every, minutes: Math.max(1, Math.round((v ?? 1) * unit)) })} />
            <select value={unit} aria-label="Unit" onChange={(e) => onChange({ ...every, minutes: (every.minutes / unit) * Number(e.target.value) })}>
              <option value={1}>minutes</option>
              <option value={60}>hours</option>
              <option value={1440}>days</option>
            </select>
            <span className="muted">counting from</span>
            <input type="datetime-local" value={toLocalInput(every.anchor)} aria-label="Counting from" onChange={(e) => onChange({ ...every, anchor: fromLocalInput(e.target.value) })} />
          </>
        )}
        {trigger.type === "daily" && (
          <>
            <input type="time" value={trigger.time} aria-label="Time" onChange={(e) => onChange({ ...trigger, time: e.target.value || "03:00" })} />
            <span className="days" role="group" aria-label="Days">
              {WEEKDAYS.map((d, i) => (
                <button key={d} type="button" className="day" aria-pressed={trigger.days.length === 0 || trigger.days.includes(i)} onClick={() => {
                  const all = trigger.days.length === 0 ? [0, 1, 2, 3, 4, 5, 6] : trigger.days;
                  const next = all.includes(i) ? all.filter((x) => x !== i) : [...all, i];
                  onChange({ ...trigger, days: next.length === 7 || next.length === 0 ? [] : next.sort() });
                }}>{d}</button>
              ))}
            </span>
          </>
        )}
        {trigger.type === "monthly" && (
          <>
            <span>on day</span>
            <NumInput label="Day of the month" min={1} max={31} value={trigger.day} onChange={(v) => onChange({ ...trigger, day: Math.min(31, Math.max(1, v ?? 1)) })} />
            <span>at</span>
            <input type="time" value={trigger.time} aria-label="Time" onChange={(e) => onChange({ ...trigger, time: e.target.value || "03:00" })} />
            <span className="muted">(a short month uses its last day)</span>
          </>
        )}
        {trigger.type === "onChange" && (
          <>
            <PathBox value={trigger.path} onChange={(path) => onChange({ ...trigger, path })} folder placeholder="Folder to watch" />
            <span>then wait</span>
            <NumInput label="Seconds of quiet" min={1} value={trigger.quietSecs} onChange={(v) => onChange({ ...trigger, quietSecs: Math.max(1, v ?? 10) })} />
            <span>seconds of quiet</span>
          </>
        )}
        {trigger.type === "driveConnected" && <input type="text" value={trigger.name} placeholder="Part of the drive's name (empty: any drive)" onChange={(e) => onChange({ ...trigger, name: e.target.value })} />}
        {trigger.type === "startup" && <span className="muted">Runs a few seconds after the app opens (turn on “Start with the computer” in Settings to have that happen at sign-in).</span>}
        {trigger.type === "manual" && <span className="muted">Never starts by itself.</span>}
      </div>
      <button className="icon-button" aria-label="Remove trigger" onClick={onRemove}><Icon name="close" size={13} /></button>
    </div>
  );
}

// ---- Paths ------------------------------------------------------------------------------------

function PathBox({ value, onChange, folder, placeholder, file }: { value: string; onChange: (v: string) => void; folder?: boolean; file?: boolean; placeholder?: string }) {
  const pick = async () => {
    try {
      const r = await openDialog({ directory: !!folder && !file, multiple: false, title: placeholder ?? "Choose" });
      if (typeof r === "string") onChange(r);
    } catch (e) {
      say(e);
    }
  };
  return (
    <span className="pathbox">
      <input type="text" value={value} placeholder={placeholder} spellCheck={false} onChange={(e) => onChange(e.target.value)} />
      <button type="button" className="secondary" onClick={() => void pick()}>Browse…</button>
    </span>
  );
}

function PathList({ value, onChange, label }: { value: string[]; onChange: (v: string[]) => void; label: string }) {
  const add = async (directory: boolean) => {
    try {
      const r = await openDialog({ directory, multiple: true, title: label });
      const picked = Array.isArray(r) ? r : typeof r === "string" ? [r] : [];
      if (picked.length) onChange([...new Set([...value, ...picked])]);
    } catch (e) {
      say(e);
    }
  };
  const [text, setText] = useState("");
  return (
    <div className="pathlist">
      {value.map((p, i) => (
        <div className="pathrow" key={p + i}>
          <input type="text" value={p} spellCheck={false} aria-label={label} onChange={(e) => onChange(value.map((x, j) => (j === i ? e.target.value : x)))} />
          <button className="icon-button" aria-label="Remove" onClick={() => onChange(value.filter((_, j) => j !== i))}><Icon name="close" size={12} /></button>
        </div>
      ))}
      <div className="pathrow">
        <input
          type="text"
          value={text}
          placeholder={value.length ? "Add another path…" : "Type or paste a path, or use the buttons"}
          spellCheck={false}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && text.trim()) (onChange([...value, text.trim()]), setText(""));
          }}
          onBlur={() => text.trim() && (onChange([...value, text.trim()]), setText(""))}
        />
        <button type="button" className="secondary" onClick={() => void add(true)}>Folders…</button>
        <button type="button" className="secondary" onClick={() => void add(false)}>Files…</button>
      </div>
    </div>
  );
}

// ---- Steps ------------------------------------------------------------------------------------

const CONFLICTS: [Conflict, string][] = [
  ["overwrite", "Replace it"],
  ["overwriteIfNewer", "Replace it only if the new one is newer"],
  ["skip", "Leave the existing one"],
  ["keepBoth", "Keep both (number the new one)"],
];

function ActionEditor({ action: a, onChange }: { action: Action; onChange: (a: Action) => void }) {
  const [filtering, setFiltering] = useState(false);
  const filter = "filter" in a ? a.filter : null;
  const patch = (p: Partial<Action>) => onChange({ ...a, ...p } as Action);
  const open = filtering || (filter && !isOpenFilter(filter));
  return (
    <div className="step-body">
      {(a.type === "copy" || a.type === "move") && (
        <>
          <Field label="From (files and folders)"><PathList label="Source" value={a.from} onChange={(from) => patch({ from })} /></Field>
          <Field label="To (folder)"><PathBox value={a.to} folder placeholder="Destination folder" onChange={(to) => patch({ to })} /></Field>
          <Field label="If a file with that name is already there">
            <select value={a.conflict} onChange={(e) => patch({ conflict: e.target.value as Conflict })}>
              {CONFLICTS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
            </select>
          </Field>
          <label className="check"><input type="checkbox" checked={a.keepStructure} onChange={(e) => patch({ keepStructure: e.target.checked })} /> Keep the folder structure (otherwise all files land directly in the destination)</label>
        </>
      )}
      {a.type === "delete" && (
        <>
          <Field label="Delete from"><PathList label="What to delete" value={a.paths} onChange={(paths) => patch({ paths })} /></Field>
          <label className="check"><input type="checkbox" checked={a.permanent} onChange={(e) => patch({ permanent: e.target.checked })} /> Delete for good (skip the trash can)</label>
          <label className="check"><input type="checkbox" checked={a.removeEmptyFolders} onChange={(e) => patch({ removeEmptyFolders: e.target.checked })} /> Remove folders that are left empty</label>
          <label className="check"><input type="checkbox" checked={a.removeFolder} onChange={(e) => patch({ removeFolder: e.target.checked })} /> Delete the chosen folders themselves when no filter is set</label>
        </>
      )}
      {a.type === "compress" && (
        <>
          <Field label="Compress"><PathList label="What to compress" value={a.from} onChange={(from) => patch({ from })} /></Field>
          <Field label="Save as" hint="Use {date}, {time}, {datetime}, {year}, {month}, {day} and {task} in the name: backups/{task}-{date}.zip">
            <PathBox value={a.to} file placeholder="/path/to/archive-{date}.zip" onChange={(to) => patch({ to })} />
          </Field>
        </>
      )}
      {a.type === "extract" && (
        <>
          <Field label="Archive"><PathBox value={a.archive} file placeholder="Archive to unpack" onChange={(archive) => patch({ archive })} /></Field>
          <Field label="Unpack into"><PathBox value={a.to} folder placeholder="Folder" onChange={(to) => patch({ to })} /></Field>
        </>
      )}
      {a.type === "sync" && (
        <>
          <Field label="Source folder"><PathBox value={a.from} folder placeholder="Folder to copy from" onChange={(from) => patch({ from })} /></Field>
          <Field label="Make this folder match"><PathBox value={a.to} folder placeholder="Folder to update" onChange={(to) => patch({ to })} /></Field>
          <label className="check"><input type="checkbox" checked={a.mirror} onChange={(e) => patch({ mirror: e.target.checked })} /> Mirror: files that are only in the destination go to the trash</label>
          <p className="muted">New and changed files are copied. Files that are the same size and date are left alone, so a second run only does what is new.</p>
        </>
      )}
      {a.type === "run" && (
        <>
          <Field label="Program or script"><PathBox value={a.program} file placeholder="Program to start" onChange={(program) => patch({ program })} /></Field>
          <Field label="Arguments (one per line)" hint="{date}, {time} and the other tokens work here too.">
            <textarea rows={Math.min(5, Math.max(2, a.args.length + 1))} value={a.args.join("\n")} spellCheck={false} onChange={(e) => patch({ args: e.target.value === "" ? [] : e.target.value.split("\n") })} />
          </Field>
          <Field label="Start in folder"><PathBox value={a.cwd} folder placeholder="Optional" onChange={(cwd) => patch({ cwd })} /></Field>
          <Field label="Give up after (seconds, 0 = never)"><NumInput value={a.timeoutSecs} min={0} onChange={(v) => patch({ timeoutSecs: v ?? 0 })} /></Field>
        </>
      )}
      {filter && (
        <>
          <button className="linklike" aria-expanded={!!open} onClick={() => setFiltering(!open)}>{open ? "Hide filters" : isOpenFilter(filter) ? "Only some files…" : "Filters (on)"}</button>
          {open && <FilterEditor filter={filter} onChange={(f) => patch({ filter: f } as Partial<Action>)} />}
        </>
      )}
    </div>
  );
}

const isOpenFilter = (f: Filter) => !f.include.trim() && !f.exclude.trim() && f.olderThanDays == null && f.newerThanDays == null && f.minBytes == null && f.maxBytes == null && f.recursive && !f.hidden;

const MB = 1024 * 1024;

function FilterEditor({ filter: f, onChange }: { filter: Filter; onChange: (f: Filter) => void }) {
  const set = (p: Partial<Filter>) => onChange({ ...f, ...p });
  return (
    <div className="filter-grid">
      <Field label="Only names like" hint="For example *.jpg; report*. Separate with ; or ,"><input type="text" value={f.include} onChange={(e) => set({ include: e.target.value })} /></Field>
      <Field label="Leave out names like"><input type="text" value={f.exclude} placeholder="*.tmp; ~*" onChange={(e) => set({ exclude: e.target.value })} /></Field>
      <Field label="Older than (days)"><NumInput value={f.olderThanDays} min={0} step={0.5} placeholder="any age" onChange={(v) => set({ olderThanDays: v })} /></Field>
      <Field label="Newer than (days)"><NumInput value={f.newerThanDays} min={0} step={0.5} placeholder="any age" onChange={(v) => set({ newerThanDays: v })} /></Field>
      <Field label="At least (MB)"><NumInput value={f.minBytes == null ? null : f.minBytes / MB} min={0} step={0.1} placeholder="any size" onChange={(v) => set({ minBytes: v == null ? null : Math.round(v * MB) })} /></Field>
      <Field label="At most (MB)"><NumInput value={f.maxBytes == null ? null : f.maxBytes / MB} min={0} step={0.1} placeholder="any size" onChange={(v) => set({ maxBytes: v == null ? null : Math.round(v * MB) })} /></Field>
      <label className="check"><input type="checkbox" checked={f.recursive} onChange={(e) => set({ recursive: e.target.checked })} /> Include folders inside folders</label>
      <label className="check"><input type="checkbox" checked={f.hidden} onChange={(e) => set({ hidden: e.target.checked })} /> Include hidden files (names starting with a dot)</label>
    </div>
  );
}

// ---- Options ------------------------------------------------------------------------------------

function OptionsEditor({ options: o, onChange }: { options: Options; onChange: (o: Options) => void }) {
  const set = (p: Partial<Options>) => onChange({ ...o, ...p });
  return (
    <div className="options-grid">
      <label className="check"><input type="checkbox" checked={!o.stopOnError} onChange={(e) => set({ stopOnError: !e.target.checked })} /> Keep going after a failure (otherwise the first failure ends the run)</label>
      <label className="check"><input type="checkbox" checked={o.catchUp} onChange={(e) => set({ catchUp: e.target.checked })} /> Run once when the computer is back if a run was missed while it was off or asleep</label>
      <label className="check"><input type="checkbox" checked={o.skipIfRunning} onChange={(e) => set({ skipIfRunning: e.target.checked })} /> Do not start again while the last run is still going</label>
      <label className="check"><input type="checkbox" checked={o.onlyOnAc} onChange={(e) => set({ onlyOnAc: e.target.checked })} /> Only while plugged in</label>
      <label className="check"><input type="checkbox" checked={o.lowPriority} onChange={(e) => set({ lowPriority: e.target.checked })} /> Run quietly (low priority, so it does not slow other work)</label>
      <label className="check"><input type="checkbox" checked={o.runAsAdmin} onChange={(e) => set({ runAsAdmin: e.target.checked })} /> Always run as administrator (the system asks you to approve each run)</label>
      <Field label="When the computer sleeps or shuts down in the middle of a run" hint="After a shutdown, “Carry on” resumes it at the next start (files that were not finished are done again). “Cancel” leaves it until the next scheduled run.">
        <select value={o.onSleep} onChange={(e) => set({ onSleep: e.target.value as Options["onSleep"] })}>
          <option value="resume">Carry on afterwards</option>
          <option value="cancel">Cancel the run; wait for the next one</option>
        </select>
      </Field>
      <Field label="Retry a failed file">
        <span className="inline">
          <NumInput value={o.retries} min={0} max={20} onChange={(v) => set({ retries: v ?? 0 })} label="Retries" /> times, waiting <NumInput value={o.retryDelaySecs} min={1} max={3600} onChange={(v) => set({ retryDelaySecs: v ?? 5 })} label="Seconds between tries" /> seconds
        </span>
      </Field>
      <Field label="Stop a run that takes longer than (minutes, 0 = no limit)"><NumInput value={o.timeoutMinutes} min={0} onChange={(v) => set({ timeoutMinutes: v ?? 0 })} /></Field>
      <Field label="Tell me when it finishes">
        <select value={o.notify} onChange={(e) => set({ notify: e.target.value as Options["notify"] })}>
          <option value="failure">Only if something went wrong</option>
          <option value="always">Every time</option>
          <option value="never">Never</option>
        </select>
      </Field>
      <button className="linklike" onClick={() => onChange(defaultOptions())}>Back to the defaults</button>
    </div>
  );
}

function PreviewPanel({ preview: p }: { preview: Preview }) {
  return (
    <div className="preview">
      <p>
        <strong>{p.files}</strong> item{p.files === 1 ? "" : "s"}, <strong>{formatSize(p.bytes)}</strong>
        {p.truncated ? " (showing the first part of the list)" : ""}
      </p>
      {p.problems.map((x, i) => <p key={i} className="problem">{x}</p>)}
      {p.ops.length > 0 && (
        <ul className="preview-list">
          {p.ops.filter((o) => o.kind !== "mkdir").map((o, i) => (
            <li key={i}>
              <span className="badge" data-kind={o.kind}>{o.kind}</span>
              <span className="preview-path" title={o.from}>{o.from}</span>
              {o.to && <><span className="muted">→</span><span className="preview-path" title={o.to}>{o.to}</span></>}
              {o.bytes > 0 && <span className="muted">{formatSize(o.bytes)}</span>}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
