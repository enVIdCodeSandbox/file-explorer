// The tabs at the top of an extra pane of the split view. They work like the tabs of the window: click to switch,
// middle-click or × to close, + for a new one, drag to reorder, or drag to another pane (or an edge of one, to make a
// new pane there), or onto the tab bar of the window to open it there. Ctrl+T, Ctrl+W and Ctrl+Tab act on them while
// the pane has the focus.

import { useRef, useState } from "react";
import { useSplit, activatePaneTab, closeOtherPaneTabs, closePane, closePaneTab, dropOnPanes, endHover, hoverPanes, moveTab, openPaneTab, reorderPaneTab } from "../split/state";
import { baseName } from "../util/path";
import { ContextMenu, type MenuState } from "./ContextMenu";
import { FileIcon, Icon } from "./Icons";

const titleOf = (path: string) => baseName(path) || path;

export function PaneTabs({ slot }: { slot: number }) {
  const pane = useSplit((s) => s.panes[slot]);
  const strip = useRef<HTMLDivElement>(null);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [drag, setDrag] = useState<{ id: string; x: number; y: number; w: number; index: number; where: "strip" | "main" | "panes" | "none"; over: boolean } | null>(null);
  if (!pane) return null;

  const start = (e: React.PointerEvent, id: string) => {
    if (e.button !== 0 || (e.target as HTMLElement).closest(".pane-tab-close")) return;
    const [sx, sy] = [e.clientX, e.clientY];
    const w = (e.currentTarget as HTMLElement).offsetWidth;
    const label = titleOf(pane.tabs.find((t) => t.id === id)?.path ?? "");
    let started = false;
    const where = (ev: PointerEvent) => {
      const r = strip.current?.getBoundingClientRect();
      const inStrip = !!r && ev.clientX >= r.left - 8 && ev.clientX <= r.right + 8 && ev.clientY >= r.top - 8 && ev.clientY <= r.bottom + 8;
      let index = 0;
      strip.current?.querySelectorAll<HTMLElement>(".pane-tab[data-tab-id]").forEach((el) => {
        if (el.dataset.tabId === id) return;
        const b = el.getBoundingClientRect();
        if (ev.clientX > b.left + b.width / 2) index++;
      });
      const overMain = !inStrip && !!document.elementFromPoint(ev.clientX, ev.clientY)?.closest(".tab-bar, .tab-strip");
      return { index, inStrip, overMain };
    };
    const move = (ev: PointerEvent) => {
      if (!started && Math.hypot(ev.clientX - sx, ev.clientY - sy) < 6) return;
      started = true;
      const at = where(ev);
      let kind: "strip" | "main" | "panes" | "none" = "none";
      let over = false;
      if (at.inStrip) {
        kind = "strip";
        endHover();
      } else if (at.overMain) {
        kind = "main";
        endHover();
      } else if (hoverPanes(ev.clientX, ev.clientY, { slot, tabId: id }, label)) {
        kind = "panes";
        over = !!useSplit.getState().drag?.hover;
      }
      setDrag({ id, x: ev.clientX, y: ev.clientY, w, index: at.index, where: kind, over });
    };
    const finish = (commit: boolean, ev?: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("keydown", key, true);
      setDrag(null);
      if (!started || !commit || !ev) return endHover();
      const at = where(ev);
      if (at.inStrip) {
        endHover();
        reorderPaneTab(slot, id, at.index);
      } else if (at.overMain) {
        endHover();
        moveTab({ slot, tabId: id }, { target: 0, side: "merge" });
      } else dropOnPanes({ slot, tabId: id });
    };
    const up = (ev: PointerEvent) => finish(true, ev);
    const key = (ev: KeyboardEvent) => ev.key === "Escape" && finish(false);
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("keydown", key, true);
  };

  const menuFor = (e: React.MouseEvent, id: string) => {
    e.preventDefault();
    const tab = pane.tabs.find((t) => t.id === id)!;
    setMenu({
      x: e.clientX,
      y: e.clientY,
      items: [
        { id: "dup", label: "Duplicate tab", run: () => openPaneTab(slot, tab.path, pane.tabs.findIndex((t) => t.id === id) + 1) },
        { id: "main", label: "Open in the window's tab bar", run: () => moveTab({ slot, tabId: id }, { target: 0, side: "merge" }) },
        { id: "close", label: "Close tab", separatorBefore: true, run: () => closePaneTab(slot, id) },
        { id: "others", label: "Close other tabs", disabled: pane.tabs.length < 2, run: () => closeOtherPaneTabs(slot, id) },
        { id: "pane", label: "Close this pane", run: () => closePane(slot) },
      ],
    });
  };

  const dragged = drag && pane.tabs.find((t) => t.id === drag.id);
  const hint = !drag ? "" : drag.where === "strip" ? "" : drag.where === "main" ? "Open in the window's tab bar" : drag.where === "panes" ? (drag.over ? "Drop here" : "Drop on a pane") : "";
  return (
    <div className="pane-tabs" data-pane-tabs={slot} ref={strip} role="tablist" aria-label={`Tabs of pane ${slot + 1}`}>
      {pane.tabs.map((t, i) => {
        // The marker for where a dragged tab would be put, while it is reordered in this strip.
        const rest = drag?.where === "strip" ? pane.tabs.filter((x) => x.id !== drag.id) : [];
        const marker = drag?.where === "strip" && drag.id !== t.id && rest[drag.index]?.id === t.id;
        return (
          <div
            key={t.id}
            className="pane-tab"
            role="tab"
            aria-selected={t.id === pane.active}
            data-tab-id={t.id}
            data-active={t.id === pane.active || undefined}
            data-dragging={drag?.id === t.id || undefined}
            data-marker={marker || undefined}
            data-last-marker={drag?.where === "strip" && i === pane.tabs.length - 1 && drag.index >= rest.length && drag.id !== t.id ? true : undefined}
            title={t.path}
            onPointerDown={(e) => start(e, t.id)}
            onClick={() => activatePaneTab(slot, t.id)}
            onAuxClick={(e) => e.button === 1 && closePaneTab(slot, t.id)}
            onContextMenu={(e) => menuFor(e, t.id)}
          >
            <FileIcon kind="folder" size={14} />
            <span className="pane-tab-title">{titleOf(t.path)}</span>
            <button className="pane-tab-close" aria-label={`Close ${titleOf(t.path)}`} title="Close tab (Ctrl+W)" onClick={(e) => (e.stopPropagation(), closePaneTab(slot, t.id))}>
              <Icon name="close" size={11} />
            </button>
          </div>
        );
      })}
      <button className="pane-tab-new" aria-label="New tab in this pane" title="New tab (Ctrl+T)" onClick={() => openPaneTab(slot)}>
        <Icon name="plus" size={13} />
      </button>
      {drag && dragged && drag.where !== "strip" && (
        <div className="pane-tab pane-tab-ghost" data-detach={drag.where === "none" || undefined} style={{ left: drag.x - drag.w / 2, top: drag.y - 14, width: drag.w }}>
          <FileIcon kind="folder" size={14} />
          <span className="pane-tab-title">{titleOf(dragged.path)}</span>
        </div>
      )}
      {/* Said beside the tab, not inside it: a tab is too narrow for the sentence, and it pushed the name out. */}
      {drag && dragged && hint && drag.where !== "strip" && (
        <div className="pane-tab-note" style={{ left: drag.x - drag.w / 2, top: drag.y + 18 }}>{hint}</div>
      )}
      {drag && dragged && drag.where === "strip" && (
        <div className="pane-tab pane-tab-ghost" style={{ left: drag.x - drag.w / 2, top: drag.y - 14, width: drag.w }}>
          <FileIcon kind="folder" size={14} />
          <span className="pane-tab-title">{titleOf(dragged.path)}</span>
        </div>
      )}
      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
    </div>
  );
}
