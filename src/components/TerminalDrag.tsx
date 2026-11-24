// Dragging the terminal by its title bar: while the pointer is inside the window, a ghost box shows where the panel
// would dock (the same kind of preview as dragging a tab into the split view); letting go outside the window moves
// the shell into a window of its own, or into the system's terminal app, as set in Settings > Terminal.

import { create } from "zustand";
import { api } from "../api";
import { adv } from "../settings/advanced";
import { useStore } from "../store";
import { DOCK_LABEL, moveTo, zoneAt, type Dock, type Layout } from "../util/layout";
import { useTerminal } from "../terminal/state";
import { Icon } from "./Icons";

interface DragState {
  /** Where the panel would go; "outside" is past the window's edge. */
  zone: Dock | "outside";
  x: number;
  y: number;
}

export const useTerminalDrag = create<DragState | null>(() => null);

const DEAD = 6;
const SIZE = 0.38;

/** Where the ghost box is for a zone, in window pixels. */
export function ghostRect(zone: Dock, w: number, h: number): { left: number; top: number; width: number; height: number } {
  switch (zone) {
    case "top": return { left: 0, top: 0, width: w, height: h * SIZE };
    case "bottom": return { left: 0, top: h * (1 - SIZE), width: w, height: h * SIZE };
    case "left": return { left: 0, top: 0, width: w * SIZE, height: h };
    case "right": return { left: w * (1 - SIZE), top: 0, width: w * SIZE, height: h };
    default: return { left: w * 0.25, top: h * 0.25, width: w * 0.5, height: h * 0.5 };
  }
}

export const isOutside = (x: number, y: number, w = window.innerWidth, h = window.innerHeight) => x < 0 || y < 0 || x > w || y > h;

/** Which place the pointer is over: a dock for each edge, floating in the middle, or "outside". */
export function zoneFor(x: number, y: number, docks: Dock[]): DragState["zone"] {
  if (isOutside(x, y)) return "outside";
  const z = zoneAt(x, y, window.innerWidth, window.innerHeight);
  return docks.includes(z) ? z : "float";
}

/** Start dragging the terminal. Nothing happens until the pointer has moved a few pixels, so a click still clicks. */
export function beginTerminalDrag(e: React.PointerEvent, opts: { docks: Dock[]; layout: Layout; setLayout: (l: Layout) => void; current: Dock; detach: (x: number, y: number) => void }) {
  if (e.button !== 0) return;
  const target = e.target as HTMLElement;
  if (target.closest("button, input, select")) return;
  const from = { x: e.clientX, y: e.clientY };
  let started = false;
  let latest = { zone: opts.current as DragState["zone"], x: from.x, y: from.y, cx: from.x, cy: from.y };
  const el = e.currentTarget as HTMLElement;
  try {
    el.setPointerCapture(e.pointerId);
  } catch {
    /* the window still hears the moves while the pointer is inside it */
  }
  const finish = (apply: boolean) => {
    window.removeEventListener("pointermove", move, true);
    window.removeEventListener("pointerup", up, true);
    window.removeEventListener("pointercancel", cancel, true);
    window.removeEventListener("keydown", key, true);
    window.removeEventListener("blur", cancel);
    try {
      el.releasePointerCapture(e.pointerId);
    } catch {
      /* already let go */
    }
    document.body.removeAttribute("data-terminal-dragging");
    useTerminalDrag.setState(null, true);
    if (!apply || !started) return;
    if (latest.zone === "outside") opts.detach(latest.x, latest.y);
    else if (latest.zone === "float") opts.setLayout(moveTo(opts.layout, "terminal", "float", { x: latest.cx - 40, y: latest.cy - 12 }));
    else if (latest.zone !== opts.current) opts.setLayout(moveTo(opts.layout, "terminal", latest.zone));
  };
  const move = (m: PointerEvent) => {
    if (!started && Math.hypot(m.clientX - from.x, m.clientY - from.y) < DEAD) return;
    if (m.buttons === 0) return finish(false);
    started = true;
    document.body.setAttribute("data-terminal-dragging", "");
    latest = { zone: zoneFor(m.clientX, m.clientY, opts.docks), x: m.screenX, y: m.screenY, cx: m.clientX, cy: m.clientY };
    useTerminalDrag.setState({ zone: latest.zone, x: m.clientX, y: m.clientY }, true);
  };
  const up = () => finish(true);
  const cancel = () => finish(false);
  const key = (k: KeyboardEvent) => {
    if (k.key === "Escape") finish(false);
  };
  window.addEventListener("pointermove", move, true);
  window.addEventListener("pointerup", up, true);
  window.addEventListener("pointercancel", cancel, true);
  window.addEventListener("keydown", key, true);
  window.addEventListener("blur", cancel);
}

/** Open the shell as the settings say: a window of its own in the app (the default), or the system's terminal app. */
export async function detachShell(tabId: string, paneId: string, sid: string, x: number, y: number) {
  const st = useStore.getState();
  try {
    if (adv<string>(st.settings, "terminal.detachTo") === "system") {
      await api.termPopout(sid);
      return;
    }
    // Let go of the pane first, so the shell is never shown in two places; if the window cannot open, take it back.
    const released = useTerminal.getState().release(tabId, paneId);
    if (!released) return;
    try {
      await api.termWindow(sid, x, y);
    } catch (e) {
      await useTerminal.getState().adopt(sid);
      throw e;
    }
  } catch (e) {
    st.showToast(String(e));
  }
}

/** The ghost box (or the note about leaving the window) shown while the terminal is being dragged. */
export function TerminalDropPreview() {
  const drag = useTerminalDrag();
  const system = useStore((s) => adv<string>(s.settings, "terminal.detachTo") === "system");
  if (!drag) return null;
  const w = window.innerWidth;
  const h = window.innerHeight;
  if (drag.zone === "outside") {
    return (
      <div className="terminal-drop" aria-hidden="true">
        <div className="terminal-drop-note" style={{ left: Math.min(Math.max(drag.x, 120), w - 120), top: Math.min(Math.max(drag.y, 30), h - 30) }}>
          <Icon name="popout" size={14} /> {system ? "Release to open in the system's terminal app" : "Release to open in a new terminal window"}
        </div>
      </div>
    );
  }
  const r = ghostRect(drag.zone, w, h);
  return (
    <div className="terminal-drop" aria-hidden="true">
      <div className="terminal-drop-ghost" style={r}>
        <span>
          <Icon name="terminal" size={14} /> Terminal: {DOCK_LABEL[drag.zone]}
        </span>
      </div>
    </div>
  );
}
