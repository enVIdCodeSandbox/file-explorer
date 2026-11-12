// Draws the window from the layout model: docks along the edges, floating
// panels, a tray for popped-out ones, and the on-screen editing mode.

import { useEffect, useRef, useState } from "react";
import { DOCK_LABEL, PANELS, PRESETS, moveTo, nudge, panelsAt, resolveLayout, zoneAt, type Dock, type Layout, type PanelId } from "../util/layout";
import { ContextMenu, type MenuItem, type MenuState } from "./ContextMenu";
import { Icon } from "./Icons";
import { useStore } from "../store";

export function useLayout(): [Layout, (next: Layout) => void] {
  const saved = useStore((s) => s.settings?.appearance.layout);
  const layout = resolveLayout(saved);
  const set = (next: Layout) => useStore.getState().updateSettings((s) => void (s.appearance.layout = next));
  return [layout, set];
}

/** Change one panel's state. */
export function patchPanel(layout: Layout, id: PanelId, patch: Partial<Layout[PanelId]>): Layout {
  return { ...layout, [id]: { ...layout[id], ...patch } };
}

interface FrameProps {
  id: PanelId;
  layout: Layout;
  setLayout: (l: Layout) => void;
  children: React.ReactNode;
  onGrab: (id: PanelId, e: React.PointerEvent) => void;
}

/** One panel with its handle (shown while editing, and always for floating panels). */
function Frame({ id, layout, setLayout, children, onGrab }: FrameProps) {
  const edit = useStore((s) => s.layoutEdit);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const state = layout[id];
  const info = PANELS.find((p) => p.id === id)!;
  const floating = state.dock === "float";
  const ref = useRef<HTMLDivElement>(null);
  const sidebarCollapsed = useStore((s) => s.settings?.appearance.sidebarCollapsed);
  const collapsed = id === "sidebar" ? false : state.collapsed;

  const openMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const items: MenuItem[] = [
      ...info.docks.map((d) => ({
        id: `dock-${d}`,
        label: DOCK_LABEL[d],
        checked: state.dock === d,
        run: () => setLayout(moveTo(layout, id, d, d === "float" ? { x: 140, y: 140 } : undefined)),
      })),
      { id: "earlier", label: "Move earlier", separatorBefore: true, disabled: panelsAt(layout, state.dock).indexOf(id) <= 0, run: () => setLayout(nudge(layout, id, -1)) },
      { id: "later", label: "Move later", disabled: panelsAt(layout, state.dock).indexOf(id) >= panelsAt(layout, state.dock).length - 1, run: () => setLayout(nudge(layout, id, 1)) },
      id === "sidebar"
        ? { id: "collapse", label: sidebarCollapsed ? "Show labels" : "Collapse to icons", run: () => useStore.getState().updateSettings((s) => void (s.appearance.sidebarCollapsed = !s.appearance.sidebarCollapsed)) }
        : { id: "collapse", label: state.collapsed ? "Expand" : "Collapse", run: () => setLayout(patchPanel(layout, id, { collapsed: !state.collapsed })) },
    ];
    setMenu({ x: e.clientX, y: e.clientY, items });
  };

  // Persist a floating panel's size after the person drags its corner.
  const saveSize = () => {
    const el = ref.current;
    if (!el || !floating) return;
    const w = Math.round(el.offsetWidth);
    const h = Math.round(el.offsetHeight);
    if (w !== state.w || h !== state.h) setLayout(patchPanel(layout, id, { w, h }));
  };

  const showGrip = edit || floating;
  return (
    <div
      ref={ref}
      className="panel"
      data-panel={id}
      data-dock={state.dock}
      data-collapsed={collapsed || undefined}
      data-edit={edit || undefined}
      style={floating ? { left: state.x, top: state.y, width: state.w, height: collapsed ? undefined : state.h } : undefined}
      onPointerUp={floating ? saveSize : undefined}
    >
      {showGrip && (
        <div className="panel-grip" onPointerDown={(e) => onGrab(id, e)} onContextMenu={openMenu} title="Drag to move this panel. Right-click for more.">
          <Icon name="more" size={12} />
          <span className="grow">{info.label}</span>
          {id !== "sidebar" && (
            <button className="icon-button" aria-label={state.collapsed ? "Expand" : "Collapse"} title={state.collapsed ? "Expand" : "Collapse"} onPointerDown={(e) => e.stopPropagation()} onClick={() => setLayout(patchPanel(layout, id, { collapsed: !state.collapsed }))}>
              <Icon name={state.collapsed ? "expand" : "collapse"} size={11} />
            </button>
          )}
          <button className="icon-button" aria-label="Panel position" title="Position" onPointerDown={(e) => e.stopPropagation()} onClick={openMenu}>
            <Icon name="settings" size={12} />
          </button>
        </div>
      )}
      {collapsed && !showGrip ? (
        <button className="panel-collapsed" onClick={() => setLayout(patchPanel(layout, id, { collapsed: false }))} title={`Show ${info.label}`}>
          <Icon name="expand" size={10} />
          <span>{info.label}</span>
        </button>
      ) : collapsed ? null : (
        <div className="panel-body">{children}</div>
      )}
      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
    </div>
  );
}

interface ShellProps {
  /** What to draw for each panel; null means it has nothing to show right now. */
  render: (id: PanelId, dock: Dock) => React.ReactNode | null;
  content: React.ReactNode;
}

export function Shell({ render, content }: ShellProps) {
  const [layout, setLayout] = useLayout();
  const edit = useStore((s) => s.layoutEdit);
  const popout = useStore((s) => s.popout);
  const [drag, setDrag] = useState<{ id: PanelId; x: number; y: number; zone: Dock } | null>(null);
  const root = useRef<HTMLDivElement>(null);

  const visible = (id: PanelId) => render(id, layout[id].dock) != null;

  const grab = (id: PanelId, e: React.PointerEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    const el = root.current!;
    const rect = el.getBoundingClientRect();
    const offset = layout[id].dock === "float" ? { x: e.clientX - rect.left - layout[id].x, y: e.clientY - rect.top - layout[id].y } : { x: 40, y: 12 };
    let latest = { id, x: e.clientX, y: e.clientY, zone: layout[id].dock };
    const info = PANELS.find((p) => p.id === id)!;
    const move = (m: PointerEvent) => {
      const zone = zoneAt(m.clientX - rect.left, m.clientY - rect.top, rect.width, rect.height);
      latest = { id, x: m.clientX, y: m.clientY, zone: info.docks.includes(zone) ? zone : "float" };
      setDrag(latest);
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      setDrag(null);
      if (latest.zone === "float") setLayout(moveTo(layout, id, "float", { x: latest.x - rect.left - offset.x, y: latest.y - rect.top - offset.y }));
      else if (latest.zone !== layout[id].dock) setLayout(moveTo(layout, id, latest.zone));
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const frame = (id: PanelId) => {
    const content = render(id, layout[id].dock);
    if (content == null) return null;
    return (
      <Frame key={id} id={id} layout={layout} setLayout={setLayout} onGrab={grab}>
        {content}
      </Frame>
    );
  };

  const dock = (d: Dock) => {
    const ids = panelsAt(layout, d).filter(visible);
    if (!ids.length && !(edit && drag)) return null;
    return (
      <div className={`dock dock-${d}`} data-dock={d} data-empty={ids.length === 0 || undefined}>
        {ids.map(frame)}
      </div>
    );
  };

  const popped = panelsAt(layout, "popout").filter(visible);
  const floating = panelsAt(layout, "float").filter(visible);
  const open = popout && layout[popout].dock === "popout" ? popout : null;

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && useStore.setState({ popout: null });
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  return (
    <div className="shell" ref={root} data-editing={edit || undefined}>
      {dock("top")}
      <div className="shell-middle">
        {dock("left")}
        <main className="content">{content}</main>
        {dock("right")}
      </div>
      {dock("bottom")}

      {floating.map(frame)}

      {popped.length > 0 && (
        <div className="popout-tray" role="toolbar" aria-label="Hidden panels">
          {popped.map((id) => {
            const info = PANELS.find((p) => p.id === id)!;
            return (
              <button key={id} className="icon-button" data-on={open === id || undefined} title={info.label} aria-label={info.label} onClick={() => useStore.setState({ popout: open === id ? null : id })}>
                <Icon name={info.icon} size={15} />
              </button>
            );
          })}
        </div>
      )}
      {open && (
        <div className="popout-backdrop" onMouseDown={() => useStore.setState({ popout: null })}>
          <div className="popout-panel" data-panel={open} onMouseDown={(e) => e.stopPropagation()}>
            {render(open, "popout")}
          </div>
        </div>
      )}

      {edit && (
        <div className="layout-banner" role="status">
          <Icon name="more" size={14} />
          <span>Drag a panel's handle to an edge to dock it, or to the middle to float it. Right-click a handle for more.</span>
          <select aria-label="Layout preset" value="" onChange={(e) => e.target.value && setLayout(structuredClone(PRESETS.find((p) => p.id === e.target.value)!.layout))}>
            <option value="">Preset…</option>
            {PRESETS.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
          <button className="secondary" onClick={() => setLayout(structuredClone(PRESETS[0].layout))}>
            Reset
          </button>
          <button className="primary" onClick={() => useStore.setState({ layoutEdit: false })}>
            Done
          </button>
        </div>
      )}
      {edit && drag && (
        <div className="drop-zones" aria-hidden="true">
          {(["top", "left", "right", "bottom"] as const).map((z) => (
            <div key={z} className={`drop-zone zone-${z}`} data-hot={drag.zone === z || undefined}>
              {DOCK_LABEL[z]}
            </div>
          ))}
          <div className="drop-zone zone-float" data-hot={drag.zone === "float" || undefined}>
            Float here
          </div>
        </div>
      )}
    </div>
  );
}
