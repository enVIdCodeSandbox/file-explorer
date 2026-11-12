// Window chrome: the title bar (tabs and window buttons in one row), the
// address row, the command bar and the sidebar.

import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, type Drive, type TerminalMode } from "../api";
import {
  GROUP_COLORS,
  VIEW_LEVELS,
  ask,
  createPath,
  deletePaths,
  promptText,
  showProperties,
  tabOrigin,
  tabTitle,
  thisPcLabel,
  undoLast,
  useStore,
} from "../store";
import { adv } from "../settings/advanced";
import { driveName, driveRemote } from "../util/drives";
import { isFiltering, toSearchQuery } from "../util/filter";
import {
  HOME,
  THIS_PC,
  analyzeLocation,
  baseName,
  crumbs,
  isAppLocation,
  parentPath,
  parseLocation,
  searchLocation,
} from "../util/path";
import { ContextMenu, type MenuItem, type MenuState } from "./ContextMenu";
import { FileIcon, Icon } from "./Icons";

// ---------------------------------------------------------------------------
// Tabs
// ---------------------------------------------------------------------------

/** Minimize, maximize/restore and close, drawn like Windows 11's. */
function WindowControls() {
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
    <div className="window-controls">
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
  return (
    <div className="title-bar caption-strip" data-tauri-drag-region data-os={os}>
      {os === "macos" && <div className="traffic-light-space" data-tauri-drag-region />}
      <span className="caption-title" data-tauri-drag-region>FAST File Explorer</span>
      <div className="title-drag" data-tauri-drag-region />
      {os !== "macos" && <WindowControls />}
    </div>
  );
}

export function TabBar({ controls = true, vertical = false }: { controls?: boolean; vertical?: boolean } = {}) {
  const os = useStore((s) => s.info?.os);
  const tabs = useStore((s) => s.tabs);
  const groups = useStore((s) => s.groups);
  const activeTab = useStore((s) => s.activeTab);
  const secondary = useStore((s) => s.isSecondary);
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
    { id: "close", label: "Close tab", shortcut: "Ctrl+W", separatorBefore: true, run: () => closeTab(id) },
    { id: "closeOthers", label: "Close other tabs", disabled: tabs.length < 2, run: () => closeOthers(id) },
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
        label: "Next colour",
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
  const { moveTab, detachTab } = useStore.getState();
  const [drag, setDrag] = useState<{ id: string; x: number; y: number; w: number; index: number; inside: boolean } | null>(null);
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
    const move = (ev: PointerEvent) => {
      if (!started && Math.hypot(ev.clientX - sx, ev.clientY - sy) < 6) return;
      started = true;
      setDrag({ id, x: ev.clientX, y: ev.clientY, w, ...where(ev) });
    };
    const finish = (commit: boolean, ev?: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("keydown", key, true);
      setDrag(null);
      if (!started || !commit || !ev) return;
      const at = where(ev);
      if (at.inside) moveTab(id, at.index);
      else void detachTab(id, ev.screenX, ev.screenY);
    };
    const up = (ev: PointerEvent) => finish(true, ev);
    const key = (ev: KeyboardEvent) => ev.key === "Escape" && finish(false);
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("keydown", key, true);
  };

  const dragged = drag ? tabs.find((t) => t.id === drag.id) : undefined;
  const shadow = (key: string) => <div key={key} className="tab-drop-shadow" aria-hidden="true" style={vertical ? undefined : { width: drag?.w }} />;

  let lastGroup: string | null = null;
  let slot = 0;
  return (
    <div className="title-bar" data-tauri-drag-region data-os={os} data-vertical={vertical || undefined} data-controls={controls || undefined}>
     {controls && os === "macos" && <div className="traffic-light-space" data-tauri-drag-region />}
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
        const isDragged = drag?.id === t.id;
        const before = drag?.inside && !isDragged && slot === drag.index ? shadow(`shadow-${t.id}`) : null;
        if (!isDragged) slot++;
        return (
          <div key={t.id} className="tab-slot" data-dragging={isDragged || undefined} style={group ? { ["--group-color" as string]: group.color } : undefined}>
            {before}
            {chip && (
              <button
                className="tab-group-chip"
                title={group.collapsed ? "Expand group" : "Collapse group"}
                onClick={() => updateGroup(group.id, { collapsed: !group.collapsed })}
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
                aria-selected={t.id === activeTab}
                data-active={t.id === activeTab || undefined}
                data-grouped={group ? true : undefined}
                onPointerDown={(e) => startDrag(e, t.id)}
                onMouseDown={(e) => e.button === 0 && activate(t.id)}
                onAuxClick={(e) => e.button === 1 && closeTab(t.id)}
                onContextMenu={(e) => open(e, tabMenu(t.id, t.groupId))}
                title={isAppLocation(t.location) ? tabTitle(t.location) : t.location}
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
      </div>
      {more.after && (
        <button className="tab-scroll" aria-label="Scroll tabs forward" title="Scroll tabs forward" data-forward onClick={() => scrollBy(1)}>
          <Icon name="chevron" size={11} />
        </button>
      )}
      <button className="tab-new" aria-label="New tab" title="New tab (Ctrl+T)" onClick={() => newTab()}>
        <Icon name="plus" size={14} />
      </button>
      {secondary && (
        <button
          className="tab-new reattach-tabs"
          aria-label="Move back to the main window"
          title="Move these tabs back to the main window, where they were"
          onClick={() => void api.reattach("tabs", { locations: tabs.map((t) => t.location), index: tabOrigin() }).catch((e) => useStore.getState().showToast(String(e)))}
        >
          <Icon name="popin" size={14} />
        </button>
      )}
     </div>
     {!vertical && <div className="title-drag" data-tauri-drag-region />}
     {controls && os !== "macos" && <WindowControls />}
     {drag && dragged && (
       <div className="tab tab-ghost" data-detach={!drag.inside || undefined} style={{ left: drag.x - drag.w / 2, top: drag.y - 17, width: drag.w }}>
         <FileIcon kind={isAppLocation(dragged.location) ? "file" : "folder"} size={14} />
         <span className="tab-title">{tabs.length < 2 && !drag.inside ? "The only tab stays here" : tabTitle(dragged.location)}</span>
         {!drag.inside && tabs.length > 1 && <span className="tab-ghost-hint">New window</span>}
       </div>
     )}
     {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Address row: navigation, breadcrumb, search
// ---------------------------------------------------------------------------

export function Toolbar() {
  const tab = useStore((s) => s.tabs.find((t) => t.id === s.activeTab));
  const info = useStore((s) => s.info);
  const drives = useStore((s) => s.drives);
  const settings = useStore((s) => s.settings)!;
  const { navigate, back, forward, bumpDirs, showToast } = useStore.getState();
  const [editing, setEditing] = useState(false);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  const location = tab?.location ?? HOME;
  const loc = parseLocation(location);
  const folder = loc.kind === "folder" ? loc.path : null;
  const up = folder ? (parentPath(folder) ?? THIS_PC) : null;

  // The search box mirrors the query when a search tab is showing.
  const [query, setQuery] = useState("");
  useEffect(() => setQuery(loc.kind === "search" ? loc.query : ""), [location]); // eslint-disable-line

  // Scope for a new search: the folder being viewed, unless the user prefers everything.
  const scopeRef = useRef<string | null>(null);
  if (folder) scopeRef.current = settings.search.scopeToCurrentFolder ? folder : null;
  else if (loc.kind === "search") scopeRef.current = loc.scope;
  else scopeRef.current = null;

  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const onSearch = (text: string) => {
    setQuery(text);
    clearTimeout(timer.current);
    if (!text.trim()) return;
    // Search as you type; the same tab is reused so history stays clean.
    timer.current = setTimeout(() => {
      navigate(searchLocation(text, scopeRef.current), { replace: loc.kind === "search" });
    }, adv<number>(settings, "search.debounceMs"));
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      if ((mod && e.key.toLowerCase() === "f") || e.key === "F3") {
        e.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
      } else if ((mod && e.key.toLowerCase() === "l") || (e.altKey && e.key.toLowerCase() === "d")) {
        e.preventDefault();
        setEditing(true);
      } else if (e.altKey && e.key === "ArrowLeft") back();
      else if (e.altKey && e.key === "ArrowRight") forward();
      else if (e.altKey && e.key === "ArrowUp" && up) navigate(up);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [up, back, forward, navigate]);

  const terminalDir = folder ?? info?.home ?? "/";
  const openTerminal = (mode: TerminalMode) => api.openTerminal(terminalDir, mode).catch((e) => showToast(String(e)));
  const terminalMenu = (e: React.MouseEvent) => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const admin = info?.os === "windows" ? "as administrator" : "as root";
    setMenu({
      x: r.left,
      y: r.bottom + 4,
      items: [
        { id: "here", label: folder ? "Open in this folder" : "Open terminal", run: () => void openTerminal("here") },
        { id: "home", label: "Open in home folder", run: () => void openTerminal("home") },
        { id: "drive", label: "Open at drive root", disabled: !folder, run: () => void openTerminal("driveRoot") },
        { id: "admin", label: `Open here ${admin}`, separatorBefore: true, run: () => void openTerminal("admin") },
      ],
    });
  };

  /** This PC > Main-Dev-Drive (A:) > Projects: the drive shows its name and letter. */
  const trail: { label: string; path: string; icon?: string }[] = [{ label: "", path: THIS_PC, icon: "pc" }, { label: thisPcLabel(info?.os), path: THIS_PC }];
  if (folder) {
    for (const [i, c] of crumbs(folder).entries()) {
      const drive = i === 0 ? drives.find((d) => d.mount === c.path || d.mount.replace(/[\\/]$/, "") === c.path.replace(/[\\/]$/, "")) : undefined;
      trail.push({ label: drive ? driveName(drive, settings.appearance) : c.label, path: c.path });
    }
  } else if (loc.kind !== "thispc") {
    trail.length = 0;
    trail.push({ label: tabTitle(location), path: location });
  }

  return (
    <div className="toolbar">
      <div className="nav-buttons">
        <button aria-label="Back" title="Back (Alt+Left)" disabled={!tab || tab.historyIndex === 0} onClick={back}>
          <Icon name="back" />
        </button>
        <button aria-label="Forward" title="Forward (Alt+Right)" disabled={!tab || tab.historyIndex >= tab.history.length - 1} onClick={forward}>
          <Icon name="forward" />
        </button>
        <button aria-label="Up" title="Up (Alt+Up)" disabled={!up} onClick={() => up && navigate(up)}>
          <Icon name="up" />
        </button>
        <button aria-label="Refresh" title="Refresh (F5)" onClick={() => (folder ? bumpDirs([folder]) : useStore.getState().refreshDrives())}>
          <Icon name="refresh" />
        </button>
      </div>

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
            defaultValue={folder ?? ""}
            placeholder="Type a path"
            onFocus={(e) => e.target.select()}
            onBlur={() => setEditing(false)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && e.currentTarget.value.trim()) {
                const typed = e.currentTarget.value;
                void api
                  .resolvePath(typed)
                  .then((r) => {
                    if (r.isDir) navigate(r.path);
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
                  .catch((err) => showToast(String(err)));
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

      <div className="search-box">
        <Icon name="search" size={14} />
        <input
          ref={searchRef}
          value={query}
          placeholder={scopeRef.current ? `Search ${tabTitle(scopeRef.current)}` : `Search ${thisPcLabel(info?.os)}`}
          aria-label="Search"
          title="Try: report *.pdf ext:jpg type:folder size:>10mb"
          onChange={(e) => onSearch(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && query.trim()) {
              clearTimeout(timer.current);
              navigate(searchLocation(query, scopeRef.current), { replace: loc.kind === "search" });
            }
            e.stopPropagation();
          }}
        />
      </div>
      <button className="icon-button" aria-label="Open terminal" title="Open terminal" onClick={terminalMenu}>
        <Icon name="terminal" />
      </button>
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

/** Pick a drive letter that is not in use, for the Map network drive dialog. */
function freeLetters(drives: Drive[]): string[] {
  const used = new Set(drives.map((d) => d.mount.slice(0, 1).toUpperCase()));
  return "ZYXWVUTSRQPONMLKJIHGFED".split("").filter((l) => !used.has(l));
}

async function mapNetworkDrive(drives: Drive[], auto: boolean, os: string | undefined) {
  const { showToast, refreshDrives } = useStore.getState();
  const windows = os === "windows";
  const r = await ask({
    title: auto || !windows ? "Add a network location" : "Map network drive",
    message: windows ? "Folder or computer to connect to, like \\\\server\\share" : "Server and share to connect to, like smb://server/share",
    input: windows ? "\\\\" : "smb://",
    select: auto || !windows ? undefined : { label: "Drive letter", options: freeLetters(drives).map((l) => ({ value: `${l}:`, label: `${l}:` })) },
    buttons: [
      { id: "ok", label: "Connect", primary: true },
      { id: "cancel", label: "Cancel" },
    ],
  });
  if (!r || r.button !== "ok" || !r.input.trim()) return;
  try {
    const mount = await api.networkMap(r.input.trim(), auto || !windows ? null : r.selected || null);
    showToast("Connected.");
    refreshDrives();
    if (mount) useStore.getState().navigate(mount);
  } catch (e) {
    showToast(String(e));
  }
}

export function CommandBar({ onFilter, vertical = false }: { onFilter: (anchor: DOMRect) => void; vertical?: boolean }) {
  const os = useStore((s) => s.info?.os);
  const location = useStore((s) => s.tabs.find((t) => t.id === s.activeTab)?.location ?? HOME);
  const selection = useStore((s) => s.selection);
  const clipboard = useStore((s) => s.clipboard);
  const undoCount = useStore((s) => s.undo.length);
  const sort = useStore((s) => s.sort);
  const filter = useStore((s) => s.filter);
  const drives = useStore((s) => s.drives);
  const settings = useStore((s) => s.settings)!;
  const { setClipboard, startTransfer, setSort, updateSettings, setViewLevel, showToast, refreshDrives } = useStore.getState();
  const [menu, setMenu] = useState<MenuState | null>(null);

  const loc = parseLocation(location);
  const folder = loc.kind === "folder" ? loc.path : null;
  const inFolder = !!folder;
  const hasSel = selection.length > 0;
  const level = settings.appearance.viewLevel;

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
      { id: "folder", label: "Folder", icon: "folderOpen", shortcut: "Ctrl+Shift+N", run: () => void create(true, "New folder") },
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
      {
        id: "details-pane",
        label: "Details pane",
        separatorBefore: true,
        icon: "details",
        checked: settings.appearance.showDetailsPane,
        run: () => updateSettings((s) => void (s.appearance.showDetailsPane = !s.appearance.showDetailsPane)),
      },
      { id: "hidden", label: "Hidden items", checked: settings.general.showHidden, run: () => updateSettings((s) => void (s.general.showHidden = !s.general.showHidden)) },
      { id: "ext", label: "File name extensions", checked: settings.general.showExtensions, run: () => updateSettings((s) => void (s.general.showExtensions = !s.general.showExtensions)) },
      { id: "sidebar", label: "Navigation pane", checked: !settings.appearance.sidebarCollapsed, run: () => updateSettings((s) => void (s.appearance.sidebarCollapsed = !s.appearance.sidebarCollapsed)) },
    ]);

  const share = (e: React.MouseEvent) =>
    below(e, [
      { id: "copyPath", label: "Copy as path", icon: "copy", run: () => void navigator.clipboard.writeText(selection.join("\n")).then(() => showToast("Path copied.")) },
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
    const network = drives.filter((d) => d.media === "network");
    below(e, [
      { id: "undo", label: "Undo", icon: "undo", shortcut: "Ctrl+Z", disabled: undoCount === 0, run: () => void undoLast() },
      ...(onThisPc
        ? [
            { id: "addNetwork", label: "Add a network location", icon: "network", separatorBefore: true, run: () => void mapNetworkDrive(drives, true, os) },
            ...(os === "windows" ? [{ id: "mapDrive", label: "Map network drive", icon: "network", run: () => void mapNetworkDrive(drives, false, os) }] : []),
            {
              id: "disconnect",
              label: os === "windows" ? "Disconnect network drive" : "Disconnect network location",
              icon: "network",
              disabled: network.length === 0,
              children: network.map((d) => ({
                id: `disc-${d.mount}`,
                label: driveName(d, settings.appearance),
                run: () =>
                  void api
                    .networkDisconnect(d.mount)
                    .then(() => (showToast("Disconnected."), refreshDrives()))
                    .catch((err) => showToast(String(err))),
              })),
            },
          ]
        : []),
      { id: "selectAll", label: "Select all", icon: "check", shortcut: "Ctrl+A", separatorBefore: true, disabled: !inFolder, run: () => useStore.getState().runCommand("selectAll") },
      { id: "selectNone", label: "Select none", disabled: !inFolder, run: () => useStore.getState().runCommand("selectNone") },
      { id: "invert", label: "Invert selection", disabled: !inFolder, run: () => useStore.getState().runCommand("invert") },
      { id: "properties", label: "Properties", icon: "info", separatorBefore: true, disabled: !hasSel && !folder, run: () => void showProperties(hasSel ? selection : folder ? [folder] : []) },
      { id: "options", label: "Options", icon: "settings", run: () => useStore.getState().openSettings() },
    ]);
  };

  const button = (id: string, label: string, icon: string, run: (e: React.MouseEvent) => void, opts?: { disabled?: boolean; caret?: boolean; on?: boolean; title?: string }) => (
    <button key={id} className="cmd" data-cmd={id} data-on={opts?.on || undefined} disabled={opts?.disabled} title={opts?.title ?? label} onClick={run}>
      <Icon name={icon} size={16} />
      <span>{label}</span>
      {opts?.caret && <Icon name="expand" size={9} />}
    </button>
  );

  return (
    <div className="command-bar" role="toolbar" aria-label="Actions" data-vertical={vertical || undefined}>
      {button("new", "New", "newItem", newMenu, { disabled: !inFolder, caret: true })}
      <span className="cmd-sep" />
      {button("cut", "Cut", "cut", () => setClipboard({ paths: selection, op: "move" }), { disabled: !hasSel, title: "Cut (Ctrl+X)" })}
      {button("copy", "Copy", "copy", () => setClipboard({ paths: selection, op: "copy" }), { disabled: !hasSel, title: "Copy (Ctrl+C)" })}
      {button("paste", "Paste", "paste", () => clipboard && folder && void startTransfer(clipboard.paths, folder, clipboard.op).then(() => clipboard.op === "move" && setClipboard(null)), { disabled: !clipboard || !inFolder, title: "Paste (Ctrl+V)" })}
      {button("rename", "Rename", "rename", () => useStore.getState().runCommand("rename"), { disabled: selection.length !== 1, title: "Rename (F2)" })}
      {button("share", "Share", "share", share, { disabled: !hasSel, caret: true })}
      {button("delete", "Delete", "trash", () => void deletePaths(selection), { disabled: !hasSel, title: "Delete (Del)" })}
      <span className="cmd-sep" />
      {button("sort", "Sort", "sort", sortMenu, { disabled: !inFolder, caret: true })}
      {button("view", "View", "view", viewMenu, { caret: true })}
      {button("filter", isFiltering(filter) ? "Filter •" : "Filter", "filter", (e) => onFilter((e.currentTarget as HTMLElement).getBoundingClientRect()), { disabled: !inFolder, caret: true, on: isFiltering(filter) })}
      <span className="cmd-sep" />
      {button("more", "", "more", more, { title: "See more" })}
      <span className="cmd-grow" />
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

const COLLAPSED_WIDTH = 52;
const MIN_WIDTH = 150;
const MAX_WIDTH = 480;

export function Sidebar() {
  const pins = useStore((s) => s.pins);
  const drives = useStore((s) => s.drives);
  const frequent = useStore((s) => s.frequent);
  const info = useStore((s) => s.info);
  const appearance = useStore((s) => s.settings!.appearance);
  const location = useStore((s) => s.tabs.find((t) => t.id === s.activeTab)?.location);
  const { navigate, newTab, togglePin, addRecent, showToast, updateSettings } = useStore.getState();
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [dragWidth, setDragWidth] = useState<number | null>(null);

  const collapsed = appearance.sidebarCollapsed;
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
    extra?: { isFile?: boolean; menu?: MenuItem[]; sub?: string; tip?: string },
  ) => (
    <button
      key={key}
      className="side-item"
      data-active={location === target || undefined}
      title={extra?.tip ?? (isAppLocation(target) ? label : `${label}\n${target}`)}
      onClick={() => {
        if (extra?.isFile) {
          addRecent(target, false);
          api.openItem(target).catch((e) => showToast(String(e)));
        } else navigate(target);
      }}
      onAuxClick={(e) => e.button === 1 && !extra?.isFile && newTab(target, { background: true })}
      onContextMenu={(e) => {
        e.preventDefault();
        const items: MenuItem[] = [
          ...(extra?.isFile ? [] : [{ id: "newTab", label: "Open in new tab", run: () => newTab(target) }]),
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
        { id: "hide", label: "Hide this section", separatorBefore: true, run: () => updateSettings((st) => void (st.appearance[flag] = false)) },
      ],
    });
  };
  const heading = (text: string, rowsKey?: string, flag?: "showFrequent" | "showPinned" | "showDrives") =>
    collapsed ? (
      <div className="side-divider" key={text} />
    ) : (
      <div className="side-heading" key={text} onContextMenu={rowsKey && flag ? (e) => sectionMenu(e, rowsKey, flag) : undefined} title={rowsKey ? "Right-click for options" : undefined}>
        {text}
      </div>
    );
  /** A section's rows, with their own scroll bar past the chosen number. */
  const section = (rowsKey: string, rows: React.ReactNode) => {
    const n = adv<number>(useStore.getState().settings, rowsKey);
    return (
      <div className="side-section" data-scroll={n > 0 || undefined} style={n > 0 ? { maxHeight: `calc(${n} * var(--side-row, 30px))` } : undefined}>
        {rows}
      </div>
    );
  };
  const driveIcon = (d: Drive) => <Icon name={d.media === "network" ? "network" : "drive"} />;
  const pinnedPaths = new Set(pins.map((p) => p.path));
  const frequentShown = frequent.filter((p) => !pinnedPaths.has(p)).slice(0, adv<number>(useStore.getState().settings, "files.frequentCount"));

  return (
    <div className="sidebar-wrap" style={{ width }}>
    <nav className="sidebar" aria-label="Places" data-collapsed={collapsed || undefined}>
      <button
        className="side-item side-toggle"
        title={collapsed ? "Expand the navigation pane" : "Collapse to icons"}
        aria-label={collapsed ? "Expand the navigation pane" : "Collapse the navigation pane"}
        onClick={() => updateSettings((s) => void (s.appearance.sidebarCollapsed = !s.appearance.sidebarCollapsed))}
      >
        <Icon name="sidebar" />
        {!collapsed && <span className="side-label">Navigation</span>}
      </button>
      {item("home", "Home", HOME, <Icon name="home" />)}
      {item("thispc", thisPcLabel(info?.os), THIS_PC, <Icon name="pc" />)}
      {info?.home && item("analyze", "Disk usage", analyzeLocation(info.home), <Icon name="chart" />, { tip: "Disk usage" })}

      {appearance.showFrequent && frequentShown.length > 0 && (
        <>
          {heading("Frequently used", "sidebar.frequentRows", "showFrequent")}
          {section(
            "sidebar.frequentRows",
            frequentShown.map((p) =>
              item(`freq-${p}`, baseName(p), p, <FileIcon kind="folder" />, {
                menu: [{ id: "pin", label: "Pin to sidebar", run: () => togglePin(p) }],
              }),
            ),
          )}
        </>
      )}

      {appearance.showPinned && (
        <>
          {heading("Pinned", "sidebar.pinnedRows", "showPinned")}
          {pins.length === 0 && !collapsed && <div className="side-empty">Right-click a folder to pin it.</div>}
          {section(
            "sidebar.pinnedRows",
            pins.map((p) =>
              item(`pin-${p.path}`, p.label, p.path, <FileIcon kind={p.isFile ? "file" : "folder"} name={p.isFile ? p.label : undefined} />, {
                isFile: p.isFile,
                menu: [{ id: "unpin", label: "Unpin", run: () => togglePin(p.path) }],
              }),
            ),
          )}
        </>
      )}

      {appearance.showDrives && (
        <>
          {heading("Drives", "sidebar.drivesRows", "showDrives")}
          {section(
            "sidebar.drivesRows",
            drives.map((d) =>
              item(`drive-${d.mount}`, driveName(d, appearance), d.mount, driveIcon(d), {
                sub: d.online ? undefined : "offline",
                tip: driveRemote(d, appearance) ? `${driveName(d, appearance)}\n${d.remote}` : undefined,
                menu: [{ id: "analyze", label: "Analyze disk usage", run: () => navigate(analyzeLocation(d.mount)) }],
              }),
            ),
          )}
        </>
      )}
      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
    </nav>
    <div
      className="sidebar-resizer"
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize navigation pane"
      onPointerDown={startResize}
      onDoubleClick={() => updateSettings((s) => void (s.appearance.sidebarCollapsed = !s.appearance.sidebarCollapsed))}
    />
    </div>
  );
}
