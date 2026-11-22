// Settings > Logs: every action the app takes, live, with what is running right now and what has stopped answering.

import { useEffect, useMemo, useRef, useState } from "react";
import { api, type ActivityActive, type ActivityEvent } from "../api";
import { useStore } from "../store";

export type LogFilter = "all" | "problems" | "hung";

/** Which events the filter and the search text leave in. */
export function filterEvents(events: ActivityEvent[], filter: LogFilter, text: string): ActivityEvent[] {
  const needle = text.trim().toLowerCase();
  return events.filter((e) => {
    if (filter === "hung" && e.level !== "hung") return false;
    if (filter === "problems" && e.level === "info") return false;
    if (!needle) return true;
    return `${e.area} ${e.what} ${e.detail}`.toLowerCase().includes(needle);
  });
}

export function clock(ms: number): string {
  const d = new Date(ms);
  const two = (n: number) => String(n).padStart(2, "0");
  return `${two(d.getHours())}:${two(d.getMinutes())}:${two(d.getSeconds())}.${String(d.getMilliseconds()).padStart(3, "0")}`;
}

export function age(ms: number): string {
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  return m < 60 ? `${m} min ${s % 60} s` : `${Math.floor(m / 60)} h ${m % 60} min`;
}

/** The newest events are kept in the page; the backend keeps many more (and the file keeps what the list drops). */
const SHOWN_MAX = 5000;

export function ActivityLogPanel() {
  const [events, setEvents] = useState<ActivityEvent[]>([]);
  const [active, setActive] = useState<ActivityActive[]>([]);
  const [folder, setFolder] = useState("");
  const [filter, setFilter] = useState<LogFilter>("all");
  const [text, setText] = useState("");
  const [paused, setPaused] = useState(false);
  const last = useRef(0);
  const box = useRef<HTMLDivElement>(null);
  const { showToast } = useStore.getState();

  useEffect(() => {
    let alive = true;
    const tick = async () => {
      try {
        const snap = await api.activityLog(last.current);
        if (!alive) return;
        if (snap.lastSeq < last.current) {
          // The log was cleared.
          last.current = 0;
          setEvents([]);
        }
        last.current = snap.lastSeq;
        setActive(snap.active);
        setFolder(snap.folder);
        if (snap.events.length) setEvents((old) => [...old, ...snap.events].slice(-SHOWN_MAX));
      } catch {
        /* the app is closing */
      }
    };
    void tick();
    const t = window.setInterval(() => void tick(), 1000);
    return () => {
      alive = false;
      window.clearInterval(t);
    };
  }, []);

  const shown = useMemo(() => filterEvents(events, filter, text), [events, filter, text]);

  useEffect(() => {
    const el = box.current;
    if (el && !paused) el.scrollTop = el.scrollHeight;
  }, [shown, paused]);

  const copy = async () => {
    const all = await api.activityExport().catch(() => "");
    if (all) await navigator.clipboard.writeText(all).then(() => showToast("Log copied."));
  };
  const save = async () => {
    const all = await api.activityExport().catch(() => "");
    if (!all) return;
    const a = document.createElement("a");
    a.href = URL.createObjectURL(new Blob([all], { type: "text/plain" }));
    a.download = `fast-explorer-log-${new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-")}.txt`;
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 5000);
  };
  const clear = async () => {
    await api.activityClear();
    last.current = 0;
    setEvents([]);
  };

  const hung = active.filter((a) => a.hung).length;
  return (
    <div className="activity-log">
      <div className="activity-now">
        <strong>Right now{hung ? ` — ${hung} not answering` : ""}</strong>
        {active.length === 0 ? (
          <p className="muted">Nothing is running.</p>
        ) : (
          <ul>
            {active.map((a) => (
              <li key={a.id} data-hung={a.hung || undefined}>
                <span className="activity-tag">{a.hung ? "HUNG" : a.area}</span>
                <span className="activity-what">{a.what}</span>
                {a.detail && <span className="activity-detail">{a.detail}</span>}
                <span className="muted">
                  {age(a.ageMs)}
                  {a.quietMs > 3000 ? `, no progress for ${age(a.quietMs)}` : ""}
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>
      <div className="activity-tools">
        <select value={filter} onChange={(e) => setFilter(e.target.value as LogFilter)} aria-label="Show">
          <option value="all">Everything</option>
          <option value="problems">Problems and hangs</option>
          <option value="hung">Only hangs</option>
        </select>
        <input type="search" placeholder="Filter the list" value={text} onChange={(e) => setText(e.target.value)} />
        <label className="check">
          <input type="checkbox" checked={paused} onChange={(e) => setPaused(e.target.checked)} /> Hold scrolling
        </label>
        <button className="secondary" onClick={() => void copy()}>
          Copy
        </button>
        <button className="secondary" onClick={() => void save()}>
          Save as text file
        </button>
        <button className="secondary" onClick={() => void clear()}>
          Clear
        </button>
      </div>
      <div className="activity-list" ref={box} role="log">
        {shown.length === 0 && <p className="muted">Nothing recorded{events.length ? " that matches" : " yet"}.</p>}
        {shown.map((e) => (
          <div key={e.seq} className="activity-row" data-level={e.level}>
            <span className="activity-time">{clock(e.atMs)}</span>
            <span className="activity-tag">{e.level === "hung" ? "HUNG" : e.level === "info" ? e.area : `${e.level} · ${e.area}`}</span>
            <span className="activity-what">{e.what}</span>
            {e.detail && <span className="activity-detail">{e.detail}</span>}
            {e.tookMs != null && <span className="muted">{e.tookMs} ms</span>}
          </div>
        ))}
      </div>
      {folder && <p className="muted">Also kept in {folder} (the last two files, newest first).</p>}
    </div>
  );
}
