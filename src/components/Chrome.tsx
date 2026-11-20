// Window chrome: the title bar (tabs and window buttons in one row), the
// address row, the command bar and the sidebar.

import { sk } from "../util/shortcutText";
import { shellFor } from "../util/shellWords";
import { viewLevelOf } from "../util/viewLevel";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import { api, type Drive, type SavedServer, type TerminalMode } from "../api";
import {
  GROUP_COLORS,
  VIEW_LEVELS,
  createPath,
  deletePaths,
  extractAndOpenTerminal,
  openFile,
  promptText,
  showProperties,
  tabTitle,
  thisPcLabel,
  undoLast,
  redoLast,
  useStore,
} from "../store";
import { adv } from "../settings/advanced";
import { commandById, commandsFor, keyFor } from "../settings/keys";
import { driveName, driveRemote, hidesAddress } from "../util/drives";
import { deviceIcon, deviceStatus, isMainDevice, openDevice } from "../util/devices";
import { isFiltering, toSearchQuery } from "../util/filter";
import {
  HOME,
  THIS_PC,
  TRASH,
  trashLabel,
  analyzeLocation,
  clearLocation,
  archiveDisplay,
  archiveFromTyped,
  archiveUrl,
  baseName,
  crumbs,
  isAppLocation,
  isArchiveFile,
  isArchiveLoc,
  isRemote,
  maskAddresses,
  parentPath,
  parseLocation,
  searchLocation,
  type SearchMode,
} from "../util/path";
import { beginDrag, sidebarOrder, type DragPayload } from "../dnd";
import { ContextMenu, type MenuItem, type MenuState } from "./ContextMenu";
import { usePathMenu, type MenuTarget } from "./pathMenu";
import { iconMenuItems } from "./IconPicker";
import { usePulse } from "../util/pulse";
import { SIDEBAR_LIMITS } from "../util/limits";
import { splitMenuItems } from "./Split";
import { dropOnPanes, endHover, hoverPanes } from "../split/state";
import { driveSlot } from "../util/customIcons";
import { deviceSlot, deviceTarget, driveTarget, iconTarget, serverSlot, serverTarget } from "../util/iconTarget";
import { SETTING as LAYOUT_KEY, isDefault as layoutIsDefault, moveNav, parseLayout, placed, serializeLayout, zoneOf, type NavId, type Zone } from "../util/sideLayout";
import { driveMenuItems, networkMenuItems } from "./driveActions";
import { SIDE_NAMES, useChrome } from "../util/chrome";
import { toggleTerminalPanel, useTerminal } from "../terminal/state";
import { fileToolsItems } from "./toolsOpen";
import { openCount } from "./todoNotes";
import { isMobileOs } from "../util/native";
import { FileIcon, Icon } from "./Icons";
import { useTasksRunning } from "./tasksUtil";

// ---------------------------------------------------------------------------
// Tabs
// ---------------------------------------------------------------------------

/** Minimize, maximize/restore and close, drawn like Windows 11's. */
function WindowControls({ side = "right" }: { side?: "left" | "right" }) {
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    const win = getCurrentWindow();
    const sync = () => void win.isMaximized().then(setMaximized).catch(() => {});
    sync();
    const off = win.onResized(sync);
    return () => void off.then((f) => f());
  }, []);
  const win = () => getCurrentWindow();
  return (
    <div className="window-controls" data-side={side} data-look={side === "left" ? "round" : undefined}>
      <button aria-label="Minimize" title="Minimize" onClick={() => void win().minimize()}>
        <Icon name="minimize" size={12} />
      </button>
      <button aria-label={maximized ? "Restore" : "Maximize"} title={maximized ? "Restore" : "Maximize"} onClick={() => void win().toggleMaximize()}>
        <Icon name={maximized ? "restore" : "maximize"} size={12} />
      </button>
      <button className="close" aria-label="Close" title="Close" onClick={() => void win().close()}>
        <Icon name="close" size={12} />
      </button>
    </div>
  );
}

/** A slim strip with the window buttons, for when the tabs are not in the top row. */
export function CaptionStrip() {
  const os = useStore((s) => s.info?.os);
  const side = useStore((s) => (adv<string>(s.settings, "ui.windowControlsSide") === "left" ? "left" : "right"));
  return (
    <div className="title-bar caption-strip" data-tauri-drag-region data-os={os}>
      {os === "macos" && <div className="traffic-light-space" data-tauri-drag-region />}
      {os !== "macos" && side === "left" && <WindowControls side="left" />}
      <span className="caption-title" data-tauri-drag-region>FAST File Explorer</span>
      <div className="title-drag" data-tauri-drag-region />
      {os !== "macos" && side === "right" && <WindowControls />}
    </div>
  );
}

/** The window buttons inside the Finder-like and GNOME-like header row. */
function HeaderControls() {
  const os = useStore((s) => s.info?.os);
  const side = useStore((s) => (adv<string>(s.settings, "ui.windowControlsSide") === "left" ? "left" : "right"));
  if (os === "macos") return side === "left" ? <div className="traffic-light-space" data-tauri-drag-region /> : null;
  return <WindowControls side={side} />;
}

export function TabBar({ controls = true, vertical = false }: { controls?: boolean; vertical?: boolean } = {}) {
  const os = useStore((s) => s.info?.os);
  const side = useStore((s) => (adv<string>(s.settings, "ui.windowControlsSide") === "left" ? "left" : "right"));
  const incoming = useStore((s) => s.incomingTab);
  const closedCount = useStore((s) => s.closedTabs.length);
  const tabs = useStore((s) => s.tabs);
  const groups = useStore((s) => s.groups);
  const activeTab = useStore((s) => s.activeTab);
  const { activate, closeTab, closeOthers, duplicateTab, newTab, groupTab, newGroupFromTab, updateGroup } =
    useStore.getState();
  const [menu, setMenu] = useState<MenuState | null>(null);

  const tabMenu = (id: string, groupId: string | null): MenuItem[] => [
    { id: "duplicate", label: "Duplicate tab", run: () => duplicateTab(id) },
    { id: "newGroup", label: "Add to new group", separatorBefore: true, run: () => newGroupFromTab(id) },
    ...groups
      .filter((g) => g.id !== groupId)
      .map((g) => ({ id: `group-${g.id}`, label: `Add to "${g.name}"`, run: () => groupTab(id, g.id) })),
    ...(groupId ? [{ id: "ungroup", label: "Remove from group", run: () => groupTab(id, null) }] : []),
    { id: "close", label: "Close tab", shortcut: sk("closeTab"), separatorBefore: true, run: () => closeTab(id) },
    { id: "closeOthers", label: "Close other tabs", disabled: tabs.length < 2, run: () => closeOthers(id) },
    { id: "reopen", label: "Bring back closed tab", shortcut: sk("reopenTab"), disabled: closedCount === 0, run: () => useStore.getState().reopenTab() },
  ];

  const groupMenu = (id: string): MenuItem[] => {
    const g = groups.find((x) => x.id === id)!;
    return [
      {
        id: "rename",
        label: "Rename group",
        run: () => void promptText("Group name", g.name).then((name) => name && updateGroup(id, { name })),
      },
      {
        id: "color",
        label: "Next color",
        run: () => updateGroup(id, { color: GROUP_COLORS[(GROUP_COLORS.indexOf(g.color) + 1) % GROUP_COLORS.length] }),
      },
      {
        id: "ungroupAll",
        label: "Ungroup",
        separatorBefore: true,
        run: () => tabs.filter((t) => t.groupId === id).forEach((t) => groupTab(t.id, null)),
      },
    ];
  };

  const open = (e: React.MouseEvent, items: MenuItem[]) => {
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, items });
  };

  // ---- Scrolling when there are more tabs than fit ------------------------
  const barRef = useRef<HTMLDivElement>(null);
  const [more, setMore] = useState({ before: false, after: false });
  const measure = useCallback(() => {
    const el = barRef.current;
    if (!el) return;
    const [pos, size, total] = vertical ? [el.scrollTop, el.clientHeight, el.scrollHeight] : [el.scrollLeft, el.clientWidth, el.scrollWidth];
    const next = { before: pos > 1, after: pos + size < total - 1 };
    setMore((m) => (m.before === next.before && m.after === next.after ? m : next));
  }, [vertical]);
  useEffect(() => {
    const el = barRef.current;
    if (!el) return;
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [measure, tabs.length]);
  useEffect(() => {
    barRef.current?.querySelector("[data-active]")?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [activeTab, tabs.length]);
  const scrollBy = (dir: -1 | 1) => barRef.current?.scrollBy(vertical ? { top: dir * 160, behavior: "smooth" } : { left: dir * 240, behavior: "smooth" });

  // ---- Dragging a tab: reorder inside the bar, or drop outside for a new window
  const { moveTab, moveGroup, detachTab } = useStore.getState();
  const [drag, setDrag] = useState<{ id: string; group?: string; x: number; y: number; w: number; index: number; inside: boolean; split?: boolean } | null>(null);
  const suppressChipClick = useRef(false);
  const startDrag = (e: React.PointerEvent, id: string) => {
    if (e.button !== 0 || (e.target as HTMLElement).closest(".tab-close")) return;
    const [sx, sy] = [e.clientX, e.clientY];
    const w = (e.currentTarget as HTMLElement).offsetWidth;
    let started = false;
    let last = { index: 0, inside: true };
    const where = (ev: PointerEvent) => {
      const bar = barRef.current;
      if (!bar) return last;
      const r = bar.getBoundingClientRect();
      const slack = 34;
      const inside = ev.clientX >= r.left - slack && ev.clientX <= r.right + slack && ev.clientY >= r.top - slack && ev.clientY <= r.bottom + slack;
      let index = 0;
      bar.querySelectorAll<HTMLElement>(".tab[data-tab-id]").forEach((el) => {
        if (el.dataset.tabId === id) return;
        const b = el.getBoundingClientRect();
        if (vertical ? ev.clientY > b.top + b.height / 2 : ev.clientX > b.left + b.width / 2) index++;
      });
      return (last = { index, inside });
    };
    let lastHover = 0;
    let hovering = false;
    // Over the folder view of this window: the panes make room and show where the tab would go (split view).
    let splitting = false;
    const label = tabTitle(useStore.getState().tabs.find((t) => t.id === id)?.location ?? "");
    const move = (ev: PointerEvent) => {
      if (!started && Math.hypot(ev.clientX - sx, ev.clientY - sy) < 6) return;
      started = true;
      const at = where(ev);
      splitting = !at.inside && hoverPanes(ev.clientX, ev.clientY, { slot: 0, tabId: id }, label);
      if (at.inside) endHover();
      setDrag({ id, x: ev.clientX, y: ev.clientY, w, ...at, split: splitting });
      // Outside this window's tab strip: let the window underneath show where it would land.
      if (!at.inside && !splitting) {
        const t = performance.now();
        if (t - lastHover > 40) {
          lastHover = t;
          hovering = true;
          void api.tabHover(ev.screenX, ev.screenY, w, true).catch(() => {});
        }
      } else if (hovering) {
        hovering = false;
        void api.tabHover(0, 0, w, false).catch(() => {});
      }
    };
    const finish = (commit: boolean, ev?: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("keydown", key, true);
      setDrag(null);
      if (hovering) void api.tabHover(0, 0, w, false).catch(() => {});
      if (!started || !commit || !ev) return endHover();
      const at = where(ev);
      if (at.inside) {
        endHover();
        moveTab(id, at.index);
      } else if (splitting) {
        dropOnPanes({ slot: 0, tabId: id });
      } else {
        endHover();
        void detachTab(id, ev.screenX, ev.screenY);
      }
    };
    const up = (ev: PointerEvent) => finish(true, ev);
    const key = (ev: KeyboardEvent) => ev.key === "Escape" && finish(false);
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("keydown", key, true);
  };

  // ---- Dragging a group chip: the whole group moves as one block ---------------
  const startGroupDrag = (e: React.PointerEvent, groupId: string) => {
    if (e.button !== 0) return;
    const [sx, sy] = [e.clientX, e.clientY];
    const w = (e.currentTarget as HTMLElement).offsetWidth;
    let started = false;
    let last = { index: 0, inside: true };
    const where = (ev: PointerEvent) => {
      const bar = barRef.current;
      if (!bar) return last;
      const r = bar.getBoundingClientRect();
      const slack = 34;
      const inside = ev.clientX >= r.left - slack && ev.clientX <= r.right + slack && ev.clientY >= r.top - slack && ev.clientY <= r.bottom + slack;
      const rest = useStore.getState().tabs.filter((t) => t.groupId !== groupId);
      let index = 0;
      for (const t of rest) {
        const el = bar.querySelector<HTMLElement>(`.tab[data-tab-id="${t.id}"]`);
        if (!el) continue;
        const b = el.getBoundingClientRect();
        if (vertical ? ev.clientY > b.top + b.height / 2 : ev.clientX > b.left + b.width / 2) index = rest.indexOf(t) + 1;
      }
      // Never land inside another group: snap to the start of the group we are over.
      while (index > 0 && index < rest.length && rest[index].groupId && rest[index].groupId === rest[index - 1].groupId) index--;
      return (last = { index, inside });
    };
    const move = (ev: PointerEvent) => {
      if (!started && Math.hypot(ev.clientX - sx, ev.clientY - sy) < 6) return;
      started = true;
      setDrag({ id: "", group: groupId, x: ev.clientX, y: ev.clientY, w, ...where(ev) });
    };
    const finish = (commit: boolean, ev?: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("keydown", key, true);
      setDrag(null);
      if (!started) return;
      suppressChipClick.current = true; // the click that follows a drag must not collapse the group
      setTimeout(() => (suppressChipClick.current = false), 0);
      if (!commit || !ev) return;
      const at = where(ev);
      if (at.inside) moveGroup(groupId, at.index);
    };
    const up = (ev: PointerEvent) => finish(true, ev);
    const key = (ev: KeyboardEvent) => ev.key === "Escape" && finish(false);
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("keydown", key, true);
  };

  const dragged = drag ? tabs.find((t) => t.id === drag.id) : undefined;
  const draggedGroup = drag?.group ? groups.find((g) => g.id === drag.group) : undefined;
  const shadow = (key: string, width = drag?.w) => <div key={key} className="tab-drop-shadow" aria-hidden="true" style={vertical ? undefined : { width }} />;

  let lastGroup: string | null = null;
  let slot = 0;
  return (
    <div className="title-bar" data-tauri-drag-region data-os={os} data-vertical={vertical || undefined} data-controls={controls || undefined}>
     {controls && os === "macos" && <div className="traffic-light-space" data-tauri-drag-region />}
     {controls && os !== "macos" && side === "left" && <WindowControls side="left" />}
     <div className="tab-strip" data-vertical={vertical || undefined}>
      {more.before && (
        <button className="tab-scroll" aria-label="Scroll tabs back" title="Scroll tabs back" onClick={() => scrollBy(-1)}>
          <Icon name="chevron" size={11} />
        </button>
      )}
      <div
        className="tab-bar"
        role="tablist"
        ref={barRef}
        data-tauri-drag-region
        onScroll={measure}
        onWheel={(e) => {
          const el = barRef.current;
          if (el && !vertical && e.deltaY && !e.deltaX) el.scrollLeft += e.deltaY;
        }}
      >
      {tabs.map((t) => {
        const group = t.groupId ? groups.find((g) => g.id === t.groupId) : undefined;
        const chip = group && group.id !== lastGroup;
        lastGroup = group?.id ?? null;
        const hidden = group?.collapsed && t.id !== activeTab;
        const isDragged = drag?.group ? t.groupId === drag.group : drag?.id === t.id;
        const before =
          drag?.inside && !isDragged && slot === drag.index
            ? shadow(`shadow-${t.id}`)
            : !drag && incoming && slot === incoming.index
              ? shadow(`incoming-${t.id}`, incoming.w)
              : null;
        if (!isDragged) slot++;
        return (
          <div key={t.id} className="tab-slot" data-dragging={isDragged || undefined} style={group ? { ["--group-color" as string]: group.color } : undefined}>
            {before}
            {chip && (
              <button
                className="tab-group-chip"
                title={group.collapsed ? "Expand group" : "Collapse group"}
                onPointerDown={(e) => startGroupDrag(e, group.id)}
                onClick={() => !suppressChipClick.current && updateGroup(group.id, { collapsed: !group.collapsed })}
                onContextMenu={(e) => open(e, groupMenu(group.id))}
              >
                {group.name}
              </button>
            )}
            {!hidden && (
              <div
                className="tab"
                role="tab"
                data-tab-id={t.id}
                data-drop="tab"
                data-drop-path={isAppLocation(t.location) ? undefined : t.location}
                aria-selected={t.id === activeTab}
                data-active={t.id === activeTab || undefined}
                data-grouped={group ? true : undefined}
                onPointerDown={(e) => startDrag(e, t.id)}
                onMouseDown={(e) => e.button === 0 && activate(t.id)}
                onAuxClick={(e) => e.button === 1 && closeTab(t.id)}
                onContextMenu={(e) => open(e, tabMenu(t.id, t.groupId))}
                title={isAppLocation(t.location) ? tabTitle(t.location) : maskAddresses(t.location)}
              >
                <FileIcon kind={isAppLocation(t.location) ? "file" : "folder"} size={14} />
                <span className="tab-title">{tabTitle(t.location)}</span>
                <button
                  className="tab-close"
                  aria-label="Close tab"
                  onMouseDown={(e) => e.stopPropagation()}
                  onClick={() => closeTab(t.id)}
                >
                  <Icon name="close" size={10} />
                </button>
              </div>
            )}
          </div>
        );
      })}
      {drag?.inside && slot <= drag.index && shadow("shadow-end")}
      {!drag && incoming && slot <= incoming.index && shadow("incoming-end", incoming.w)}
      </div>
      {more.after && (
        <button className="tab-scroll" aria-label="Scroll tabs forward" title="Scroll tabs forward" data-forward onClick={() => scrollBy(1)}>
          <Icon name="chevron" size={11} />
        </button>
      )}
      <button className="tab-new" aria-label="New tab" title="New tab (Ctrl+T)" onClick={() => newTab()}>
        <Icon name="plus" size={14} />
      </button>
     </div>
     {!vertical && <div className="title-drag" data-tauri-drag-region />}
     {controls && os !== "macos" && side === "right" && <WindowControls />}
     {drag && draggedGroup && (
       <div className="tab-group-chip tab-ghost-chip" style={{ left: drag.x - drag.w / 2, top: drag.y - 10, ["--group-color" as string]: draggedGroup.color }}>
         {draggedGroup.name}
       </div>
     )}
     {drag && dragged && (
       <div className="tab tab-ghost" data-detach={(!drag.inside && !drag.split) || undefined} style={{ left: drag.x - drag.w / 2, top: drag.y - 17, width: drag.w }}>
         <FileIcon kind={isAppLocation(dragged.location) ? "file" : "folder"} size={14} />
         <span className="tab-title">{tabs.length < 2 && !drag.inside && !drag.split ? "The only tab stays here" : tabTitle(dragged.location)}</span>
         {drag.split ? <span className="tab-ghost-hint">{tabs.length < 2 ? "Opens a copy" : "Split here"}</span> : !drag.inside && tabs.length > 1 && <span className="tab-ghost-hint">New window</span>}
       </div>
     )}
     {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Address row: navigation, breadcrumb, search
// ---------------------------------------------------------------------------

export type ToolbarPart = "nav" | "title" | "address" | "search" | "terminal";
const ALL_PARTS: ToolbarPart[] = ["nav", "address", "search", "terminal"];

/**
 * The navigation row. By default it holds everything (Windows layout). The
 * Finder-like and GNOME-like layouts use the same row with fewer parts, put the
 * window buttons in it (`controls`), and slot other buttons in (`extra`); the path
 * bar of the Finder-like layout is a second copy showing only the address.
 */
export function Toolbar({ parts = ALL_PARTS, controls = false, extra }: { parts?: ToolbarPart[]; controls?: boolean; extra?: React.ReactNode } = {}) {
  const has = (p: ToolbarPart) => parts.includes(p);
  const controlsSide = useStore((st) => (adv<string>(st.settings, "ui.windowControlsSide") === "left" ? "left" : "right"));
  const tab = useStore((s) => s.tabs.find((t) => t.id === s.activeTab));
  const info = useStore((s) => s.info);
  const drives = useStore((s) => s.drives);
  const settings = useStore((s) => s.settings)!;
  const { navigate, back, forward, showToast } = useStore.getState();
  const [editing, setEditing] = useState(false);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const searches = useStore((s) => s.searches);
  const [boxFocused, setBoxFocused] = useState(false);
  const historyLimit = adv<number>(settings, "search.historyLimit");

  const location = tab?.location ?? HOME;
  const loc = parseLocation(location);
  const folder = loc.kind === "folder" ? loc.path : null;
  // The refresh button turns for a moment after a refresh, so it is clear it happened.
  const refreshedAt = useStore((st) => st.refreshedAt);
  const spinning = usePulse(refreshedAt, 900);
  const up = folder ? (parentPath(folder) ?? THIS_PC) : null;

  // The search box mirrors the query when a search tab is showing.
  const [query, setQuery] = useState("");
  useEffect(() => setQuery(loc.kind === "search" ? loc.query : ""), [location]); // eslint-disable-line

  // Scope for a new search: the folder being viewed, unless the user prefers everything.
  // How far it reaches: the person's choice (shortcut or the box's own menu), else the setting.
  const chosenMode = useStore((s) => s.searchMode);
  const base = folder ?? (loc.kind === "search" ? loc.scope : null);
  const wanted: SearchMode = chosenMode ?? (loc.kind === "search" ? loc.mode : settings.search.scopeToCurrentFolder ? adv<SearchMode>(settings, "search.defaultMode") : "global");
  const mode: SearchMode = base ? wanted : "global";
  const scopeRef = useRef<string | null>(null);
  scopeRef.current = mode === "global" ? null : base;
  const modeRef = useRef<SearchMode>("tree");
  modeRef.current = mode;
  /** Change how far searching reaches, and redo a search already typed. */
  const applyMode = (m: SearchMode) => {
    useStore.getState().setSearchMode(m);
    const scope = m === "global" ? null : base;
    if (query.trim()) navigate(searchLocation(query, scope, m), { replace: loc.kind === "search" });
  };

  /** Empty the box and, if a search is showing, go back to the folder. */
  const leaveSearch = () => {
    clearTimeout(timer.current);
    setQuery("");
    if (loc.kind === "search") useStore.getState().closeSearch();
  };

  const chord = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const onSearch = (text: string) => {
    setQuery(text);
    clearTimeout(timer.current);
    // Nothing typed: no search, so the folder shows as it was.
    if (!text.trim()) {
      if (loc.kind === "search") useStore.getState().closeSearch();
      return;
    }
    // Search as you type; the same tab is reused so history stays clean.
    timer.current = setTimeout(() => {
      navigate(searchLocation(text, scopeRef.current, modeRef.current), { replace: loc.kind === "search" });
    }, adv<number>(settings, "search.debounceMs"));
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const cmd = commandsFor(useStore.getState().settings, e).find((id) => commandById(id)?.scope === "nav") ?? null;
      const typing = (e.target as HTMLElement | null)?.closest?.("input, textarea, select, [contenteditable]");
      // Two copies can be on screen (header and path bar): each answers only for what it shows.
      const toAddress = cmd === "focusAddress" || cmd === "focusAddressAlt" || cmd === "focusAddressF4";
      if (toAddress) {
        if (!has("address")) return;
      } else if (!has("nav") && !has("search")) return;
      const chordOn = adv<boolean>(useStore.getState().settings, "search.chord");
      const focusBox = (m: SearchMode | null) => {
        e.preventDefault();
        if (m) applyMode(m);
        searchRef.current?.focus();
        searchRef.current?.select();
      };
      // Ctrl+R, then F: this folder and everything under it.
      if (chordOn && chord.current && Date.now() - chord.current < 1500 && e.key.toLowerCase() === "f" && !e.altKey && !e.shiftKey) {
        chord.current = 0;
        focusBox("tree");
        return;
      }
      chord.current = 0;
      if (chordOn && e.ctrlKey && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "r") {
        e.preventDefault();
        chord.current = Date.now();
        return;
      }
      if (cmd === "focusSearch") focusBox("level");
      else if (cmd === "searchGlobal") focusBox("global");
      else if (e.key === "F3") focusBox(null);
      else if (toAddress) {
        e.preventDefault();
        setEditing(true);
      } else if (cmd === "back" && !(typing && !e.altKey && !e.ctrlKey && !e.metaKey)) {
        e.preventDefault();
        back();
      } else if (cmd === "forward" && !(typing && !e.altKey && !e.ctrlKey && !e.metaKey)) {
        e.preventDefault();
        forward();
      } else if (cmd === "goUp" && up && !(typing && !e.altKey && !e.ctrlKey && !e.metaKey)) {
        e.preventDefault();
        navigate(up);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [up, back, forward, navigate, parts.join()]); // eslint-disable-line react-hooks/exhaustive-deps

  // A folder inside an archive or on a server is not a folder a terminal can open in.
  const inArchive = !!folder && isArchiveLoc(folder);
  const onServer = !!folder && isRemote(folder);
  const realFolder = folder && !inArchive && !onServer ? folder : null;
  const terminalDir = realFolder ?? info?.home ?? "/";
  const openTerminal = async (mode: TerminalMode, plain = false) => {
    try {
      const { terminal } = useStore.getState().settings!;
      let dir = terminalDir;
      // The plain button follows the settings: the home folder, or the one selected folder.
      if (plain && mode === "here") {
        if (terminal.defaultFolder === "home" && info?.home) dir = info.home;
        const sel = useStore.getState().selection;
        if (terminal.useSelectedFolder && sel.length === 1) {
          const r = await api.resolvePath(sel[0]).catch(() => null);
          if (r?.isDir) dir = r.path;
        }
      }
      if (inArchive && folder && (mode === "here" || mode === "admin")) await extractAndOpenTerminal(folder, mode);
      else await api.openTerminal(dir, mode);
    } catch (e) {
      showToast(String(e));
    }
  };
  const terminalMenu = (e: React.MouseEvent) => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const admin = info?.os === "windows" ? "as administrator" : "as root";
    setMenu({
      x: r.left,
      y: r.bottom + 4,
      items: inArchive
        ? [
            // Looking inside an archive: it is unpacked first, then the terminal opens in the unpacked folder.
            { id: "extract", label: "Unpack next to the archive, then open terminal", run: () => void extractAndOpenTerminal(folder!, "here") },
            { id: "extractHere", label: "Unpack into this folder, then open terminal", run: () => void extractAndOpenTerminal(folder!, "here", true) },
            { id: "home", label: "Open in home folder", run: () => void openTerminal("home") },
            { id: "extractAdmin", label: `Unpack, then open ${admin}`, separatorBefore: true, run: () => void extractAndOpenTerminal(folder!, "admin") },
          ]
        : [
            { id: "plain", label: "Open terminal", run: () => void openTerminal("here", true) },
            { id: "here", label: "Open in this folder", disabled: !realFolder, run: () => void openTerminal("here") },
            { id: "home", label: "Open in home folder", run: () => void openTerminal("home") },
            { id: "drive", label: "Open at drive root", disabled: !realFolder, run: () => void openTerminal("driveRoot") },
            { id: "admin", label: `Open here ${admin}`, separatorBefore: true, disabled: onServer, run: () => void openTerminal("admin") },
          ],
    });
  };

  /** This PC > Main-Dev-Drive (A:) > Projects: the drive shows its name and letter. */
  const trail: { label: string; path: string; icon?: string }[] = [{ label: "", path: THIS_PC, icon: "pc" }, { label: thisPcLabel(info?.os), path: THIS_PC }];
  if (folder && isRemote(folder)) {
    // A server starts its own trail: "tester@host › folder › subfolder".
    trail.length = 0;
    trail.push({ label: "", path: THIS_PC, icon: "pc" });
    for (const c of crumbs(folder)) trail.push({ label: c.label, path: c.path });
  } else if (folder) {
    for (const [i, c] of crumbs(folder).entries()) {
      const drive = i === 0 ? drives.find((d) => d.mount === c.path || d.mount.replace(/[\\/]$/, "") === c.path.replace(/[\\/]$/, "")) : undefined;
      trail.push({ label: drive ? driveName(drive, settings.appearance) : c.label, path: c.path });
    }
  } else if (loc.kind !== "thispc") {
    trail.length = 0;
    trail.push({ label: tabTitle(location), path: location });
  }

  return (
    <div className="toolbar" data-parts={parts.join(" ")} data-tauri-drag-region={controls || undefined}>
      {controls && controlsSide === "left" && <HeaderControls />}
      {has("nav") && (
      <div className="nav-buttons">
        <button aria-label="Back" title="Back (Alt+Left)" disabled={!tab || tab.historyIndex === 0} onClick={back}>
          <Icon name="back" />
        </button>
        <button aria-label="Forward" title="Forward (Alt+Right)" disabled={!tab || tab.historyIndex >= tab.history.length - 1} onClick={forward}>
          <Icon name="forward" />
        </button>
        <button aria-label="Up" data-nav="up" title="Up (Alt+Up)" disabled={!up} onClick={() => up && navigate(up)}>
          <Icon name="up" />
        </button>
        <button aria-label="Refresh" data-nav="refresh" data-spinning={spinning} title={`Refresh (${[keyFor(settings, "refresh"), keyFor(settings, "refreshAlt")].filter(Boolean).join(" or ")})`} onClick={() => useStore.getState().refreshTab()}>
          <Icon name="refresh" />
        </button>
      </div>
      )}

      {has("title") && (
        <div className="header-title" data-tauri-drag-region title={loc.kind === "folder" ? archiveDisplay(loc.path) : tabTitle(location)}>
          {tabTitle(location)}
        </div>
      )}

      {has("address") && (
      <div
        className="address-bar"
        data-editing={editing || undefined}
        onMouseDown={(e) => {
          // Anywhere on the bar except a breadcrumb starts typing, on every page.
          if (!editing && e.button === 0 && !(e.target as HTMLElement).closest(".crumb")) {
            e.preventDefault();
            setEditing(true);
          }
        }}
      >
        {editing ? (
          <input
            className="address-input"
            autoFocus
            defaultValue={folder ? archiveDisplay(folder) : ""}
            placeholder="Type a path"
            onFocus={(e) => e.target.select()}
            onBlur={() => setEditing(false)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && e.currentTarget.value.trim()) {
                const typed = e.currentTarget.value.trim();
                if (isRemote(typed)) {
                  // A server address: opens it, asking for the login if needed.
                  navigate(typed);
                  setEditing(false);
                  return;
                }
                // "cmd", "powershell" and the like open that shell in this folder, as Explorer does.
                const where = adv<string>(settings, "address.shellWords");
                const wanted = where === "off" ? null : shellFor(typed, info?.os);
                if (wanted) {
                  setEditing(false);
                  if (where === "builtin") void useTerminal.getState().newTab({ cwd: terminalDir }).catch((err) => showToast(String(err)));
                  else void api.openTerminal(terminalDir, "here", wanted.shell, wanted.loginShell).catch((err) => showToast(String(err)));
                  return;
                }
                const viaArchive = archiveFromTyped(typed);
                void api
                  .resolvePath(typed)
                  .then((r) => {
                    if (!r.isDir && isArchiveFile(r.path) && adv<boolean>(settings, "files.openArchives")) navigate(archiveUrl(r.path));
                    else if (r.isDir) navigate(r.path);
                    else {
                      // A file: show its folder with the file selected.
                      const dir = parentPath(r.path);
                      if (dir) {
                        navigate(dir);
                        useStore.setState({ pendingSelect: r.path });
                      }
                    }
                    setEditing(false);
                  })
                  .catch((err) => {
                    // Not on disk as typed: perhaps it goes through an archive (a.zip/docs).
                    if (viaArchive) {
                      navigate(viaArchive);
                      setEditing(false);
                    } else showToast(String(err));
                  });
              } else if (e.key === "Escape") setEditing(false);
              e.stopPropagation();
            }}
          />
        ) : (
          <div className="breadcrumbs">
            {trail.map((c, i) => (
              <span key={`${i}-${c.path}`} className="crumb-wrap">
                <button
                  className="crumb"
                  data-drop={isAppLocation(c.path) ? undefined : "folder"}
                  data-drop-path={isAppLocation(c.path) ? undefined : c.path}
                  aria-label={c.label || "This PC"}
                  onClick={(e) => {
                    e.stopPropagation();
                    navigate(c.path);
                  }}
                >
                  {c.icon ? <Icon name={c.icon} size={14} /> : c.label}
                </button>
                {i < trail.length - 1 && <Icon name="chevron" size={10} />}
              </span>
            ))}
          </div>
        )}
      </div>
      )}

      {controls && <div className="header-drag" data-tauri-drag-region />}
      {extra && <div className="header-actions">{extra}</div>}

      {has("search") && (
      <div className="search-box" onBlur={(e) => { if (!e.currentTarget.contains(e.relatedTarget as Node | null)) { setBoxFocused(false); if (loc.kind === "search") useStore.getState().addSearch(query); } }}>
        <Icon name="search" size={14} />
        <input
          ref={searchRef}
          value={query}
          placeholder={scopeRef.current ? `Search ${tabTitle(scopeRef.current)}` : `Search ${thisPcLabel(info?.os)}`}
          aria-label="Search"
          title="Try: report *.pdf ext:jpg type:folder size:>10mb"
          onChange={(e) => onSearch(e.target.value)}
          onFocus={() => setBoxFocused(true)}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              setBoxFocused(false);
              if (loc.kind === "search" || query) leaveSearch();
            }
            if (e.key === "Enter" && query.trim()) {
              clearTimeout(timer.current);
              useStore.getState().addSearch(query);
              setBoxFocused(false);
              navigate(searchLocation(query, scopeRef.current, modeRef.current), { replace: loc.kind === "search" });
            }
            e.stopPropagation();
          }}
        />
        {(query || loc.kind === "search") && (
          <button type="button" className="search-clear" aria-label="Close search" title="Close search (Esc)" onMouseDown={(e) => e.preventDefault()} onClick={leaveSearch}>
            <Icon name="close" size={11} />
          </button>
        )}
        {base && (
          <button
            type="button"
            className="search-mode"
            aria-label="How far to search"
            aria-haspopup="menu"
            title="How far searching reaches"
            onMouseDown={(e) => e.preventDefault()}
            onClick={(e) => {
              const r = e.currentTarget.getBoundingClientRect();
              setMenu({
                x: r.right - 190,
                y: r.bottom + 4,
                items: [
                  { id: "search-level", label: "This folder only", checked: mode === "level", run: () => applyMode("level") },
                  { id: "search-tree", label: "This folder and its subfolders", checked: mode === "tree", run: () => applyMode("tree") },
                  { id: "search-global", label: "Everywhere", checked: mode === "global", run: () => applyMode("global") },
                ],
              });
            }}
          >
            {mode === "level" ? "This folder" : mode === "tree" ? "+ subfolders" : "Everywhere"}
            <Icon name="expand" size={9} />
          </button>
        )}
        {boxFocused && historyLimit > 0 && searches.length > 0 && (
          <div className="search-history" role="listbox" aria-label="Previous searches">
            <div className="search-history-head">
              <span>Recent searches</span>
              <button className="link-button" onMouseDown={(e) => e.preventDefault()} onClick={() => useStore.getState().clearSearches()}>Clear</button>
            </div>
            {searches
              .filter((t) => !query.trim() || (t.toLowerCase().includes(query.trim().toLowerCase()) && t !== query))
              .slice(0, 12)
              .map((t) => (
                <div key={t} className="search-history-row" role="option" aria-selected={false}
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => { setQuery(t); setBoxFocused(false); useStore.getState().addSearch(t); navigate(searchLocation(t, scopeRef.current, modeRef.current), { replace: loc.kind === "search" }); }}>
                  <Icon name="clock" size={13} />
                  <span className="search-history-text">{t}</span>
                  <button className="search-history-x" aria-label={`Forget ${t}`} onClick={(e) => { e.stopPropagation(); useStore.getState().removeSearch(t); }}>
                    <Icon name="close" size={9} />
                  </button>
                </div>
              ))}
          </div>
        )}
      </div>
      )}
      {has("terminal") && (
      <button className="icon-button" aria-label="Open terminal" title="Open terminal" onClick={terminalMenu}>
        <Icon name="terminal" />
      </button>
      )}
      {controls && controlsSide === "right" && <HeaderControls />}
      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Command bar: labelled actions, Sort, View, Filter and the "..." menu
// ---------------------------------------------------------------------------

const VIEW_NAMES: Record<string, string> = {
  xl: "Extra large icons",
  large: "Large icons",
  medium: "Medium icons",
  small: "Small icons",
  list: "List",
  details: "Details",
};

const SORT_NAMES: [string, string][] = [
  ["name", "Name"],
  ["modified", "Date modified"],
  ["created", "Date created"],
  ["type", "Type"],
  ["extension", "Extension"],
  ["size", "Size"],
];

export function CommandBar({ onFilter, vertical = false, only, iconOnly = false }: { onFilter: (anchor: DOMRect) => void; vertical?: boolean; only?: string[]; iconOnly?: boolean }) {
  const os = useStore((s) => s.info?.os);
  const location = useStore((s) => s.tabs.find((t) => t.id === s.activeTab)?.location ?? HOME);
  const selection = useStore((s) => s.selection);
  const undoCount = useStore((s) => s.undo.length);
  const redoCount = useStore((s) => s.redo.length);
  const sort = useStore((s) => s.sort);
  const filter = useStore((s) => s.filter);
  const drives = useStore((s) => s.drives);
  const settings = useStore((s) => s.settings)!;
  const { setClipboard, setSort, updateSettings, setViewLevel, showToast } = useStore.getState();
  const [menu, setMenu] = useState<MenuState | null>(null);
  const tasksShown = adv(settings, "tasks.showButton") === true && os !== "android" && os !== "ios";
  const tasksBusy = useTasksRunning();

  const loc = parseLocation(location);
  const folder = loc.kind === "folder" ? loc.path : null;
  const inFolder = !!folder;
  const hasSel = selection.length > 0;
  const folderView = useStore((s) => s.folderView);
  const level = viewLevelOf(settings, folderView);

  const below = (e: React.MouseEvent, items: MenuItem[]) => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    setMenu({ x: r.left, y: r.bottom + 4, items });
  };

  const create = async (isFolder: boolean, name: string) => {
    if (!folder) return;
    const typed = await promptText(isFolder ? "New folder" : "New file", name);
    if (!typed) return;
    const made = await createPath(folder, typed, isFolder);
    if (made) useStore.setState({ pendingSelect: made });
  };

  const newMenu = (e: React.MouseEvent) =>
    below(e, [
      { id: "folder", label: "Folder", icon: "folderOpen", shortcut: sk("newFolder"), run: () => void create(true, "New folder") },
      { id: "text", label: "Text document", icon: "list", run: () => void create(false, "New Text Document.txt") },
      { id: "file", label: "File…", icon: "newItem", run: () => void create(false, "New file") },
    ]);

  const sortMenu = (e: React.MouseEvent) =>
    below(e, [
      ...SORT_NAMES.map(([key, label]) => ({
        id: `sort-${key}`,
        label,
        checked: sort.key === key,
        run: () => setSort({ key: key as typeof sort.key, asc: sort.key === key ? sort.asc : key !== "size" && key !== "modified" && key !== "created" }),
      })),
      { id: "asc", label: "Ascending", separatorBefore: true, checked: sort.asc, run: () => setSort({ ...sort, asc: true }) },
      { id: "desc", label: "Descending", checked: !sort.asc, run: () => setSort({ ...sort, asc: false }) },
      {
        id: "foldersFirst",
        label: "Folders first",
        separatorBefore: true,
        checked: settings.general.foldersFirst,
        run: () => updateSettings((s) => void (s.general.foldersFirst = !s.general.foldersFirst)),
      },
    ]);

  const viewMenu = (e: React.MouseEvent) =>
    below(e, [
      ...[...VIEW_LEVELS].reverse().map((name) => ({
        id: `view-${name}`,
        label: VIEW_NAMES[name],
        checked: VIEW_LEVELS[level] === name,
        run: () => setViewLevel(VIEW_LEVELS.indexOf(name)),
      })),
      ...splitMenuItems(),
      { id: "miller", label: "Miller columns", checked: adv<boolean>(settings, "view.miller"), run: () => updateSettings((s) => void (s.advanced["view.miller"] = !adv<boolean>(s, "view.miller"))) },
      { id: "choose-details", label: "Choose details…", run: () => useStore.setState({ chooseDetailsOpen: true }) },
      {
        id: "details-pane",
        label: "Details pane",
        checked: settings.appearance.showDetailsPane,
        run: () => updateSettings((s) => void (s.appearance.showDetailsPane = !s.appearance.showDetailsPane)),
      },
      { id: "hidden", label: "Hidden items", checked: settings.general.showHidden, run: () => updateSettings((s) => void (s.general.showHidden = !s.general.showHidden)) },
      { id: "ext", label: "File name extensions", checked: settings.general.showExtensions, run: () => updateSettings((s) => void (s.general.showExtensions = !s.general.showExtensions)) },
      { id: "sidebar", label: "Navigation pane", checked: !settings.appearance.sidebarCollapsed, run: () => updateSettings((s) => void (s.appearance.sidebarCollapsed = !s.appearance.sidebarCollapsed)) },
    ]);

  const share = (e: React.MouseEvent) =>
    below(e, [
      { id: "shareLink", label: "Temporary download link…", icon: "share", run: () => useStore.setState({ shareFor: selection }) },
      { id: "copyPath", label: "Copy as path", icon: "copy", separatorBefore: true, run: () => void navigator.clipboard.writeText(selection.join("\n")).then(() => showToast("Path copied.")) },
      { id: "copyName", label: "Copy name", run: () => void navigator.clipboard.writeText(selection.map(baseName).join("\n")).then(() => showToast("Name copied.")) },
      {
        id: "copyLink",
        label: "Copy file link",
        run: () =>
          void navigator.clipboard
            .writeText(selection.map((p) => `file:///${p.replace(/\\/g, "/").replace(/^\//, "")}`.replace(/ /g, "%20")).join("\n"))
            .then(() => showToast("Link copied.")),
      },
    ]);

  const more = (e: React.MouseEvent) => {
    const onThisPc = loc.kind === "thispc";
    below(e, [
      { id: "undo", label: "Undo", icon: "undo", shortcut: keyFor(settings, "undo"), disabled: undoCount === 0, run: () => void undoLast() },
      { id: "redo", label: "Redo", icon: "redo", shortcut: keyFor(settings, "redo"), disabled: redoCount === 0, run: () => void redoLast() },
      ...(onThisPc
        ? [
            ...networkMenuItems(drives, os, settings.appearance).map((m, i) => (i === 0 ? { ...m, separatorBefore: true } : m)),
          ]
        : []),
      { id: "newTab", label: "New tab", shortcut: sk("newTab"), separatorBefore: true, run: () => useStore.getState().newTab() },
      { id: "selectAll", label: "Select all", shortcut: sk("selectAll"), disabled: !inFolder, run: () => useStore.getState().runCommand("selectAll") },
      { id: "selectNone", label: "Select none", disabled: !inFolder, run: () => useStore.getState().runCommand("selectNone") },
      { id: "invert", label: "Invert selection", disabled: !inFolder, run: () => useStore.getState().runCommand("invert") },
      { id: "selectGroup", label: "Select group…", shortcut: sk("selectGroup"), disabled: !inFolder, run: () => useStore.getState().runCommand("selectGroup") },
      { id: "deselectGroup", label: "Unselect group…", shortcut: sk("deselectGroup"), disabled: !inFolder, run: () => useStore.getState().runCommand("deselectGroup") },
      { id: "restoreSelection", label: "Restore selection", shortcut: sk("restoreSelection"), disabled: !inFolder, run: () => useStore.getState().runCommand("restoreSelection") },
      { id: "todo", label: openCount(folder ?? "") ? `To-do for this folder (${openCount(folder ?? "")})…` : "To-do for this folder…", separatorBefore: true, disabled: !inFolder || !folder, run: () => useStore.setState({ todoFor: folder }) },
      { id: "fileTools", label: "File tools", disabled: !inFolder, children: fileToolsItems(selection) },
      { id: "properties", label: "Properties", icon: "info", separatorBefore: true, disabled: !hasSel && !folder, run: () => void showProperties(hasSel ? selection : folder ? [folder] : []) },
      { id: "options", label: "Options", icon: "settings", run: () => useStore.getState().openSettings() },
    ]);
  };

  const button = (id: string, label: string, icon: string, run: (e: React.MouseEvent) => void, opts?: { disabled?: boolean; caret?: boolean; on?: boolean; title?: string }) =>
    only && !only.includes(id) ? null : (
    <button key={id} className="cmd" data-cmd={id} data-on={opts?.on || undefined} disabled={opts?.disabled} title={opts?.title ?? label} onClick={run}>
      <Icon name={icon} size={16} />
      {!iconOnly && <span>{label}</span>}
      {opts?.caret && !iconOnly && <Icon name="expand" size={9} />}
    </button>
  );

  return (
    <div className="command-bar" role="toolbar" aria-label="Actions" data-vertical={vertical || undefined} data-icons={iconOnly || undefined}>
      {button("new", "New", "newItem", newMenu, { disabled: !inFolder, caret: true })}
      {!only && <span className="cmd-sep" />}
      {button("cut", "Cut", "cut", () => setClipboard({ paths: selection, op: "move" }), { disabled: !hasSel, title: "Cut (Ctrl+X)" })}
      {button("copy", "Copy", "copy", () => setClipboard({ paths: selection, op: "copy" }), { disabled: !hasSel, title: "Copy (Ctrl+C)" })}
      {button("paste", "Paste", "paste", () => folder && void useStore.getState().pasteInto(folder), { disabled: !inFolder, title: "Paste (Ctrl+V)" })}
      {button("rename", "Rename", "rename", () => useStore.getState().runCommand("rename"), { disabled: selection.length < 1, title: selection.length > 1 ? "Rename the selected items (F2)" : "Rename (F2)" })}
      {button("share", "Share", "share", share, { disabled: !hasSel, caret: true })}
      {tasksShown && button("tasks", tasksBusy > 0 ? `Tasks (${tasksBusy})` : "Tasks", "clock", () => useStore.getState().openTasks(), { title: tasksBusy > 0 ? `Tasks: ${tasksBusy} running` : "Tasks: automatic copying, moving, cleaning and backups", on: tasksBusy > 0 })}
      {button("delete", "Delete", "trash", (e) => void deletePaths(selection, e.shiftKey), { disabled: !hasSel, title: "Delete (Del) — Shift+click deletes permanently" })}
      {!only && <span className="cmd-sep" />}
      {button("sort", "Sort", "sort", sortMenu, { disabled: !inFolder, caret: true })}
      {button("view", "View", "view", viewMenu, { caret: true })}
      {button("filter", isFiltering(filter) ? "Filter •" : "Filter", "filter", (e) => onFilter((e.currentTarget as HTMLElement).getBoundingClientRect()), { disabled: !inFolder, caret: true, on: isFiltering(filter) })}
      {!only && <span className="cmd-sep" />}
      {button("more", "", "more", more, { title: "See more" })}
      {!only && <span className="cmd-grow" />}
      {button("details", "Details", "details", () => updateSettings((s) => void (s.appearance.showDetailsPane = !s.appearance.showDetailsPane)), { on: settings.appearance.showDetailsPane })}
      {button("settings", "Settings", "settings", () => useStore.getState().openSettings(), { title: "Settings (Ctrl+,)" })}
      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Filter panel
// ---------------------------------------------------------------------------

const MODIFIED: [string, string][] = [["any", "Any time"], ["today", "Today"], ["week", "Past 7 days"], ["month", "Past 30 days"], ["year", "Past year"], ["older", "Older than a year"]];
const SIZES: [string, string][] = [["any", "Any size"], ["empty", "Empty (0 B)"], ["small", "Small (under 1 MB)"], ["medium", "Medium (1 – 100 MB)"], ["large", "Large (over 100 MB)"]];
const KIND_LABEL: Record<string, string> = { folder: "Folders", image: "Pictures", video: "Videos", audio: "Music", archive: "Archives", code: "Code", doc: "Documents", sheet: "Spreadsheets", slides: "Presentations", exe: "Programs", file: "Other files" };

export function FilterPanel({ anchor, onClose }: { anchor: DOMRect; onClose: () => void }) {
  const filter = useStore((s) => s.filter);
  const facets = useStore((s) => s.facets);
  const location = useStore((s) => s.tabs.find((t) => t.id === s.activeTab)?.location ?? HOME);
  const { setFilter, navigate } = useStore.getState();
  const loc = parseLocation(location);
  const folder = loc.kind === "folder" ? loc.path : loc.kind === "search" ? loc.scope : null;

  const toggle = (list: string[], v: string) => (list.includes(v) ? list.filter((x) => x !== v) : [...list, v]);

  // "Include subfolders" is answered by the search engine, scoped to this folder.
  const setSubfolders = (on: boolean) => {
    if (!folder) return;
    const next = { ...useStore.getState().filter, subfolders: on };
    setFilter({ subfolders: on });
    navigate(on ? searchLocation(toSearchQuery(next), folder) : folder);
  };
  const change = (patch: Partial<typeof filter>) => {
    setFilter(patch);
    const next = { ...useStore.getState().filter };
    if (next.subfolders && folder) navigate(searchLocation(toSearchQuery(next), folder), { replace: true });
  };

  const left = Math.max(8, Math.min(anchor.left, window.innerWidth - 336));
  return (
    <div className="menu-backdrop" onMouseDown={onClose}>
      <div className="filter-panel" style={{ left, top: anchor.bottom + 4 }} onMouseDown={(e) => e.stopPropagation()} onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <label className="filter-field">
          <span>Name contains</span>
          <input className="text-input" autoFocus value={filter.text} placeholder="Type to filter" onChange={(e) => change({ text: e.target.value })} />
        </label>
        {facets.kinds.length > 0 && (
          <div className="filter-group">
            <h4>Type</h4>
            <div className="chips">
              {facets.kinds.map(([k, n]) => (
                <button key={k} className="chip" data-on={filter.kinds.includes(k) || undefined} onClick={() => change({ kinds: toggle(filter.kinds, k) })}>
                  {KIND_LABEL[k] ?? k} <span className="muted">{n}</span>
                </button>
              ))}
            </div>
          </div>
        )}
        {facets.extensions.length > 1 && (
          <div className="filter-group">
            <h4>File extension</h4>
            <div className="chips">
              {facets.extensions.map(([x, n]) => (
                <button key={x} className="chip" data-on={filter.extensions.includes(x) || undefined} onClick={() => change({ extensions: toggle(filter.extensions, x) })}>
                  .{x} <span className="muted">{n}</span>
                </button>
              ))}
            </div>
          </div>
        )}
        <div className="filter-row">
          <label>
            <span>Date modified</span>
            <select value={filter.modified} onChange={(e) => change({ modified: e.target.value })}>
              {MODIFIED.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
            </select>
          </label>
          <label>
            <span>Size</span>
            <select value={filter.size} onChange={(e) => change({ size: e.target.value })}>
              {SIZES.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
            </select>
          </label>
        </div>
        <div className="filter-not" role="group" aria-label="Leave out (is not)">
          <h4>Leave out (is not)</h4>
          <label className="filter-field">
            <span>Name does not contain</span>
            <input className="text-input" value={filter.notText} placeholder="e.g. backup" onChange={(e) => change({ notText: e.target.value })} />
          </label>
          {facets.kinds.length > 0 && (
            <div className="filter-group">
              <h5>Type is not</h5>
              <div className="chips">
                {facets.kinds.map(([k, n]) => (
                  <button key={k} className="chip" data-not data-on={filter.notKinds.includes(k) || undefined} onClick={() => change({ notKinds: toggle(filter.notKinds, k) })}>
                    {KIND_LABEL[k] ?? k} <span className="muted">{n}</span>
                  </button>
                ))}
              </div>
            </div>
          )}
          {facets.extensions.length > 1 && (
            <div className="filter-group">
              <h5>Extension is not</h5>
              <div className="chips">
                {facets.extensions.map(([x, n]) => (
                  <button key={x} className="chip" data-not data-on={filter.notExtensions.includes(x) || undefined} onClick={() => change({ notExtensions: toggle(filter.notExtensions, x) })}>
                    .{x} <span className="muted">{n}</span>
                  </button>
                ))}
              </div>
            </div>
          )}
          <div className="filter-row">
            <label>
              <span>Date modified is not</span>
              <select value={filter.notModified} onChange={(e) => change({ notModified: e.target.value })}>
                {MODIFIED.map(([v, l]) => <option key={v} value={v}>{v === "any" ? "(nothing left out)" : l}</option>)}
              </select>
            </label>
            <label>
              <span>Size is not</span>
              <select value={filter.notSize} onChange={(e) => change({ notSize: e.target.value })}>
                {SIZES.map(([v, l]) => <option key={v} value={v}>{v === "any" ? "(nothing left out)" : l}</option>)}
              </select>
            </label>
          </div>
        </div>
        <label className="filter-check">
          <input type="checkbox" checked={filter.subfolders} disabled={!folder} onChange={(e) => setSubfolders(e.target.checked)} />
          Include subfolders
        </label>
        <div className="dialog-buttons">
          <button className="secondary" onClick={() => { setFilter(null); if (filter.subfolders && folder) navigate(folder); }}>Clear filter</button>
          <button className="primary" onClick={onClose}>Done</button>
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Sidebar: resizable, collapsible to icons, sections can be hidden in Settings
// ---------------------------------------------------------------------------

const COLLAPSED_WIDTH = SIDEBAR_LIMITS.collapsed;
const MIN_WIDTH = SIDEBAR_LIMITS.min;
const MAX_WIDTH = SIDEBAR_LIMITS.max;

/** The address a saved server opens at. */
export function serverUrl(s: SavedServer): string {
  const scheme = s.protocol === "dav" ? (s.tls ? "davs" : "dav") : s.protocol;
  const port = s.port === (s.protocol === "ftp" ? 21 : s.protocol === "sftp" ? 22 : s.tls ? 443 : 80) ? "" : `:${s.port}`;
  const user = s.user ? `${s.user}@` : "";
  const path = s.path ? (s.path.startsWith("/") ? s.path : `/${s.path}`) : "/";
  return `${scheme}://${user}${s.host}${port}${path}`;
}

export function Sidebar({ horizontal = false }: { horizontal?: boolean }) {
  const pins = useStore((s) => s.pins);
  const places = useStore((s) => s.places);
  const drives = useStore((s) => s.drives);
  const frequent = useStore((s) => s.frequent);
  const recent = useStore((s) => s.recent);
  const servers = useStore((s) => s.servers);
  const info = useStore((s) => s.info);
  const appearance = useStore((s) => s.settings!.appearance);
  const settings = useStore((s) => s.settings);
  const location = useStore((s) => s.tabs.find((t) => t.id === s.activeTab)?.location);
  const { navigate, newTab, addRecent, showToast, updateSettings } = useStore.getState();
  const [menu, setMenu] = useState<MenuState | null>(null);
  const openPathMenu = usePathMenu(setMenu);
  const [dragWidth, setDragWidth] = useState<number | null>(null);
  const terminalOpen = useTerminal((t) => t.open);

  const chrome = useChrome();
  const nm = (t: string) => SIDE_NAMES[chrome][t] ?? t;
  const collapsed = appearance.sidebarCollapsed && !horizontal;
  const width = collapsed ? COLLAPSED_WIDTH : (dragWidth ?? appearance.sidebarWidth);

  // The Frequently used list follows what you open; refresh it when you navigate.
  useEffect(() => {
    const t = setTimeout(() => useStore.getState().refreshFrequent(), 600);
    return () => clearTimeout(t);
  }, [location]);

  const startResize = (e: React.PointerEvent) => {
    e.preventDefault();
    const startX = e.clientX;
    const startW = collapsed ? COLLAPSED_WIDTH : appearance.sidebarWidth;
    const target = e.currentTarget;
    target.setPointerCapture(e.pointerId);
    let latest = startW;
    const move = (ev: PointerEvent) => {
      latest = startW + ev.clientX - startX;
      setDragWidth(Math.max(COLLAPSED_WIDTH, Math.min(MAX_WIDTH, latest)));
    };
    const up = () => {
      target.removeEventListener("pointermove", move as EventListener);
      target.removeEventListener("pointerup", up);
      setDragWidth(null);
      // Dragged nearly shut: fold down to icons. Otherwise remember the width.
      if (latest < MIN_WIDTH - 30) updateSettings((s) => void (s.appearance.sidebarCollapsed = true));
      else updateSettings((s) => void ((s.appearance.sidebarCollapsed = false), (s.appearance.sidebarWidth = Math.max(MIN_WIDTH, Math.min(MAX_WIDTH, Math.round(latest))))));
    };
    target.addEventListener("pointermove", move as EventListener);
    target.addEventListener("pointerup", up);
  };

  const item = (
    key: string,
    label: string,
    target: string,
    icon: React.ReactNode,
    extra?: { isFile?: boolean; menu?: MenuItem[]; sub?: string; tip?: string; drop?: string; pinIndex?: number; drag?: DragPayload; onOpen?: () => void; navId?: NavId; pathMenu?: MenuTarget },
  ) => (
    <button
      key={key}
      className="side-item"
      data-nav-item={extra?.navId}
      data-drop={extra?.drop}
      data-drop-path={extra?.drop && !extra.isFile ? target : undefined}
      data-pin-index={extra?.pinIndex}
      onPointerDown={extra?.drag ? (e) => beginDrag(e, () => extra.drag!) : undefined}
      data-active={location === target || undefined}
      title={extra?.tip ?? (isAppLocation(target) ? label : `${label}\n${maskAddresses(target)}`)}
      onClick={() => {
        if (extra?.onOpen) extra.onOpen();
        else if (extra?.isFile) {
          addRecent(target, false);
          void openFile(target);
        } else navigate(target);
      }}
      onAuxClick={(e) => e.button === 1 && !extra?.isFile && !extra?.onOpen && newTab(target, { background: true })}
      onContextMenu={(e) => {
        // Folders and files get the same menu as in a folder window.
        if (extra?.pathMenu) return openPathMenu(e, extra.pathMenu, extra.menu ?? []);
        e.preventDefault();
        const items: MenuItem[] = [
          ...(extra?.isFile || extra?.onOpen ? [] : [{ id: "newTab", label: "Open in new tab", run: () => newTab(target) }]),
          ...(extra?.menu ?? []),
        ];
        if (items.length) setMenu({ x: e.clientX, y: e.clientY, items });
      }}
    >
      {icon}
      {!collapsed && <span className="side-label">{label}</span>}
      {!collapsed && extra?.sub && <span className="side-sub">{extra.sub}</span>}
    </button>
  );

  // Right-click a heading: how many rows show before the section scrolls, or hide it.
  const sectionMenu = (e: React.MouseEvent, rowsKey: string, flag: "showFrequent" | "showPinned" | "showDrives") => {
    e.preventDefault();
    const current = adv<number>(useStore.getState().settings, rowsKey);
    setMenu({
      x: e.clientX,
      y: e.clientY,
      items: [
        ...[3, 5, 8, 12, 20, 0].map((n) => ({
          id: `rows-${n}`,
          label: n === 0 ? "Show all rows" : `Show ${n} rows, then scroll`,
          checked: current === n,
          run: () => updateSettings((st) => void (st.advanced = { ...st.advanced, [rowsKey]: n })),
        })),
        ...(flag === "showDrives" ? networkMenuItems(drives, info?.os, appearance).map((m, i) => (i === 0 ? { ...m, separatorBefore: true } : m)) : []),
        { id: "hide", label: "Hide this section", separatorBefore: true, run: () => updateSettings((st) => void (st.appearance[flag] = false)) },
      ],
    });
  };
  // Sections can be folded to just their heading; the choice is remembered.
  const folded = new Set(String(settings?.advanced?.["sidebar.folded"] ?? "").split(",").filter(Boolean));
  const toggleFold = (id: string) =>
    updateSettings((st) => {
      const next = new Set(String(st.advanced?.["sidebar.folded"] ?? "").split(",").filter(Boolean));
      next.has(id) ? next.delete(id) : next.add(id);
      st.advanced = { ...st.advanced, ["sidebar.folded"]: [...next].join(",") };
    });
  const heading = (text: string, rowsKey?: string, flag?: "showFrequent" | "showPinned" | "showDrives", id?: string, onMenu?: (e: React.MouseEvent) => void) =>
    collapsed ? (
      <div className="side-divider" key={text} />
    ) : (
      <div
        className="side-heading"
        key={text}
        data-drop={id === "pinned" ? "pins" : undefined}
        data-pin-count={id === "pinned" ? pins.length : undefined}
        onPointerDown={id ? (e) => beginDrag(e, () => ({ paths: [], dirs: [], section: id, label: text })) : undefined}
        onClick={id ? () => toggleFold(id) : undefined}
        onContextMenu={onMenu ?? (rowsKey && flag ? (e) => sectionMenu(e, rowsKey, flag) : undefined)}
        title={rowsKey ? `${folded.has(id ?? "") ? "Click to expand" : "Click to collapse"} · drag to move · right-click for options` : undefined}
        aria-expanded={id ? !folded.has(id) : undefined}
      >
        {id && <span className="side-fold" data-folded={folded.has(id) || undefined}><Icon name="chevron" size={9} /></span>}
        {nm(text)}
      </div>
    );
  /** A section's rows, with their own scroll bar past the chosen number. */
  const section = (rowsKey: string, rows: React.ReactNode) => {
    const n = adv<number>(useStore.getState().settings, rowsKey);
    const isPins = rowsKey === "sidebar.pinnedRows";
    return (
      <div className="side-section" data-drop={isPins ? "pins" : undefined} data-pin-count={isPins ? pins.length : undefined} data-scroll={n > 0 || undefined} style={n > 0 ? { maxHeight: `calc(${n} * var(--side-row, 30px))` } : undefined}>
        {rows}
      </div>
    );
  };
  // ---- where the five fixed entries are shown ----
  const layout = parseLayout(settings?.advanced?.[LAYOUT_KEY]);
  const setAdv = (key: string, value: boolean | number | string) => updateSettings((st) => void (st.advanced = { ...st.advanced, [key]: value }));
  const showRecents = adv<boolean>(settings, "recents.show");
  const shownZones = new Set<Zone>(["top", ...(appearance.showFrequent ? ["frequent" as const] : []), ...(showRecents ? ["recents" as const] : []), ...(appearance.showPinned ? ["pinned" as const] : []), ...(appearance.showDrives ? ["drives" as const] : [])]);
  const hiddenNav = new Set<NavId>([
    ...(adv<boolean>(settings, "sidebar.showTrash") ? [] : ["trash" as const]),
    ...(adv<boolean>(settings, "sidebar.showAnalyze") ? [] : ["analyze" as const]),
    ...(adv<boolean>(settings, "sidebar.showClear") ? [] : ["clear" as const]),
  ]);
  const where = placed(layout, shownZones, hiddenNav);
  const move = (id: NavId, zone: Zone) => setAdv(LAYOUT_KEY, serializeLayout(moveNav(layout, id, zone)));
  const NAV_KEY: Partial<Record<NavId, string>> = { trash: "sidebar.showTrash", analyze: "sidebar.showAnalyze", clear: "sidebar.showClear" };
  const navEntry = (id: NavId): React.ReactNode => {
    const spec: Record<NavId, [string, string, string, string?]> = {
      home: ["Home", HOME, "home"],
      thispc: [thisPcLabel(info?.os), THIS_PC, "pc"],
      trash: [trashLabel(info?.os), TRASH, "trash", `${trashLabel(info?.os)}: what was deleted, and Put back`],
      analyze: ["Disk usage", info?.home ? analyzeLocation(info.home) : "", "chart", "Disk usage"],
      clear: ["Smart Clear", clearLocation(null), "broom", "Smart Clear: free up space"],
    };
    const [label, target, icon, tip] = spec[id];
    if (!target) return null;
    const placedAt = zoneOf(layout, id);
    const menuItems: MenuItem[] = [
      ...(placedAt !== "top" ? [{ id: "nav-top", label: "Move back to the top", run: () => move(id, "top") }] : []),
      ...(layoutIsDefault(layout) ? [] : [{ id: "nav-reset", label: "Put all of these back at the top", run: () => setAdv(LAYOUT_KEY, "") }]),
      ...iconMenuItems(iconTarget(icon, label)).map((m, i) => (i === 0 ? { ...m, separatorBefore: true } : m)),
      ...(NAV_KEY[id] ? [{ id: "nav-hide", label: `Hide ${label}`, separatorBefore: true, run: () => setAdv(NAV_KEY[id]!, false) }] : []),
    ];
    return item(`nav-${id}`, label, target, <Icon name={icon} />, { tip, navId: id, drag: { paths: [], dirs: [], navItem: id, label }, menu: menuItems });
  };
  const navRows = (zone: Zone) => where[zone].map(navEntry);

  // Right-click Frequently used or Recents: the settings that shape them, without opening Settings.
  const radio = (key: string, values: [number | string, string][], current: number | string) =>
    values.map(([v, label]) => ({ id: `${key}-${v}`, label, checked: current === v, run: () => setAdv(key, v) }));
  const rowChoices = (rowsKey: string): MenuItem[] =>
    radio(rowsKey, [[3, "3 rows"], [5, "5 rows"], [8, "8 rows"], [12, "12 rows"], [20, "20 rows"], [0, "All rows"]], adv<number>(settings, rowsKey));
  const moreSettings = (): MenuItem => ({
    id: "more",
    label: "More settings…",
    separatorBefore: true,
    run: () => {
      useStore.setState({ settingsSection: "Navigation pane" });
      useStore.getState().openSettings();
    },
  });
  const frequentMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    const a = <T extends boolean | number | string>(k: string) => adv<T>(settings, k);
    setMenu({
      x: e.clientX,
      y: e.clientY,
      items: [
        { id: "f-window", label: "Count the last…", children: radio("frequent.window", [[50, "50 navigations"], [100, "100 navigations"], [300, "300 navigations"], [1000, "1000 navigations"], [0, "Everything remembered"]], a<number>("frequent.window")) },
        { id: "f-rank", label: "Rank by…", children: radio("frequent.timeWeight", [[0, "How many times opened"], [25, "Mostly times opened, some time open"], [50, "Both equally"], [75, "Mostly time open"], [100, "Only how long it stayed open"]], a<number>("frequent.timeWeight")) },
        { id: "f-recency", label: "Newest navigations count…", children: radio("frequent.recencyBias", [[0, "The same as older ones"], [30, "A little more"], [100, "Twice as much"]], a<number>("frequent.recencyBias")) },
        { id: "f-min", label: "Opened at least…", children: radio("frequent.minVisits", [[1, "1 time"], [2, "2 times"], [3, "3 times"], [5, "5 times"], [10, "10 times"]], a<number>("frequent.minVisits")) },
        { id: "f-drag", label: "Folders dragged in count as…", children: radio("frequent.dragCount", [[1, "1 open"], [3, "3 opens"], [5, "5 opens"], [10, "10 opens"]], a<number>("frequent.dragCount")) },
        { id: "f-count", label: "Folders listed", separatorBefore: true, children: radio("files.frequentCount", [[5, "5"], [10, "10"], [15, "15"], [20, "20"], [30, "30"]], a<number>("files.frequentCount")) },
        { id: "f-rows", label: "Rows before it scrolls", children: rowChoices("sidebar.frequentRows") },
        moreSettings(),
        { id: "hide", label: "Hide this section", run: () => updateSettings((st) => void (st.appearance.showFrequent = false)) },
      ],
    });
  };
  const recentsMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    const a = <T extends boolean | number | string>(k: string) => adv<T>(settings, k);
    setMenu({
      x: e.clientX,
      y: e.clientY,
      items: [
        { id: "r-kind", label: "List…", children: radio("recents.kind", [["folders", "Folders"], ["files", "Files"], ["both", "Folders and files"]], a<string>("recents.kind")) },
        { id: "r-count", label: "Entries kept", children: radio("recents.count", [[5, "5"], [8, "8"], [10, "10"], [15, "15"], [25, "25"], [50, "50"]], a<number>("recents.count")) },
        { id: "r-rows", label: "Rows before it scrolls", children: rowChoices("sidebar.recentsRows") },
        { id: "r-clear", label: "Clear the list", separatorBefore: true, run: () => useStore.getState().clearRecent() },
        moreSettings(),
        { id: "hide", label: "Hide this section", run: () => setAdv("recents.show", false) },
      ],
    });
  };
  const recentKind = adv<string>(settings, "recents.kind");
  const recentShown = recent
    .filter((r) => !isAppLocation(r.path) && (recentKind === "both" || (recentKind === "files" ? !r.isDir : r.isDir)))
    .slice(0, adv<number>(settings, "recents.count"));

  const devices = useStore((s) => s.devices);
  const driveIcon = (d: Drive) => <Icon name={d.media === "network" ? "network" : "drive"} slot={driveSlot(d.mount)} />;
  const pinnedPaths = new Set(pins.map((p) => p.path));
  const frequentShown = frequent.filter((p) => !pinnedPaths.has(p)).slice(0, adv<number>(useStore.getState().settings, "files.frequentCount"));

  const blocks: Record<string, React.ReactNode> = {
    frequent: (
      <div key="frequent" className="side-group" data-drop="section" data-section="frequent" data-nav-zone="frequent">
        {appearance.showFrequent && (
        <>
          {heading("Frequently used", "sidebar.frequentRows", "showFrequent", "frequent", frequentMenu)}
          {!folded.has("frequent") && frequentShown.length === 0 && where.frequent.length === 0 && !collapsed && <div className="side-empty">Folders you open often show up here. You can also drag one in.</div>}
          {!folded.has("frequent") && (frequentShown.length > 0 || where.frequent.length > 0) && section(
            "sidebar.frequentRows",
            [
              ...navRows("frequent"),
              ...frequentShown.map((p) =>
                item(`freq-${p}`, baseName(p) || p, p, <FileIcon kind="folder" />, {
                  drop: "folder",
                  drag: { paths: [p], dirs: [true], label: baseName(p) || p },
                  pathMenu: { path: p, isDir: true },
                  menu: [{ id: "forget", label: "Remove from Frequently used", run: () => useStore.getState().forgetFrequent(p) }],
                }),
              ),
            ],
          )}
        </>
      )}
      </div>
    ),
    recents: (
      <div key="recents" className="side-group" data-drop="section" data-section="recents" data-nav-zone="recents">
        {showRecents && (
        <>
          {heading("Recents", "sidebar.recentsRows", undefined, "recents", recentsMenu)}
          {!folded.has("recents") && recentShown.length === 0 && where.recents.length === 0 && !collapsed && <div className="side-empty">Nothing opened yet.</div>}
          {!folded.has("recents") && (recentShown.length > 0 || where.recents.length > 0) && section(
            "sidebar.recentsRows",
            [
              ...navRows("recents"),
              ...recentShown.map((r) =>
                item(`recent-${r.path}`, baseName(r.path) || r.path, r.path, <FileIcon kind={r.isDir ? "folder" : "file"} name={r.isDir ? undefined : baseName(r.path)} />, {
                  isFile: !r.isDir,
                  drop: r.isDir ? "folder" : undefined,
                  drag: { paths: [r.path], dirs: [r.isDir], label: baseName(r.path) || r.path },
                  pathMenu: { path: r.path, isDir: r.isDir },
                  menu: [{ id: "unrecent", label: "Remove from Recents", run: () => useStore.getState().removeRecent(r.path) }],
                }),
              ),
            ],
          )}
        </>
      )}
      </div>
    ),
    pinned: (
      <div key="pinned" className="side-group" data-drop="section" data-section="pinned" data-nav-zone="pinned">
        {appearance.showPinned && (
        <>
          {heading("Pinned", "sidebar.pinnedRows", "showPinned", "pinned")}
          {!folded.has("pinned") && pins.length === 0 && where.pinned.length === 0 && !collapsed && <div className="side-empty" data-drop="pins" data-pin-count={0}>Drag a folder here to pin it.</div>}
          {!folded.has("pinned") && section(
            "sidebar.pinnedRows",
            [
              ...navRows("pinned"),
              ...pins.map((p, i) =>
                item(`pin-${p.path}`, p.label, p.path, <FileIcon kind={p.isFile ? "file" : "folder"} name={p.isFile ? p.label : places.some((pl) => pl.path === p.path) ? p.label : undefined} />, {
                  isFile: p.isFile,
                  drop: "pinitem",
                  pinIndex: i,
                  drag: { paths: [p.path], dirs: [!p.isFile], pinIndex: i, label: p.label },
                  pathMenu: { path: p.path, isDir: !p.isFile },
                }),
              ),
            ],
          )}
        </>
      )}
      </div>
    ),
    drives: (
      <div key="drives" className="side-group" data-drop="section" data-section="drives" data-nav-zone="drives">
        {appearance.showDrives && (
        <>
          {heading("Drives", "sidebar.drivesRows", "showDrives", "drives")}
          {!folded.has("drives") && section(
            "sidebar.drivesRows",
            [...navRows("drives"), ...drives.map((d) =>
              item(`drive-${d.mount}`, driveName(d, appearance), d.mount, driveIcon(d), {
                drop: d.online ? "folder" : undefined,
                sub: d.online ? undefined : d.removable ? "no media" : "offline",
                tip: `${driveName(d, appearance)}\n${hidesAddress(d, appearance) ? maskAddresses(d.mount, true) : d.mount}${driveRemote(d, appearance) ? `\n${d.remote}` : ""}`,
                menu: [
                  { id: "open", label: "Open", run: () => navigate(d.mount), disabled: !d.online },
                  ...driveMenuItems(d, { os: info?.os, drives, appearance }).map((m, i) => (i === 0 ? { ...m, separatorBefore: true } : m)),
                  ...iconMenuItems(driveTarget(d.mount, driveName(d, appearance), d.media)).map((m, i) => (i === 0 ? { ...m, separatorBefore: true } : m)),
                ],
              }),
            ),
            // Phones and cameras sit with the drives.
            ...devices.filter(isMainDevice).map((d) =>
              item(`device-${d.id}`, d.name, d.path ?? `device:${d.id}`, <Icon name={deviceIcon(d)} slot={deviceSlot(d.id)} />, {
                onOpen: () => void openDevice(d),
                tip: `${d.name}\n${deviceStatus(d)}${d.note ? `\n${d.note}` : ""}`,
                menu: [
                  { id: "open", label: d.path ? "Open" : d.connectable ? "Connect and open" : "Open", run: () => void openDevice(d) },
                  ...iconMenuItems(deviceTarget(d.id, d.name, deviceIcon(d))).map((m, i) => (i === 0 ? { ...m, separatorBefore: true } : m)),
                ],
              }),
            )],
          )}
        </>
      )}
      </div>
    ),
  };

  return (
    <div className="sidebar-wrap" data-horizontal={horizontal || undefined} style={horizontal ? undefined : { width, minWidth: collapsed ? COLLAPSED_WIDTH : MIN_WIDTH, maxWidth: collapsed ? COLLAPSED_WIDTH : `min(${MAX_WIDTH}px, 45vw)` }}>
    <nav className="sidebar" aria-label="Places" data-collapsed={(!horizontal && collapsed) || undefined} data-horizontal={horizontal || undefined} data-scrollbar={adv<string>(settings, "sidebar.scrollbar")}>
      <div className="side-top" data-nav-zone="top">
        <button
          className="side-item side-toggle"
          title={collapsed ? "Expand the navigation pane" : "Collapse to icons"}
          aria-label={collapsed ? "Expand the navigation pane" : "Collapse the navigation pane"}
          onClick={() => updateSettings((s) => void (s.appearance.sidebarCollapsed = !s.appearance.sidebarCollapsed))}
        >
          <Icon name="sidebar" />
          {!collapsed && <span className="side-label">Navigation</span>}
        </button>
        {navRows("top")}
      </div>

      {sidebarOrder(settings).map((id) => (
        <Fragment key={id}>
          {blocks[id]}
          {/* The strip below a section: entries can be dropped here to sit between sections. */}
          <div className="side-gap" data-nav-zone={`below:${id}`} data-has={where[`below:${id}` as Zone].length || undefined}>
            {navRows(`below:${id}` as Zone)}
          </div>
        </Fragment>
      ))}

      <div className="side-group" key="servers">
        {collapsed ? <div className="side-divider" /> : (
          <div className="side-heading" aria-expanded={!folded.has("servers")} title={folded.has("servers") ? "Click to expand" : "Click to collapse"} onClick={() => toggleFold("servers")}>
            <span className="side-fold" data-folded={folded.has("servers") || undefined}><Icon name="chevron" size={9} /></span>
            {nm("Servers")}
          </div>
        )}
        {(!folded.has("servers") || collapsed) && <div className="side-section">
          {servers.map((sv) => {
            const url = serverUrl(sv);
            return item(`server-${sv.name}`, sv.name, url, <Icon name="network" slot={serverSlot(sv.name)} />, {
              sub: undefined,
              drop: "folder",
              tip: `${sv.name}\n${maskAddresses(url)}`,
              menu: [
                { id: "server-login", label: "Log in again…", run: () => useStore.getState().openConnect({ saved: sv }) },
                { id: "server-logout", label: "Log out", run: () => void api.remoteDisconnect(url).then(() => showToast(`Logged out of ${sv.name}.`)).catch((e) => showToast(String(e))) },
                { id: "server-forget", label: "Remove from the list", separatorBefore: true, run: () => void api.serversRemove(sv.name).then(useStore.getState().setServers).catch((e) => showToast(String(e))) },
                ...iconMenuItems(serverTarget(sv.name)).map((m, i) => (i === 0 ? { ...m, separatorBefore: true } : m)),
              ],
            });
          })}
          <button className="side-item" title="Connect to an FTP, SFTP or WebDAV server" onClick={() => useStore.getState().openConnect()}>
            <Icon name="network" />
            {!collapsed && <span className="side-label">Connect to server…</span>}
          </button>
        </div>}
      </div>
      {!isMobileOs(info?.os) && (
        <div className="side-group side-terminal" key="terminal">
          {collapsed && <div className="side-divider" />}
          <button className="side-item" data-active={terminalOpen || undefined} title="Show or hide the built-in terminal (Ctrl+`)" aria-pressed={terminalOpen} onClick={toggleTerminalPanel}>
            <Icon name="terminal" />
            {!collapsed && <span className="side-label">Terminal</span>}
          </button>
        </div>
      )}
      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
    </nav>
    {!horizontal && <div
      className="sidebar-resizer"
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize navigation pane"
      onPointerDown={startResize}
      onDoubleClick={() => updateSettings((s) => void (s.appearance.sidebarCollapsed = !s.appearance.sidebarCollapsed))}
    />}
    </div>
  );
}
