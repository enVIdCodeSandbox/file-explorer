// Action notices ("Copied 3 items") in a corner of the window: pop-ups that
// fade on their own, or a small bell that opens the list. Configured in
// Settings > Notifications.

import { useEffect, useRef } from "react";
import { adv } from "../settings/advanced";
import { useStore } from "../store";
import type { NoticeKind } from "../util/notices";
import { Icon } from "./Icons";

const ICON: Record<NoticeKind, string> = { copy: "copy", cut: "cut", move: "forward", delete: "trash", rename: "rename", create: "plus", undo: "undo", info: "info", error: "close" };

const clock = (t: number) => new Date(t).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });

export function Notices() {
  const settings = useStore((s) => s.settings)!;
  const notices = useStore((s) => s.notices);
  const open = useStore((s) => s.noticesOpen);
  const unread = useStore((s) => s.unreadNotices);
  const { dismissNotice, clearNotices } = useStore.getState();
  const box = useRef<HTMLDivElement>(null);

  const enabled = adv<boolean>(settings, "notify.enabled");
  const position = adv<string>(settings, "notify.position");
  const bell = adv<string>(settings, "notify.mode") === "bell";
  const popups = bell ? [] : notices.filter((n) => n.popup).slice(-4);

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

  if (!enabled) return null;
  const showBell = bell || notices.length > 0;

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
          {notices.length === 0 && <p className="muted">Nothing yet. Copy, move, delete and rename show up here.</p>}
          {[...notices].reverse().map((n) => (
            <div key={n.id} className="notice" data-kind={n.kind}>
              <Icon name={ICON[n.kind]} size={14} />
              <div className="grow">
                <div>{n.title}</div>
                {n.detail && <div className="muted">{n.detail}</div>}
              </div>
              <span className="muted">{clock(n.at)}</span>
            </div>
          ))}
        </div>
      )}
      {!open &&
        popups.map((n) => (
          <div key={n.id} className="notice notice-popup" data-kind={n.kind} role="status">
            <Icon name={ICON[n.kind]} size={14} />
            <div className="grow">
              <div>{n.title}</div>
              {n.detail && <div className="muted">{n.detail}</div>}
            </div>
            <button className="icon-button" aria-label="Hide" title="Hide" onClick={() => dismissNotice(n.id)}>
              <Icon name="close" size={11} />
            </button>
          </div>
        ))}
      {showBell && (
        <button
          className="notice-bell"
          aria-label={`Recent actions${unread ? `, ${unread} new` : ""}`}
          title="Recent actions"
          onClick={() => useStore.setState({ noticesOpen: !open, unreadNotices: 0 })}
        >
          <Icon name="info" size={15} />
          {unread > 0 && <span className="notice-badge">{unread > 9 ? "9+" : unread}</span>}
        </button>
      )}
    </div>
  );
}
