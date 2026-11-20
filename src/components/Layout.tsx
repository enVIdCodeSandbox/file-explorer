// Draws the window from the layout model: docks along the edges, floating
// panels, a tray for popped-out ones, and the on-screen editing mode.

import { useEffect, useRef, useState } from "react";
import { DOCK_LABEL, PANELS, PRESETS, RESIZABLE, moveTo, nudge, panelsAt, resolveLayout, zoneAt, type Dock, type Layout, type PanelId } from "../util/layout";
import { ContextMenu, type MenuItem, type MenuState } from "./ContextMenu";
import { Icon } from "./Icons";
import { useStore } from "../store";
import { NARROW_WINDOW, SIDEBAR_LIMITS, clampFloat, clampPanelSize, panelRange } from "../util/limits";

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
    const { w, h } = clampFloat(el.offsetWidth, el.offsetHeight);
    if (w !== state.w || h !== state.h) setLayout(patchPanel(layout, id, { w, h }));
  };

  // A docked panel that can be resized is as wide (left and right) or as tall (top and bottom) as the person made it.
  const sideDock = state.dock === "left" || state.dock === "right";
  const edgeDock = sideDock || state.dock === "top" || state.dock === "bottom";
  const resizable = RESIZABLE.includes(id) && edgeDock && !collapsed;
  const [dragSize, setDragSize] = useState<number | null>(null);
  const size = dragSize ?? state.size;
  // Its smallest and largest size, kept by the stylesheet too: a window made smaller shrinks the panel down to its
  // smallest size before the files are squeezed, and a size saved on a bigger screen cannot take over a small one.
  const range = RESIZABLE.includes(id) && edgeDock ? panelRange(id, state.dock) : null;
  const sidebarFolded = useStore((st) => !!st.settings?.appearance.sidebarCollapsed);
  const limits: React.CSSProperties | undefined = range
    ? sideDock
      ? { minWidth: range[0], maxWidth: `min(${range[1]}px, 60vw)` }
      : { minHeight: range[0], maxHeight: `min(${range[1]}px, 60vh)` }
    : id === "sidebar" && sideDock
      ? { minWidth: sidebarFolded ? SIDEBAR_LIMITS.collapsed : SIDEBAR_LIMITS.min }
      : undefined;
  const startResize = (e: React.PointerEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const el = ref.current;
    if (!el) return;
    const target = e.currentTarget;
    target.setPointerCapture(e.pointerId);
    const start = sideDock ? el.offsetWidth : el.offsetHeight;
    const at = sideDock ? e.clientX : e.clientY;
    // Dragging towards the middle of the window makes the panel bigger.
    const sign = state.dock === "right" || state.dock === "bottom" ? -1 : 1;
    const room = (el.closest(".shell") as HTMLElement | null)?.getBoundingClientRect();
    const roomAlong = sideDock ? room?.width ?? 1200 : room?.height ?? 800;
    let latest = start;
    const move = (m: PointerEvent) => {
      latest = clampPanelSize(id, state.dock, start + sign * ((sideDock ? m.clientX : m.clientY) - at), roomAlong);
      setDragSize(latest);
    };
    const up = () => {
      target.removeEventListener("pointermove", move as EventListener);
      target.removeEventListener("pointerup", up);
      setDragSize(null);
      setLayout(patchPanel(layout, id, { size: latest }));
    };
    target.addEventListener("pointermove", move as EventListener);
    target.addEventListener("pointerup", up);
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
      style={
        floating
          ? { left: `min(${state.x}px, calc(100% - 120px))`, top: `min(${state.y}px, calc(100% - 40px))`, width: state.w, height: collapsed ? undefined : state.h }
          : resizable && size
            ? sideDock
              ? { width: size, ...limits }
              : { height: size, ...limits }
            : limits
      }
      onPointerUp={floating ? saveSize : undefined}
    >
      {resizable && (
        <div
          className="panel-resizer"
          data-edge={state.dock === "bottom" ? "top" : state.dock === "top" ? "bottom" : state.dock === "left" ? "right" : "left"}
          role="separator"
          aria-orientation={sideDock ? "vertical" : "horizontal"}
          aria-label={`Resize ${info.label}`}
          onPointerDown={startResize}
          onDoubleClick={() => setLayout(patchPanel(layout, id, { size: undefined }))}
        />
      )}
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

/** True while the window is too narrow for panels beside the files (see MIN_CONTENT in limits.ts). */
function useNarrowWindow(): boolean {
  const [narrow, setNarrow] = useState(() => window.innerWidth < NARROW_WINDOW);
  useEffect(() => {
    const q = window.matchMedia(`(max-width: ${NARROW_WINDOW - 1}px)`);
    const on = () => setNarrow(q.matches);
    on();
    q.addEventListener("change", on);
    return () => q.removeEventListener("change", on);
  }, []);
  return narrow;
}

export function Shell({ render, content }: ShellProps) {
  const [layout, setLayout] = useLayout();
  const edit = useStore((s) => s.layoutEdit);
  const popout = useStore((s) => s.popout);
  const sidebarCollapsed = useStore((s) => !!s.settings?.appearance.sidebarCollapsed);
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

  const narrow = useNarrowWindow();
  const dock = (d: Dock) => {
    // In a very small window the details pane and terminal at the side step aside, so the files keep their room.
    const ids = panelsAt(layout, d).filter(visible).filter((id) => !(narrow && (d === "left" || d === "right") && (id === "details" || id === "terminal")));
    if (!ids.length && !(edit && drag)) return null;
    // The least a dock may be squeezed to: what its panels need at their smallest. Below that the files give way
    // (they keep a minimum of their own), and a panel at the side steps aside in a very small window (shell.css).
    const side = d === "left" || d === "right";
    const least = ids.reduce((sum, id) => sum + (panelRange(id, d)?.[0] ?? (id === "sidebar" && side ? (sidebarCollapsed ? SIDEBAR_LIMITS.collapsed : SIDEBAR_LIMITS.min) : 0)), 0);
    const style: React.CSSProperties | undefined = least && side ? { minWidth: least } : least ? { minHeight: least } : undefined;
    return (
      <div className={`dock dock-${d}`} data-dock={d} data-empty={ids.length === 0 || undefined} style={style}>
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
