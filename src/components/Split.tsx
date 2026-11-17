// Split view: the folder view cut into up to eight panes. The first is the normal file list; the others are lighter
// folder lists, each with tabs of its own. Dragging a tab over the window shows where it would go (the panes make room,
// with a box where the tab would end up); the dividers are dragged, or moved with the arrow keys, and stop at a
// minimum share of the space.

import { useEffect, useRef, useState } from "react";
import { adv } from "../settings/advanced";
import { useStore } from "../store";
import {
  applyPreset,
  closeAllPanes,
  cyclePane,
  ensurePanes,
  evenOutPanes,
  focusPane,
  isCurrentShape,
  paneCount,
  presetsNow,
  setBranchShares,
  useSplit,
} from "../split/state";
import { GHOST, MAX_PANES, drag as dragShares, insertPane, isBranch, layout, leaves, minShare, presetsFor, removePane, setShares, sharesOf, type Handle, type Node } from "../util/splitLayout";
import type { MenuItem } from "./ContextMenu";
import { FileIcon } from "./Icons";
import { SplitPane } from "./Panes";

const GAP = 6;

/** The entries for the View menu: the ready-made layouts for the panes there are now and for one more. */
export function splitMenuItems(): MenuItem[] {
  const { count, current, more } = presetsNow();
  const items: MenuItem[] = [];
  if (more.length) items.push({ id: "split-more", separatorBefore: true, label: count === 1 ? "Split into two panes" : `Add a pane (${count + 1} panes)`, children: more.map((p) => ({ id: p.id, label: p.label, run: () => applyPreset(p) })) });
  if (count >= 2) {
    items.push({ id: "split-current", label: `Arrange the ${count} panes`, children: current.map((p) => ({ id: p.id, label: p.label, checked: isCurrentShape(p), run: () => applyPreset(p) })) });
    items.push({ id: "split-even", label: "Even out the sizes", run: evenOutPanes });
    items.push({ id: "split-close", label: "Close all extra panes", run: closeAllPanes });
  } else {
    items.push({ id: "split-hint", label: "Or drag a tab down into the window", disabled: true, separatorBefore: true });
  }
  return items;
}

/** What the panes would look like with a dragged tab over a place: the layout with a ghost pane in it. */
function previewOf(tree: Node, drag: ReturnType<typeof useSplit.getState>["drag"]): { tree: Node; merge: number | null } {
  const hover = drag?.hover;
  if (!hover) return { tree, merge: null };
  const base = drag.empties !== null ? removePane(tree, drag.empties) : tree;
  if (hover.preset) {
    const preset = presetsFor(paneCount(base) + 1).find((p) => p.id === hover.preset);
    return preset ? { tree: preset.build([0, ...leaves(base).filter((s) => s !== 0), GHOST]), merge: null } : { tree, merge: null };
  }
  const z = hover.zone;
  if (!z) return { tree, merge: null };
  if (z.side === "merge") return { tree: base, merge: z.target };
  return { tree: insertPane(base, z.target, z.side, GHOST), merge: null };
}

export function SplitArea({ main, folder, allowed }: { main: React.ReactNode; folder: string; allowed: boolean }) {
  const settings = useStore((s) => s.settings);
  const tree = useSplit((s) => s.tree);
  const panes = useSplit((s) => s.panes);
  const drag = useSplit((s) => s.drag);
  const focused = useSplit((s) => s.history[0]);
  const host = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 0, h: 0 });
  const [live, setLive] = useState<{ path: string; shares: number[] } | null>(null);
  const percent = adv<number>(settings, "split.minPercent");

  useEffect(() => void useSplit.setState({ allowed }), [allowed]);
  useEffect(() => ensurePanes(folder), [tree, folder]);

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const measure = () => setSize({ w: el.clientWidth, h: el.clientHeight });
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // Anything outside the panes (the toolbar, the sidebar, the tab bar) acts on the normal file list.
  useEffect(() => {
    const down = (e: PointerEvent) => {
      const t = e.target as HTMLElement | null;
      if (t?.closest?.("[data-leaf], .menu-backdrop, [role=dialog], [data-pane-tabs]")) return;
      focusPane(0);
    };
    document.addEventListener("pointerdown", down, true);
    return () => document.removeEventListener("pointerdown", down, true);
  }, []);

  // The older "second pane" switch becomes two side by side, once.
  useEffect(() => {
    const s = useStore.getState();
    if (adv<boolean>(s.settings, "view.dualPane") && paneCount() === 1) {
      useSplit.setState({ tree: { dir: "row", kids: [0, 1] } });
      s.updateSettings((st) => void (st.advanced["view.dualPane"] = false));
    }
  }, [settings]);

  const effective: Node = allowed ? tree : 0;
  const shownBase = previewOf(effective, drag);
  const shown = live ? setShares(shownBase.tree, live.path, live.shares) : shownBase.tree;
  const count = leaves(effective).length;
  const measured = size.w > 0 && size.h > 0;
  const placed = measured && leaves(shown).length > 1 ? layout(shown, { x: 0, y: 0, w: size.w, h: size.h }, GAP) : null;
  const slots = [...new Set([0, ...leaves(effective)])].sort((a, b) => a - b);

  const startHandle = (h: Handle, e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();
    const el = e.currentTarget;
    el.setPointerCapture(e.pointerId);
    const vertical = h.dir === "col";
    const from = vertical ? e.clientY : e.clientX;
    const floor = minShare(percent, h.kids, h.room);
    let latest = h.shares;
    document.body.setAttribute("data-split-dragging", h.dir);
    const move = (ev: PointerEvent) => {
      latest = dragShares(h.shares, h.index, ((vertical ? ev.clientY : ev.clientX) - from) / h.room, floor);
      setLive({ path: h.path, shares: latest });
    };
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("pointercancel", up);
      document.body.removeAttribute("data-split-dragging");
      setBranchShares(h.path, latest);
      setLive(null);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", up);
  };

  const handleKey = (h: Handle, e: React.KeyboardEvent) => {
    const vertical = h.dir === "col";
    const back = vertical ? "ArrowUp" : "ArrowLeft";
    const fwd = vertical ? "ArrowDown" : "ArrowRight";
    if (e.key === back || e.key === fwd) {
      e.preventDefault();
      setBranchShares(h.path, dragShares(h.shares, h.index, (e.key === fwd ? 1 : -1) * (e.shiftKey ? 0.1 : 0.02), minShare(percent, h.kids, h.room)));
    } else if (e.key === "Home" || e.key === "Enter") {
      e.preventDefault();
      setBranchShares(h.path, h.shares.map(() => 1 / h.kids));
    }
  };

  const style = (slot: number): React.CSSProperties => {
    if (!placed) return { inset: 0 };
    const b = placed.panes[slot];
    return b ? { left: b.x, top: b.y, width: b.w, height: b.h } : { display: "none" };
  };
  const ghost = placed?.panes[GHOST];
  const floor = (h: Handle) => minShare(percent, h.kids, h.room);

  return (
    <div ref={host} className="pane-row split-host" data-count={count} data-dragging={drag ? true : undefined}>
      {slots.map((slot) => (
        <div
          key={slot}
          className="split-leaf"
          data-leaf={slot}
          data-split-pane={slot === 0 ? 0 : undefined}
          data-focused={count > 1 && focused === slot ? true : undefined}
          data-merge={shownBase.merge === slot ? true : undefined}
          style={style(slot)}
          onMouseDownCapture={slot === 0 ? () => focusPane(0) : undefined}
          onFocusCapture={slot === 0 ? () => focusPane(0) : undefined}
          onKeyDown={
            slot === 0
              ? (e) => {
                  // Tab goes to the next pane when the list itself has the focus.
                  if (count > 1 && e.key === "Tab" && !e.ctrlKey && !e.altKey && !e.metaKey && (e.target as HTMLElement).classList.contains("file-scroller")) {
                    e.preventDefault();
                    cyclePane(0, e.shiftKey ? -1 : 1);
                  }
                }
              : undefined
          }
        >
          {slot === 0 ? main : panes[slot] ? <SplitPane slot={slot} leftFolder={folder} /> : null}
        </div>
      ))}
      {ghost && (
        <div className="split-ghost" style={{ left: ghost.x, top: ghost.y, width: ghost.w, height: ghost.h }} aria-hidden="true">
          <span>
            <FileIcon kind="folder" size={16} /> {drag?.label}
          </span>
        </div>
      )}
      {placed?.handles.map((h) => (
        <div
          key={`${h.path}/${h.index}`}
          className="split-divider"
          data-dir={h.dir}
          role="separator"
          aria-orientation={h.dir === "col" ? "horizontal" : "vertical"}
          aria-label="Resize panes (drag, or use the arrow keys; double-click to even out)"
          aria-valuemin={Math.round(floor(h) * 100)}
          aria-valuemax={Math.round((1 - floor(h)) * 100)}
          aria-valuenow={Math.round(h.shares[h.index] * 100)}
          tabIndex={0}
          title="Drag to resize · double-click to even out"
          style={{ left: h.box.x, top: h.box.y, width: h.box.w, height: h.box.h }}
          onPointerDown={(e) => startHandle(h, e)}
          onKeyDown={(e) => handleKey(h, e)}
          onDoubleClick={() => setBranchShares(h.path, h.shares.map(() => 1 / h.kids))}
        />
      ))}
      {drag && allowed && count < MAX_PANES && drag.empties === null && <LayoutPicker count={count} active={drag.hover?.preset} />}
    </div>
  );
}

/** While a tab is dragged: the ready-made layouts for one more pane, to drop it on. */
function LayoutPicker({ count, active }: { count: number; active?: string }) {
  const others = leaves(useSplit.getState().tree).filter((s) => s !== 0);
  return (
    <div className="split-presets" role="group" aria-label={`Layouts for ${count + 1} panes`}>
      <div className="split-presets-title">Drop on a layout of {count + 1} panes, or on a side of a pane</div>
      <div className="split-presets-row">
        {presetsFor(count + 1).map((p) => (
          <div key={p.id} className="split-preset" data-preset={p.id} data-active={active === p.id || undefined} title={p.label}>
            <Mini tree={p.build([0, ...others, GHOST])} />
          </div>
        ))}
      </div>
    </div>
  );
}

function Mini({ tree }: { tree: Node }) {
  if (!isBranch(tree)) return <div className="mini-leaf" data-ghost={tree === GHOST || undefined} data-main={tree === 0 || undefined} />;
  const shares = sharesOf(tree);
  return (
    <div className="mini-branch" data-dir={tree.dir}>
      {tree.kids.map((k, i) => (
        <div key={i} className="mini-cell" style={{ flex: `${shares[i]} 1 0` }}>
          <Mini tree={k} />
        </div>
      ))}
    </div>
  );
}
