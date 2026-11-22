// Action notices ("Copied 3 items") in a corner of the window: pop-ups that
// fade on their own, or a small bell that opens the list. Configured in
// Settings > Notifications.

import { useEffect, useRef, useState } from "react";
import { adv } from "../settings/advanced";
import { useStore } from "../store";
import type { Notice, NoticeKind } from "../util/notices";
import { Icon } from "./Icons";

const ICON: Record<NoticeKind, string> = { copy: "copy", cut: "cut", move: "forward", delete: "trash", rename: "rename", create: "plus", undo: "undo", info: "info", error: "close" };

const clock = (t: number) => new Date(t).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });


const SHOWN_NAMES = 8;

/** One notice. When it has details, it opens into the same layout a transfer's card uses. */
function NoticeRow({ n, popup }: { n: Notice; popup?: boolean }) {
  const [open, setOpen] = useState(false);
  const { dismissNotice } = useStore.getState();
  const detailed = !!n.facts?.length;
  return (
    <div className={`notice-block${popup ? " notice-popup" : ""}`} data-kind={n.kind} data-open={open || undefined}>
      <div className="notice" data-kind={n.kind} role={popup ? "status" : undefined}>
        <Icon name={ICON[n.kind]} size={14} />
        <div
          className="grow"
          data-link={n.kind === "error" || undefined}
          title={n.kind === "error" ? "Click to see the activity log" : undefined}
          onClick={n.kind === "error" ? () => useStore.getState().openActivityLog() : undefined}
        >
          <div>{n.title}</div>
          {n.detail && <div className="muted">{n.detail}</div>}
        </div>
        {!popup && <span className="muted">{clock(n.at)}</span>}
        {detailed && (
          <button className="icon-button" aria-label={open ? "Hide details" : "Show details"} aria-expanded={open} title={open ? "Hide details" : "Show details"} onClick={() => setOpen(!open)}>
            <Icon name={open ? "expand" : "chevron"} size={11} />
          </button>
        )}
        {/* Every notice has its X: a pop-up is hidden (it stays in the list), one in the list is removed. */}
        <button
          className="icon-button"
          aria-label={popup ? "Hide" : "Remove"}
          title={popup ? "Hide" : "Remove from the list"}
          onClick={() => dismissNotice(n.id, !popup)}
        >
          <Icon name="close" size={11} />
        </button>
      </div>
      {!!n.actions?.length && (
        <div className="notice-actions">
          {n.actions.map((a) => (
            <button key={a.label} className="secondary" onClick={a.run}>
              {a.label}
            </button>
          ))}
        </div>
      )}
      {open && detailed && (
        <div className="transfer-detail notice-detail">
          <div className="percent">
            <strong>{n.kind === "cut" || n.kind === "copy" ? (n.facts!.find(([k]) => k === "Status")?.[1] ?? "Done") : n.kind === "error" ? "Failed" : "100% complete"}</strong>
          </div>
          <div className="meter" role="progressbar" aria-valuenow={n.kind === "cut" || n.kind === "copy" ? 0 : 100} aria-valuemin={0} aria-valuemax={100}>
            <div className="meter-fill" style={{ width: n.kind === "cut" || n.kind === "copy" ? "0%" : "100%" }} />
          </div>
          <dl className="details-list transfer-facts">
            {n.facts!.map(([k, v]) => (
              <span key={k} style={{ display: "contents" }}>
                <dt>{k}</dt>
                <dd className="path">{v}</dd>
              </span>
            ))}
            <dt>When</dt>
            <dd>{clock(n.at)}</dd>
          </dl>
          {!!n.names?.length && (
            <ul className="notice-names">
              {n.names.slice(0, SHOWN_NAMES).map((name, i) => (
                <li key={i} className="path">{name}</li>
              ))}
              {n.names.length > SHOWN_NAMES && <li className="muted">and {n.names.length - SHOWN_NAMES} more</li>}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}

export function Notices() {
  const settings = useStore((s) => s.settings)!;
  const notices = useStore((s) => s.notices);
  const open = useStore((s) => s.noticesOpen);
  const unread = useStore((s) => s.unreadNotices);
  // What finished is part of the same list: its graphs and details open from here, not from a bar of their own.
  const finishedCount = useStore((s) => Object.keys(s.finished).length);
  const { clearNotices } = useStore.getState();
  const box = useRef<HTMLDivElement>(null);
  // Jobs tucked away into the button: how many, and how far along they are together.
  const tucked = useStore((s) => s.transfersHidden);
  const live = (s: ReturnType<typeof useStore.getState>) => [...Object.values(s.transfers), ...Object.values(s.deletes)].filter((p) => p.phase === "running" || p.phase === "scanning" || p.phase === "queued" || p.phase === "paused");
  const count = useStore((s) => live(s).length);
  // Whole percent, so the button is not redrawn on every tiny step.
  const percent = useStore((s) => {
    const l = live(s);
    return l.length ? Math.round((l.reduce((sum, p) => sum + (s.shown[p.id] ?? 0), 0) / l.length) * 100) : 0;
  });
  const running = { count, done: percent / 100 };
  const hiddenJobs = tucked ? running.count : 0;

  const enabled = adv<boolean>(settings, "notify.enabled");
  const position = adv<string>(settings, "notify.position");
  const bell = adv<string>(settings, "notify.mode") === "bell";
  // In the bell mode ordinary notices are never pop-ups; an important one (a crash report) still is.
  const popups = notices.filter((n) => n.popup).slice(-4);

  // Keep the transfer cards clear of notices sharing their corner.
  useEffect(() => {
    const root = document.documentElement;
    const measure = () => root.style.setProperty("--notices-h", position === "bottom-right" && box.current ? `${box.current.offsetHeight + 8}px` : "0px");
    measure();
    const ro = new ResizeObserver(measure);
    if (box.current) ro.observe(box.current);
    return () => {
      ro.disconnect();
      root.style.setProperty("--notices-h", "0px");
    };
  }, [position, enabled, popups.length, open]);

  if (!enabled && !notices.length && !finishedCount && !hiddenJobs) return null;
  const showBell = bell || notices.length > 0 || finishedCount > 0 || hiddenJobs > 0;

  return (
    <div className="notices" data-position={position} data-size={adv<string>(settings, "notify.size")} ref={box} aria-live="polite">
      {open && (
        <div className="notice-list" role="dialog" aria-label="Recent actions">
          <div className="notice-list-head">
            <strong className="grow">Recent actions</strong>
            <button className="link-button" disabled={!notices.length} onClick={clearNotices}>
              Clear
            </button>
            <button className="icon-button" aria-label="Close" onClick={() => useStore.setState({ noticesOpen: false })}>
              <Icon name="close" size={12} />
            </button>
          </div>
          {hiddenJobs > 0 && (
            <button className="notice-finished" onClick={() => useStore.setState({ transfersHidden: false, noticesOpen: false })} title="Bring the running copies, moves and deletes back onto the screen">
              <Icon name="expand" size={12} /> Show running jobs ({hiddenJobs}) · {Math.round(running.done * 100)}%
            </button>
          )}
          {finishedCount > 0 && (
            <button className="notice-finished" onClick={() => useStore.setState({ finishedOpen: true, transfersOpen: false, noticesOpen: false })} title="The final graph and every detail of the copies, moves and deletes that finished">
              <Icon name="clock" size={12} /> Graphs and details of finished jobs ({finishedCount})
            </button>
          )}
          {notices.length === 0 && <p className="muted">Nothing yet. Copy, move, delete and rename show up here.</p>}
          {[...notices].reverse().map((n) => (
            <NoticeRow key={n.id} n={n} />
          ))}
        </div>
      )}
      {!open && popups.map((n) => <NoticeRow key={n.id} n={n} popup />)}
      {showBell && (
        <button
          className="notice-bell"
          data-busy={hiddenJobs > 0 || undefined}
          style={hiddenJobs > 0 ? ({ "--ring": `${Math.round(running.done * 100)}%` } as React.CSSProperties) : undefined}
          aria-label={`Recent actions${unread ? `, ${unread} new` : ""}${hiddenJobs ? `, ${hiddenJobs} running, ${Math.round(running.done * 100)}% done` : ""}`}
          title={hiddenJobs ? `${hiddenJobs} running (${Math.round(running.done * 100)}%): click for the list` : "Recent actions"}
          onClick={() => useStore.setState({ noticesOpen: !open, unreadNotices: 0 })}
        >
          <Icon name="info" size={15} />
          {unread > 0 ? <span className="notice-badge">{unread > 9 ? "9+" : unread}</span> : hiddenJobs > 0 ? <span className="notice-badge" data-running>{hiddenJobs > 9 ? "9+" : hiddenJobs}</span> : null}
        </button>
      )}
    </div>
  );
}
