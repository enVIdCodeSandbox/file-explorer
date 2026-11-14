// The folder view: a virtualized details list or thumbnail grid. Only the
// rows on screen (plus a small overscan) exist in the DOM, so a folder with
// 100,000 files scrolls as smoothly as one with ten.

import { viewLevelOf } from "../util/viewLevel";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, on, type FileEntry, type FolderSize, type Listing } from "../api";
import {
  ICON_PX,
  VIEW_LEVELS,
  createPath,
  compressPaths,
  deletePaths,
  extractArchive,
  isArchiveName,
  openFile,
  promptText,
  renamePath,
  showProperties,
  useStore,
  type SortKey,
} from "../store";
import { applyFilter, facets as facetsOf, isFiltering, sortEntries } from "../util/filter";
import { formatDate, formatSize, kindOf, typeLabel } from "../util/format";
import { adv } from "../settings/advanced";
import { LockedCard } from "./Locks";
import { commandFor } from "../settings/keys";
import { beginDrag, type DragPayload } from "../dnd";
import { analyzeLocation, archiveUrl, insideArchive, isArchiveFile, isArchiveLoc, isRemote, joinPath, parentPath, remoteLabel, splitArchive } from "../util/path";
import { ContextMenu, type MenuItem, type MenuState } from "./ContextMenu";
import { Thumb } from "./Thumb";
import { useShellItems } from "./shellMenu";

/** Pixel widths of the Details columns. The list is only as wide as these,
 *  so there is always empty space on the right to right-click. */
const DEFAULT_COLUMNS = { name: 420, modified: 170, type: 140, size: 100 };
type Columns = typeof DEFAULT_COLUMNS;
const MIN_COLUMN = { name: 140, modified: 110, type: 80, size: 70 };

function loadColumns(): Columns {
  try {
    const saved = JSON.parse(localStorage.getItem("fast.columns") ?? "null");
    if (saved && typeof saved === "object") return { ...DEFAULT_COLUMNS, ...saved };
  } catch {
    /* storage unavailable: use the defaults */
  }
  return DEFAULT_COLUMNS;
}

/** How each icon level lays out its items. */
function layoutFor(level: string, thumbPx: number) {
  if (level === "list") return { mode: "inline" as const, w: 230, h: 24, icon: 16 };
  if (level === "small") return { mode: "inline" as const, w: 270, h: 44, icon: 32 };
  return { mode: "tile" as const, w: thumbPx + 28, h: thumbPx + 50, icon: thumbPx };
}

/** Folder sizes seen this session, by folder; the oldest folders are dropped past a limit. */
const sizesThisSession = new Map<string, Record<string, FolderSize>>();
const SESSION_FOLDERS = 300;
function rememberSize(m: FolderSize & { dir: string }) {
  const have = sizesThisSession.get(m.dir) ?? {};
  have[m.name] = m;
  sizesThisSession.delete(m.dir); // re-insert: most recently used last
  sizesThisSession.set(m.dir, have);
  if (sizesThisSession.size > SESSION_FOLDERS) sizesThisSession.delete(sizesThisSession.keys().next().value as string);
}

export function FileView({ path }: { path: string }) {
  const settings = useStore((s) => s.settings)!;
  const version = useStore((s) => s.dirVersion[path] ?? 0);
  const clipboard = useStore((s) => s.clipboard);
  const pins = useStore((s) => s.pins);
  const drives = useStore((s) => s.drives);
  const places = useStore((s) => s.places);
  const sort = useStore((s) => s.sort);
  const filter = useStore((s) => s.filter);
  const command = useStore((s) => s.command);
  const pendingSelect = useStore((s) => s.pendingSelect);
  const { navigate, newTab, setSelection, setClipboard, startTransfer, pasteInto, togglePin, addRecent, showToast, setSort, setViewLevel } =
    useStore.getState();

  const folderView = useStore((s) => s.folderView);
  const level = VIEW_LEVELS[Math.max(0, Math.min(VIEW_LEVELS.length - 1, viewLevelOf(settings, folderView)))];
  const thumbPx = ICON_PX[level] ?? settings.performance.thumbnailSize;
  const tileLayout = layoutFor(level, thumbPx);
  const details = level === "details";

  const [listing, setListing] = useState<Listing | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [renaming, setRenaming] = useState<string | null>(null);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [width, setWidth] = useState(800);
  const [columns, setColumns] = useState<Columns>(loadColumns);
  const anchor = useRef(0);
  const cursor = useRef(0);
  const scroller = useRef<HTMLDivElement>(null);
  const typeahead = useRef({ text: "", at: 0 });
  const menuTicket = useRef(0);
  const { fetchFor } = useShellItems();

  // ---- Loading ------------------------------------------------------------
  useEffect(() => {
    let alive = true;
    setError(null);
    api
      .listDir(path)
      .then((l) => alive && setListing(l))
      .catch((e) => alive && setError(String(e)));
    return () => {
      alive = false;
    };
  }, [path, version, settings.general.showHidden]);

  useEffect(() => {
    setSelected(new Set());
    setRenaming(null);
    scroller.current?.scrollTo({ top: 0 });
    scroller.current?.focus();
  }, [path]);

  const current = listing?.path === path ? listing : null;

  // ---- Folder sizes: measured in the background, shown as they arrive -----
  const wantSizes = adv<boolean>(settings, "files.folderSizes");
  const [folderSizes, setFolderSizes] = useState<Record<string, FolderSize>>({});
  const sizedPath = useRef("");
  // Sizes seen this session, so a folder revisited shows them before anything is asked.
  const remember = adv<boolean>(settings, "files.sizeCache");
  useEffect(() => {
    if (!current || !wantSizes || isRemote(current.path) || isArchiveLoc(current.path)) return;
    const same = sizedPath.current === current.path;
    sizedPath.current = current.path;
    if (!same) setFolderSizes(remember ? { ...(sizesThisSession.get(current.path) ?? {}) } : {});
    const names = current.entries.filter((e) => e.isDir && !e.isSymlink).map((e) => e.name);
    if (names.length) {
      const policy = {
        remember: adv<boolean>(settings, "files.sizeCache"),
        freshSecs: adv<number>(settings, "files.sizeCacheFreshMin") * 60,
        maxEntries: adv<number>(settings, "files.sizeCacheMax"),
      };
      void api.folderSizesStart(current.path, names, same, policy).catch(() => {});
    }
  }, [current, wantSizes]);
  useEffect(() => {
    if (!wantSizes) return;
    const off = on("folder-size", (m) => {
      if (m.dir !== path) return;
      if (remember) rememberSize(m);
      setFolderSizes((prev) => ({ ...prev, [m.name]: m }));
    });
    return () => void off.then((f) => f());
  }, [path, wantSizes, remember]);

  const sized = useMemo(
    () =>
      current && wantSizes
        ? current.entries.map((e) => (e.isDir && folderSizes[e.name] ? { ...e, size: folderSizes[e.name].bytes } : e))
        : (current?.entries ?? []),
    [current, folderSizes, wantSizes],
  );
  const sorted = useMemo(
    () => (current ? sortEntries(sized, sort.key, sort.asc, settings.general.foldersFirst) : []),
    [current, sized, sort, settings.general.foldersFirst],
  );
  const entries = useMemo(() => applyFilter(sorted, filter), [sorted, filter]);

  // The filter menu offers what is actually in this folder.
  useEffect(() => {
    if (current) useStore.setState({ facets: facetsOf(current.entries) });
  }, [current]);

  useEffect(() => {
    setSelection([...selected].map((n) => joinPath(path, n)));
  }, [selected, path, setSelection]);

  // Items that were moved, deleted or renamed away are no longer selected.
  useEffect(() => {
    if (!current) return;
    const names = new Set(current.entries.map((e) => e.name));
    setSelected((prev) => {
      const next = new Set([...prev].filter((n) => names.has(n)));
      return next.size === prev.size ? prev : next;
    });
  }, [current]);

  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setWidth(el.clientWidth));
    ro.observe(el);
    setWidth(el.clientWidth);
    return () => ro.disconnect();
  }, []);

  /** One item is renamed in place; several open the Rename dialog with a preview. */
  const startRename = () => {
    const chosen = entries.filter((e) => selected.has(e.name));
    if (chosen.length === 1) setRenaming(chosen[0].name);
    else if (chosen.length > 1)
      useStore.setState({
        batchRename: {
          dir: path,
          items: chosen.map((e) => ({ name: e.name, isDir: e.isDir, modified: e.modified ?? 0 })),
          siblings: (current?.entries ?? []).map((e) => e.name),
        },
      });
  };

  // Commands from the command bar (Select all, Rename…) and a name to select
  // once it exists (after creating something).
  useEffect(() => {
    if (!command) return;
    if (command.name === "selectAll") setSelected(new Set(entries.map((e) => e.name)));
    else if (command.name === "selectNone") setSelected(new Set());
    else if (command.name === "invert") setSelected(new Set(entries.filter((e) => !selected.has(e.name)).map((e) => e.name)));
    else if (command.name === "rename") startRename();
  }, [command?.tick]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!pendingSelect || !current) return;
    const name = pendingSelect.slice(path.length).replace(/^[\\/]/, "");
    if (current.entries.some((e) => e.name === name)) {
      setSelected(new Set([name]));
      useStore.setState({ pendingSelect: null });
    }
  }, [pendingSelect, current, path]);

  // ---- Ctrl + scroll changes the view, like Windows 11 -----------------------
  const wheelDelta = useRef(0);
  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey || !adv<boolean>(useStore.getState().settings, "files.wheelZoom")) return;
      e.preventDefault();
      wheelDelta.current += e.deltaY;
      // One step per notch; trackpads send many small deltas, so accumulate.
      while (Math.abs(wheelDelta.current) >= 60) {
        const dir = wheelDelta.current < 0 ? 1 : -1; // scroll up = bigger icons
        wheelDelta.current += dir * 60;
        const st = useStore.getState();
        const lvl = viewLevelOf(st.settings, st.folderView);
        setViewLevel(lvl + dir);
      }
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [setViewLevel]);

  // ---- Virtualization -----------------------------------------------------
  const compact = settings.appearance.density === "compact";
  const columnsPerRow = details ? 1 : Math.max(1, Math.floor((width - 16) / tileLayout.w));
  const rowCount = Math.ceil(entries.length / columnsPerRow);
  const customRow = adv<number>(settings, "ui.rowHeight");
  const rowHeight = details ? (customRow > 0 ? customRow : compact ? 26 : 34) : tileLayout.h;
  const headerHeight = details ? 28 : 0;
  // Name keeps its own width and the list does not stretch to the pane's edge (the free
  // space on the right stays free). In a pane too narrow for that, Name gives way first,
  // so there is no sideways scrollbar unless the other columns alone are wider than the pane.
  const cols = { ...columns, name: Math.max(MIN_COLUMN.name, Math.min(columns.name, width - 14 - columns.modified - columns.type - columns.size)) };
  const rowsWidth = cols.name + cols.modified + cols.type + cols.size;
  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => scroller.current,
    estimateSize: () => rowHeight,
    // The sticky column header sits inside the scroller, above the rows.
    scrollMargin: headerHeight,
    paddingStart: details ? 0 : 8,
    // With lazy loading off, render further ahead at the cost of more DOM.
    overscan: settings.performance.lazyLoading ? (details ? adv<number>(settings, "perf.overscanRows") : 2) : 40,
  });
  useEffect(() => virtualizer.measure(), [rowHeight, virtualizer]);

  // ---- Actions ------------------------------------------------------------
  const fullPath = useCallback((name: string) => joinPath(path, name), [path]);
  const selectedPaths = () => entries.filter((e) => selected.has(e.name)).map((e) => fullPath(e.name));

  const open = (e: FileEntry) => {
    if (e.isDir) {
      if (adv<boolean>(settings, "files.foldersInNewTab")) newTab(fullPath(e.name));
      else navigate(fullPath(e.name));
    } else {
      const target = fullPath(e.name);
      if (isArchiveLoc(target)) {
        // A file inside an archive is unpacked to a temporary place and opened from there.
        api.archiveCopyStart([target], "", true).catch((err) => showToast(String(err)));
        return;
      }
      if (isArchiveFile(e.name) && adv<boolean>(settings, "files.openArchives") && !isRemote(target)) {
        // Archives open like folders; "Open with…" still hands them to another program.
        if (adv<boolean>(settings, "files.foldersInNewTab")) newTab(archiveUrl(target));
        else navigate(archiveUrl(target));
        return;
      }
      if (isRemote(target)) {
        // A file on a server is fetched to a temporary place and opened from there.
        api.remoteCopyStart([target], "", false, true).catch((err) => showToast(String(err)));
        return;
      }
      addRecent(target, false);
      void openFile(target);
    }
  };

  const paste = () => void pasteInto(path);

  const create = async (folder: boolean) => {
    const name = await promptText(folder ? "New folder" : "New file", folder ? "New folder" : "New file.txt");
    if (!name) return;
    const made = await createPath(path, name, folder);
    if (made) useStore.setState({ pendingSelect: made });
  };

  const commitRename = async (oldName: string, newName: string) => {
    setRenaming(null);
    if (!newName || newName === oldName) return;
    const made = await renamePath(fullPath(oldName), newName);
    if (made) setSelected(new Set([newName]));
  };

  const terminal = (dir: string, mode: "here" | "admin") => api.openTerminal(dir, mode).catch((e) => showToast(String(e)));

  const refresh = () => useStore.getState().bumpDirs([path]);

  /** Right-clicking an unselected item selects just that item first. */
  const selectedPathsWith = (target: FileEntry): string[] => {
    if (selected.has(target.name)) return selectedPaths();
    setSelected(new Set([target.name]));
    return [fullPath(target.name)];
  };

  const sendToItems = (paths: string[]): MenuItem[] => {
    const here = new Set(paths.map((p) => parentPath(p)));
    const destinations = [
      ...places.filter((p) => ["Desktop", "Documents", "Downloads"].includes(p.label)).map((p) => ({ label: p.label, path: p.path, icon: "folderOpen" })),
      ...drives
        .filter((d) => d.online && !d.readOnly && d.mount !== "/")
        .map((d) => ({ label: d.name, path: d.mount, icon: d.media === "network" ? "network" : "drive" })),
    ].filter((d) => !here.has(d.path));
    return destinations.map((d) => ({
      id: `sendto-${d.path}`,
      label: d.label,
      icon: d.icon,
      run: () => void startTransfer(paths, d.path, "copy"),
    }));
  };

  const buildMenu = (target: FileEntry | null, shell: MenuItem[] = []): MenuItem[] => {
    const paths = target ? selectedPathsWith(target) : [];
    const single = target && paths.length === 1 ? target : null;
    const dirForTerminal = single?.isDir ? fullPath(single.name) : path;
    const pinned = single ? pins.some((p) => p.path === fullPath(single.name)) : pins.some((p) => p.path === path);
    const sendTo = sendToItems(paths);
    const picked = entries.filter((e) => paths.includes(fullPath(e.name)));
    const custom: MenuItem[] = settings.customActions
      .filter((a) => {
        if (!picked.length) return false;
        if (a.appliesTo === "file" && picked.some((e) => e.isDir)) return false;
        if (a.appliesTo === "folder" && picked.some((e) => !e.isDir)) return false;
        const exts = a.extensions.split(",").map((x) => x.trim().toLowerCase().replace(/^\./, "")).filter(Boolean);
        return !exts.length || picked.every((e) => !e.isDir && exts.includes(e.name.split(".").pop()?.toLowerCase() ?? ""));
      })
      .map((a) => ({ id: `custom-${a.id}`, label: a.label, icon: "newItem", run: () => void api.runCustomAction(a.id, paths).catch((err) => showToast(String(err))) }));
    const items: (MenuItem | false | null | undefined)[] = target
      ? [
          single && { id: "open", label: "Open", shortcut: "Enter", run: () => open(single) },
          single?.isDir && { id: "openNewTab", label: "Open in new tab", run: () => newTab(fullPath(single.name), { background: true }) },
          single && !single.isDir && !isRemote(fullPath(single.name)) && !isArchiveLoc(fullPath(single.name)) && { id: "openWith", label: "Open with…", run: () => useStore.getState().chooseApp(fullPath(single.name)) },
          ...shell.map((m, i) => (i === 0 ? { ...m, separatorBefore: true } : m)),
          ...custom.map((m, i) => (i === 0 ? { ...m, separatorBefore: true } : m)),
          single && {
            id: "pin",
            label: pinned ? "Unpin from sidebar" : "Pin to sidebar",
            icon: "pin",
            separatorBefore: shell.length === 0 && custom.length === 0,
            run: () => togglePin(fullPath(single.name), !single.isDir),
          },
          { id: "copyPath", label: "Copy as path", icon: "copy", run: () => void navigator.clipboard.writeText(paths.join("\n")) },
          {
            id: "share",
            label: "Share",
            icon: "share",
            children: [
              { id: "share-link", label: "Temporary download link…", icon: "share", run: () => useStore.setState({ shareFor: paths }) },
              { id: "share-path", separatorBefore: true, label: "Copy as path", run: () => void navigator.clipboard.writeText(paths.join("\n")) },
              { id: "share-name", label: "Copy name", run: () => void navigator.clipboard.writeText(paths.map((p) => p.split(/[\\/]/).pop()).join("\n")) },
            ],
          },
          sendTo.length > 0 && { id: "sendTo", label: "Send to", icon: "forward", children: sendTo },
          { id: "archive-zip", label: "Compress to ZIP", icon: "archive", run: () => void compressPaths(paths) },
          single && !single.isDir && !isArchiveLoc(path) && isArchiveName(single.name) && { id: "archive-here", label: "Extract here", icon: "archive", run: () => void extractArchive(fullPath(single.name), true) },
          single && !single.isDir && !isArchiveLoc(path) && isArchiveName(single.name) && { id: "archive-to", label: `Extract to "${single.name.replace(/(\.tar)?\.[^.]+$/i, "")}"`, icon: "archive", run: () => void extractArchive(fullPath(single.name), false) },
          { id: "cut", label: "Cut", shortcut: "Ctrl+X", icon: "cut", separatorBefore: true, run: () => setClipboard({ paths, op: "move" }) },
          { id: "copy", label: "Copy", shortcut: "Ctrl+C", icon: "copy", run: () => setClipboard({ paths, op: "copy" }) },
          single?.isDir && {
            id: "paste",
            label: "Paste into folder",
            icon: "paste",
            run: () => void pasteInto(fullPath(single.name)),
          },
          single && {
            id: "createShortcut",
            label: "Create shortcut",
            separatorBefore: true,
            run: () => void api.createShortcut(fullPath(single.name)).then(() => showToast("Shortcut created.")).catch((e) => showToast(String(e))),
          },
          { id: "delete", label: "Delete", shortcut: "Del", icon: "trash", run: () => void deletePaths(paths) },
          { id: "deletePermanent", label: "Delete permanently", shortcut: "Shift+Del", danger: true, run: () => void deletePaths(paths, true) },
          paths.length > 0 && { id: "rename", label: paths.length > 1 ? `Rename ${paths.length} items…` : "Rename", shortcut: "F2", icon: "rename", run: startRename },
          single?.isDir && { id: "terminal", label: "Open terminal here", icon: "terminal", separatorBefore: true, run: () => void terminal(dirForTerminal, "here") },
          single?.isDir && { id: "terminalAdmin", label: "Open terminal as administrator", run: () => void terminal(dirForTerminal, "admin") },
          single?.isDir && { id: "analyze", label: "Analyze disk usage", icon: "chart", run: () => navigate(analyzeLocation(fullPath(single.name))) },
          { id: "properties", label: "Properties", icon: "info", separatorBefore: true, run: () => void showProperties(paths) },
          // Last, and apart from everything people use all the time.
          single?.isDir && !isRemote(fullPath(single.name)) && { id: "lock", label: "Lock with passcode…", icon: "lock", separatorBefore: true, run: () => useStore.getState().openLockDialog(fullPath(single.name)) },
        ]
      : [
          ...(splitArchive(path)
            ? [
                { id: "extractAll-to", label: "Extract all to a folder next to the archive", icon: "archive", run: () => void extractArchive(splitArchive(path)!.archive, false) } as MenuItem,
                { id: "extractAll-here", label: "Extract all here", icon: "archive", run: () => void extractArchive(splitArchive(path)!.archive, true) } as MenuItem,
              ]
            : []),
          { id: "paste", label: "Paste", shortcut: "Ctrl+V", icon: "paste", separatorBefore: !!splitArchive(path), run: paste },
          { id: "newFolder", label: "New folder", shortcut: "Ctrl+Shift+N", icon: "folderOpen", separatorBefore: true, run: () => void create(true) },
          { id: "newFile", label: "New file", icon: "newItem", run: () => void create(false) },
          {
            id: "view",
            label: "View",
            separatorBefore: true,
            children: [...VIEW_LEVELS].reverse().map((v) => ({
              id: `view-${v}`,
              label: { xl: "Extra large icons", large: "Large icons", medium: "Medium icons", small: "Small icons", list: "List", details: "Details" }[v],
              checked: v === level,
              run: () => setViewLevel(VIEW_LEVELS.indexOf(v)),
            })),
          },
          {
            id: "sort",
            label: "Sort by",
            children: (["name", "modified", "created", "type", "extension", "size"] as SortKey[]).map((k) => ({
              id: `sort-${k}`,
              label: { name: "Name", modified: "Date modified", created: "Date created", type: "Type", extension: "Extension", size: "Size" }[k],
              checked: sort.key === k,
              run: () => setSort({ key: k, asc: sort.key === k ? !sort.asc : true }),
            })),
          },
          { id: "refresh", label: "Refresh", shortcut: "F5", icon: "refresh", separatorBefore: true, run: refresh },
          { id: "copyPath", label: "Copy folder path", icon: "copy", run: () => void navigator.clipboard.writeText(path) },
          { id: "pin", label: pinned ? "Unpin this folder" : "Pin this folder", icon: "pin", run: () => togglePin(path) },
          { id: "terminal", label: "Open terminal here", icon: "terminal", separatorBefore: true, run: () => void terminal(path, "here") },
          { id: "terminalAdmin", label: "Open terminal as administrator", run: () => void terminal(path, "admin") },
          { id: "analyze", label: "Analyze disk usage", icon: "chart", run: () => navigate(analyzeLocation(path)) },
          ...shell.map((m, i) => (i === 0 ? { ...m, separatorBefore: true } : m)),
          { id: "properties", label: "Properties", icon: "info", separatorBefore: true, run: () => void showProperties([path]) },
          { id: "lock", label: "Lock this folder with passcode…", icon: "lock", separatorBefore: true, run: () => useStore.getState().openLockDialog(path) },
        ];
    const hidden = new Set(settings.contextMenu.hidden);
    // Things that only make sense for files on this computer.
    const localOnly = new Set(["pin", "share", "sendTo", "archive", "createShortcut", "deletePermanent", "terminal", "terminalAdmin", "analyze", "properties", "newFile", "custom", "lock"]);
    const inArchive = !!insideArchive(path);
    const remote = isRemote(path) || inArchive;
    return items.filter((i): i is MenuItem => !!i && !hidden.has(i.id) && !hidden.has(i.id.replace(/-.*/, "")) && !(remote && localOnly.has(i.id.replace(/-.*/, "")) && !(inArchive && i.id === "newFile")));
  };

  /**
   * Show the menu at once with the built-in entries, then add the ones from
   * the operating system (Edit with…, Open with, archivers…) when they arrive.
   * The menu never waits for them.
   */
  const showMenu = (x: number, y: number, target: FileEntry | null) => {
    const ticket = ++menuTicket.current;
    const first = buildMenu(target);
    setMenu({ x, y, items: first });
    if (!settings.general.shellExtras || isRemote(path) || isArchiveLoc(path)) return;
    const paths = target ? selectedPathsWith(target) : [path];
    void fetchFor(paths, !target).then((shell) => {
      if (ticket !== menuTicket.current || shell.length === 0) return;
      setMenu({ x, y, items: buildMenu(target, shell) });
    });
  };

  // ---- Selection ----------------------------------------------------------
  // Selection works like a spreadsheet: Ctrl+click starts a new range at that
  // item and keeps everything chosen before, and Shift+click reshapes only the
  // range that began at the last click, so several runs can be built up.
  const base = useRef<Set<string>>(new Set()); // chosen before the current range began
  const lastSet = useRef<Set<string> | null>(null); // the selection this logic last produced
  const commit = (next: Set<string>) => {
    lastSet.current = next;
    setSelected(next);
  };
  /** Set when a plain press landed on an item inside a multi-selection; see itemProps. */
  const collapseOnClick = useRef<number | null>(null);
  const selectIndex = (i: number, e: { ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }) => {
    const entry = entries[i];
    if (!entry) return;
    cursor.current = i;
    const ctrl = e.ctrlKey || e.metaKey;
    // If the selection was changed some other way (select all, a rubber band), what is selected now is the base.
    if (selected !== lastSet.current) base.current = new Set(selected);
    if (e.shiftKey) {
      const [lo, hi] = [Math.min(anchor.current, i), Math.max(anchor.current, i)];
      const next = new Set(base.current);
      for (const x of entries.slice(lo, hi + 1)) next.add(x.name);
      if (ctrl) base.current = new Set(next);
      commit(next);
    } else if (ctrl) {
      const next = new Set(selected);
      next.has(entry.name) ? next.delete(entry.name) : next.add(entry.name);
      base.current = new Set(next);
      anchor.current = i;
      commit(next);
    } else {
      base.current = new Set();
      anchor.current = i;
      commit(new Set([entry.name]));
    }
  };

  const moveCursor = (delta: number, shift: boolean) => {
    const i = Math.max(0, Math.min(entries.length - 1, cursor.current + delta));
    selectIndex(i, { ctrlKey: false, metaKey: false, shiftKey: shift });
    virtualizer.scrollToIndex(Math.floor(i / columnsPerRow));
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (renaming || (e.target as HTMLElement).tagName === "INPUT") return;
    const mod = e.ctrlKey || e.metaKey;
    const paths = selectedPaths();
    const first = entries.find((x) => selected.has(x.name));
    let handled = true;
    // Shortcuts from Settings (the defaults are the keys below, so nothing changes unless they were).
    const cmd = commandFor(settings, e.nativeEvent);
    if (cmd === "open" && first) open(first);
    else if (cmd === "rename" && first) startRename();
    else if (cmd === "delete" && paths.length) void deletePaths(paths, e.shiftKey);
    else if (cmd === "newFolder") void create(true);
    else if (cmd === "properties" && paths.length) void showProperties(paths);
    else if (cmd === "refresh") refresh();
    else if (e.key === "ArrowDown") moveCursor(columnsPerRow, e.shiftKey);
    else if (e.key === "ArrowUp") moveCursor(-columnsPerRow, e.shiftKey);
    else if (e.key === "ArrowRight" && !details) moveCursor(1, e.shiftKey);
    else if (e.key === "ArrowLeft" && !details) moveCursor(-1, e.shiftKey);
    else if (e.key === "Home") moveCursor(-entries.length, e.shiftKey);
    else if (e.key === "End") moveCursor(entries.length, e.shiftKey);
    else if (e.key === "PageDown") moveCursor(columnsPerRow * 15, e.shiftKey);
    else if (e.key === "PageUp") moveCursor(-columnsPerRow * 15, e.shiftKey);
    else if (e.key === "Enter" && first) open(first);
    else if (e.key === "Backspace") {
      const up = parentPath(path);
      if (up) navigate(up);
    } else if (e.key === "Delete" && paths.length) void deletePaths(paths, e.shiftKey);
    else if (e.key === "F2" && first) startRename();
    else if (e.key === "F5") refresh();
    else if (mod && e.key.toLowerCase() === "a") setSelected(new Set(entries.map((x) => x.name)));
    else if (mod && e.key.toLowerCase() === "c" && paths.length) setClipboard({ paths, op: "copy" });
    else if (mod && e.key.toLowerCase() === "x" && paths.length) setClipboard({ paths, op: "move" });
    else if (mod && e.key.toLowerCase() === "v") paste();
    else if (mod && e.shiftKey && e.key.toLowerCase() === "n") void create(true);
    else if (e.altKey && e.key === "Enter" && paths.length) void showProperties(paths);
    else if (e.key === "Escape") setSelected(new Set());
    else if (e.key.length === 1 && !mod && !e.altKey) {
      // Type-ahead: jump to the first name starting with what was typed.
      const now = Date.now();
      const t = typeahead.current;
      t.text = now - t.at < adv<number>(settings, "files.typeAheadMs") ? t.text + e.key.toLowerCase() : e.key.toLowerCase();
      t.at = now;
      const i = entries.findIndex((x) => x.name.toLowerCase().startsWith(t.text));
      if (i >= 0) {
        selectIndex(i, { ctrlKey: false, metaKey: false, shiftKey: false });
        virtualizer.scrollToIndex(Math.floor(i / columnsPerRow));
      }
    } else handled = false;
    if (handled) e.preventDefault();
  };

  // ---- Hover prefetch: load a folder's listing before it is clicked ---------
  const hoverTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const onHover = (e: FileEntry) => {
    clearTimeout(hoverTimer.current);
    if (!e.isDir || !settings.performance.predictivePrefetch) return;
    if (!isRemote(path) && !isArchiveLoc(path)) hoverTimer.current = setTimeout(() => void api.prefetch([fullPath(e.name)]).catch(() => {}), adv<number>(settings, "perf.prefetchDelayMs"));
  };

  const cut = clipboard?.op === "move" ? new Set(clipboard.paths) : null;
  const showExt = settings.general.showExtensions;
  const display = (e: FileEntry) => {
    if (showExt || e.isDir) return e.name;
    const i = e.name.lastIndexOf(".");
    return i > 0 ? e.name.slice(0, i) : e.name;
  };

  /** What a drag that starts on `e` carries: the whole selection if `e` is in it, else just `e`. */
  const dragPayload = (e: FileEntry): DragPayload => {
    const names = selected.has(e.name) ? entries.filter((x) => selected.has(x.name)) : [e];
    return {
      paths: names.map((x) => fullPath(x.name)),
      dirs: names.map((x) => x.isDir),
      label: names.length === 1 ? names[0].name : `${names.length} items`,
    };
  };

  const isArchiveDrop = (e: FileEntry) => !e.isDir && isArchiveFile(e.name) && !isRemote(path) && !isArchiveLoc(path);

  const itemProps = (e: FileEntry, i: number) => ({
    // A folder takes what is dropped on it; so does an archive file, which adds to it.
    "data-drop": e.isDir || isArchiveDrop(e) ? "folder" : undefined,
    "data-drop-path": e.isDir ? fullPath(e.name) : isArchiveDrop(e) ? archiveUrl(fullPath(e.name)) : undefined,
    onPointerDown: (ev: React.PointerEvent) => {
      // Ctrl and Shift are for choosing several items, not for dragging.
      if (renaming || ev.ctrlKey || ev.shiftKey || ev.metaKey) return;
      beginDrag(ev, () => {
        if (!selected.has(e.name)) setSelected(new Set([e.name]));
        return dragPayload(e);
      });
    },
    "data-selected": selected.has(e.name) || undefined,
    "data-cut": cut?.has(fullPath(e.name)) || undefined,
    "data-hidden": e.isHidden || undefined,
    "data-kind": kindOf(e.name, e.isDir),
    onMouseDown: (ev: React.MouseEvent) => {
      collapseOnClick.current = null;
      const plain = ev.button === 0 && !ev.ctrlKey && !ev.shiftKey && !ev.metaKey;
      if (plain && selected.size > 1 && selected.has(e.name) && adv<boolean>(settings, "files.keepSelectionOnDrag")) {
        // Pressing on one of several selected items may start a drag of all of them.
        // The selection narrows to this item only if the button is released without dragging.
        collapseOnClick.current = i;
        cursor.current = i;
      } else if (ev.button === 0) selectIndex(i, ev);
      ev.stopPropagation();
    },
    onClick: (ev: React.MouseEvent) => {
      if (collapseOnClick.current === i) {
        collapseOnClick.current = null;
        selectIndex(i, { ctrlKey: false, metaKey: false, shiftKey: false });
      }
      if (adv<boolean>(settings, "files.singleClickOpen") && ev.button === 0 && !ev.ctrlKey && !ev.shiftKey && !ev.metaKey) open(e);
    },
    onDoubleClick: () => !adv<boolean>(settings, "files.singleClickOpen") && open(e),
    onMouseEnter: () => onHover(e),
    onAuxClick: (ev: React.MouseEvent) => {
      if (ev.button === 1 && e.isDir && adv<string>(settings, "files.middleClick") === "newTab") newTab(fullPath(e.name), { background: true });
    },
    onContextMenu: (ev: React.MouseEvent) => {
      ev.preventDefault();
      ev.stopPropagation();
      cursor.current = i;
      showMenu(ev.clientX, ev.clientY, e);
    },
  });

  const nameCell = (e: FileEntry) =>
    renaming === e.name ? (
      <input
        className="rename-input"
        defaultValue={e.name}
        autoFocus
        onFocus={(ev) => {
          const dot = e.isDir ? -1 : e.name.lastIndexOf(".");
          ev.target.setSelectionRange(0, dot > 0 ? dot : e.name.length);
        }}
        onBlur={(ev) => void commitRename(e.name, ev.target.value.trim())}
        onKeyDown={(ev) => {
          if (ev.key === "Enter") ev.currentTarget.blur();
          if (ev.key === "Escape") setRenaming(null);
          ev.stopPropagation();
        }}
        onMouseDown={(ev) => ev.stopPropagation()}
        onDoubleClick={(ev) => ev.stopPropagation()}
      />
    ) : (
      <span className="file-name">{display(e)}</span>
    );

  // ---- Column header with drag-to-resize -------------------------------------
  const resize = (key: keyof Columns, ev: React.PointerEvent) => {
    ev.preventDefault();
    ev.stopPropagation();
    const target = ev.currentTarget;
    target.setPointerCapture(ev.pointerId);
    const startX = ev.clientX;
    const real: keyof Columns = key;
    const sign = 1;
    // Start from the width the column has now (Name may be narrower than it is set to).
    const start = cols[real];
    let latest = columns;
    const move = (m: PointerEvent) => {
      latest = { ...columns, [real]: Math.max(MIN_COLUMN[real], Math.round(start + sign * (m.clientX - startX))) };
      setColumns(latest);
    };
    const up = () => {
      target.removeEventListener("pointermove", move as EventListener);
      target.removeEventListener("pointerup", up);
      try {
        localStorage.setItem("fast.columns", JSON.stringify(latest));
      } catch {
        /* not persisted this time */
      }
    };
    target.addEventListener("pointermove", move as EventListener);
    target.addEventListener("pointerup", up);
  };

  const header = (key: SortKey, label: string, col: keyof Columns) => (
    <div className="column-cell" key={key} style={{ width: cols[col] }}>
      <button
        className="column-header"
        data-column={key}
        data-sorted={sort.key === key ? (sort.asc ? "asc" : "desc") : undefined}
        onClick={() => setSort({ key, asc: sort.key === key ? !sort.asc : true })}
      >
        {label}
      </button>
      <div className="column-grip" role="separator" aria-label={`Resize ${label}`} onPointerDown={(e) => resize(col, e)} onDoubleClick={() => setColumns({ ...columns, [col]: DEFAULT_COLUMNS[col] })} />
    </div>
  );

  const totalSize = entries.reduce((s, e) => s + e.size, 0);
  const selectedSize = entries.filter((e) => selected.has(e.name)).reduce((s, e) => s + e.size, 0);
  const filtered = isFiltering(filter);

  return (
    <div className="file-view" data-view={details ? "details" : "grid"} data-level={level}>
      <div
        ref={scroller}
        className="file-scroller"
        data-drop="folder"
        data-drop-path={path}
        tabIndex={0}
        onKeyDown={onKeyDown}
        onMouseDown={() => setSelected(new Set())}
        onContextMenu={(ev) => {
          ev.preventDefault();
          setSelected(new Set());
          showMenu(ev.clientX, ev.clientY, null);
        }}
      >
        {details && (
          <div className="file-header" style={{ width: rowsWidth + 14 }} onMouseDown={(e) => e.stopPropagation()}>
            {header("name", "Name", "name")}
            {header("modified", "Date modified", "modified")}
            {header("type", "Type", "type")}
            {header("size", "Size", "size")}
          </div>
        )}
        {error && isRemote(path) && /^(needs-login|untrusted-host|changed-host):/.test(error) ? (
          <div className="empty-state">
            <strong>Log in to {remoteLabel(path)}</strong>
            <span>The password is not kept between sessions.</span>
            <button className="primary" onClick={() => useStore.getState().openConnect({ url: path })}>Log in…</button>
          </div>
        ) : error && error.startsWith("locked:") ? (
          <LockedCard path={path} onOpened={refresh} />
        ) : error ? (
          <div className="empty-state" data-error>
            <strong>This folder can't be opened</strong>
            <span>{error}</span>
          </div>
        ) : !current ? (
          <div className="loading-bar" />
        ) : entries.length === 0 ? (
          <div className="empty-state"><strong>{filtered ? "Nothing matches the filter." : "This folder is empty."}</strong>{!filtered && <span>Drop files here, or use New to make something.</span>}</div>
        ) : (
          <div className="file-rows" style={{ height: virtualizer.getTotalSize() }}>
            {virtualizer.getVirtualItems().map((row) =>
              details ? (
                (() => {
                  const e = entries[row.index];
                  return (
                    <div
                      key={e.name}
                      className="file-row"
                      style={{ transform: `translateY(${row.start - headerHeight}px)`, height: rowHeight, width: rowsWidth }}
                      {...itemProps(e, row.index)}
                    >
                      <div className="cell cell-name" style={{ width: cols.name }}>
                        <Thumb path={fullPath(e.name)} name={e.name} isDir={e.isDir} modified={e.modified} size={18} iconSize={16} view="details" />
                        {nameCell(e)}
                      </div>
                      <div className="cell cell-date" style={{ width: columns.modified }}>{formatDate(e.modified)}</div>
                      <div className="cell cell-type" style={{ width: columns.type }}>{typeLabel(e.name, e.isDir)}</div>
                      <div className="cell cell-size" style={{ width: columns.size }}>{!e.isDir ? formatSize(e.size) : !wantSizes || e.isSymlink || isRemote(path) || isArchiveLoc(path) ? "" : folderSizes[e.name] ? `${folderSizes[e.name].complete ? "" : "≥ "}${formatSize(folderSizes[e.name].bytes)}` : "…"}</div>
                    </div>
                  );
                })()
              ) : (
                <div key={row.index} className="tile-row" style={{ transform: `translateY(${row.start}px)`, height: rowHeight }}>
                  {entries.slice(row.index * columnsPerRow, (row.index + 1) * columnsPerRow).map((e, k) => (
                    <div
                      key={e.name}
                      className="file-tile"
                      data-layout={tileLayout.mode}
                      style={{ width: tileLayout.w, height: tileLayout.h - 4 }}
                      title={e.name}
                      {...itemProps(e, row.index * columnsPerRow + k)}
                    >
                      <Thumb path={fullPath(e.name)} name={e.name} isDir={e.isDir} modified={e.modified} size={tileLayout.icon} view={level} />
                      {nameCell(e)}
                    </div>
                  ))}
                </div>
              ),
            )}
          </div>
        )}
      </div>
      <div className="status-bar">
        <span>
          {entries.length.toLocaleString()} item{entries.length === 1 ? "" : "s"}
          {filtered && current ? ` (filtered from ${current.entries.length.toLocaleString()})` : ""}
        </span>
        {selected.size > 0 && (
          <span>
            {selected.size.toLocaleString()} selected{selectedSize > 0 ? ` · ${formatSize(selectedSize)}` : ""}
          </span>
        )}
        {selected.size === 0 && totalSize > 0 && <span>{formatSize(totalSize)} in total</span>}
        {clipboard && (
          <span className="status-clipboard">
            {clipboard.paths.length} item{clipboard.paths.length === 1 ? "" : "s"} ready to {clipboard.op === "move" ? "move" : "copy"} · Ctrl+V to paste
          </span>
        )}
      </div>
      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
    </div>
  );
}
