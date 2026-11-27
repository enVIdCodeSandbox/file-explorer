// The folder view: a virtualized details list or thumbnail grid. Only the
// rows on screen (plus a small overscan) exist in the DOM, so a folder with
// 100,000 files scrolls as smoothly as one with ten.

import { sk, skShift } from "../util/shortcutText";
import { viewLevelOf } from "../util/viewLevel";
import { createNotice, createPacer, recallVisit, refreshMinMs, rememberVisit } from "../util/progressive";
import { wheelSteps } from "../util/zoomKeys";
import { useVirtualizer } from "@tanstack/react-virtual";
import { SelectGroup } from "./SelectGroup";
import { fileToolsItems } from "./toolsOpen";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { changedSince, followFiles, useLive } from "../util/liveFiles";
import { api, on, type FileEntry, type FolderSize, type Listing } from "../api";
import {
  ICON_PX,
  VIEW_LEVELS,
  createPath,
  compressPaths,
  deletePaths,
  extractAndOpenTerminal,
  extractArchive,
  isArchiveName,
  openFile,
  openSshFile,
  promptText,
  renamePath,
  showPermissions,
  showProperties,
  useStore,
} from "../store";
import { useSelectionBand } from "./useSelectionBand";
import { applyFilter, facets as facetsOf, isFiltering, sortEntries } from "../util/filter";
import { formatSize, kindOf } from "../util/format";
import { cellValue, rawValue } from "../util/cellValue";
import { flipRange, toggled } from "../util/checkSelect";
import { columnDef, hideColumn, moveColumn, moveColumnTo, showColumn, widthOf, type ColumnDef } from "../util/columns";
import { applyColumnFilters, columnFacets } from "../util/columnFilter";
import { columnsPerFolder } from "../folderViews";
import { ColumnFilter } from "./ColumnFilter";
import { adv } from "../settings/advanced";
import { closestIndex, nextText } from "../util/typeFind";
import { taskFromSelection } from "./tasksUtil";
import { LockedCard } from "./Locks";
import { MacAccessHelp } from "./MacAccess";
import { commandsFor, keyFor } from "../settings/keys";
import { beginDrag, type DragPayload } from "../dnd";
import { analyzeLocation, archiveUrl, insideArchive, isArchiveFile, isArchiveLoc, isRemote, isSshLoc, baseName, joinPath, parentPath, remoteLabel, splitArchive } from "../util/path";
import { classifyRemoteFile } from "../util/remoteFileKind";
import { useTerminal } from "../terminal/state";
import { takeSelection } from "../util/unload";
import { usePulse } from "../util/pulse";
import { COLUMN_MAX_WIDTH } from "../util/limits";
import { ContextMenu, type MenuItem, type MenuState } from "./ContextMenu";
import { Thumb } from "./Thumb";
import { prefetchShell, sameShell, useShellItems } from "./shellMenu";
import { shellPrefetch } from "../util/shellCache";
import { loadOpener, openerApplies, openerHint, peekOpener, showOpener } from "../util/opener";
import { Icon } from "./Icons";
import { shareOutside, useMobile } from "../util/device";
import { clampUiIconSize } from "../util/limits";

/** How each icon level lays out its items. */
function layoutFor(level: string, thumbPx: number, scale = 1, baseIcon = 16) {
  if (level === "list") {
    const icon = Math.round(baseIcon * scale);
    return { mode: "inline" as const, w: Math.round(230 * scale) + Math.max(0, icon - 16), h: Math.max(Math.round(24 * scale), icon + 8), icon };
  }
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

let listTickets = 0;

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
  // Details and List have sizes of their own (zooming changes them before it changes the view).
  const sizePct = adv<number>(settings, level === "details" ? "view.zoomDetails" : "view.zoomList");
  const phoneLayout = useMobile();
  const baseUiIcon = clampUiIconSize(adv<number>(settings, "ui.iconSize"));
  const zoom = (level === "details" || level === "list") && !phoneLayout ? Math.max(0.5, Math.min(2, sizePct / 100)) : 1;
  const tileLayout = layoutFor(level, thumbPx, zoom, baseUiIcon);
  // The phone layout has two shapes: a list of two-line rows, and a grid of icons.
  const mobile = useMobile();
  const details = level === "details" || (mobile && (level === "list" || level === "small"));
  const selecting = useStore((s) => s.selecting);
  const setSelecting = (on: boolean) => useStore.setState({ selecting: on });

  const [listing, setListing] = useState<Listing | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [renaming, setRenaming] = useState<string | null>(null);
  const [group, setGroup] = useState<"select" | "deselect" | null>(null);
  // The selection from before the last time it was cleared or replaced as a whole, for "Restore selection" (Num /).
  const lastSelection = useRef<{ now: Set<string>; before: Set<string> }>({ now: new Set(), before: new Set() });
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [width, setWidth] = useState(800);
  // The columns this folder shows: its own choice, else the one for all folders (always that one when "same columns in every folder" is on).
  const ownColumns = columnsPerFolder(settings);
  const columnSet = useStore((s) => (ownColumns ? s.folderViews[path]?.columns : undefined) ?? s.columnSet);
  const [colFilterOpen, setColFilterOpen] = useState<{ id: string; x: number; y: number } | null>(null);
  const [dragCol, setDragCol] = useState<{ id: string; to: number } | null>(null);
  const headerRef = useRef<HTMLDivElement>(null);
  const justDragged = useRef(false);
  const [liveWidths, setLiveWidths] = useState<Record<string, number>>({});
  const checks = adv<boolean>(settings, "files.checkboxes") && !mobile;
  const singleOpen = (e: FileEntry) => adv<boolean>(settings, "files.singleClickOpen") || (adv<boolean>(settings, "files.singleClickFolders") && e.isDir);
  const anchor = useRef(0);
  const cursor = useRef(0);
  const scroller = useRef<HTMLDivElement>(null);
  const typeahead = useRef({ text: "", at: 0 });
  // Type to find (off unless switched on): the typed text, shown in a small label while it lasts.
  const typeFind = adv<boolean>(settings, "files.typeFind");
  const [findText, setFindText] = useState("");
  const findTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const menuTicket = useRef(0);
  const { fetchFor, peek } = useShellItems();

  // ---- Loading ------------------------------------------------------------
  const progressive = adv<boolean>(settings, "files.progressiveLoad");
  const [loading, setLoading] = useState(false);
  const refreshMs = refreshMinMs(settings);
  useEffect(() => {
    let alive = true;
    let off: (() => void) | undefined;
    const ticket = progressive ? ++listTickets : undefined;
    setError(null);
    const startedAt = Date.now();
    const visit = recallVisit(path);
    const pacer = createPacer(refreshMs);
    let over = false;
    const finish = () => {
      // Every item is in: nothing may announce "still loading" afterwards (the notice's timer, a waiting redraw).
      over = true;
      notice.stop();
      pacer.cancel();
      setLoading(false);
      if (useStore.getState().loadingDir?.path === path) useStore.setState({ loadingDir: null });
    };
    // A folder that takes a moment says so (a phone hands its list over all at once, so nothing else would show):
    // not for one that opens at once, which would only flash.
    const show = () => {
      if (!alive || over || useStore.getState().loadingDir?.path === path) return;
      useStore.setState({ loadingDir: { path, done: 0, total: visit?.count ?? 0, totalFrom: visit ? "last-visit" : undefined, lastMs: visit?.ms, startedAt } });
    };
    const notice = createNotice(show);
    void (async () => {
      if (ticket !== undefined) {
        // Listen before asking, so the first batch cannot be missed.
        let seen: FileEntry[] = [];
        const un = await on("dir-chunk", (c) => {
          if (!alive || c.ticket !== ticket) return;
          seen = c.restart ? [...c.entries] : seen.concat(c.entries);
          const upTo = seen;
          // Batches are drawn at a steady pace, the latest one standing for those skipped, so a fast device cannot
          // keep a slow computer busy redrawing.
          pacer.push(() => {
            if (!alive || over) return;
            const t = performance.now();
            setListing({ path, parent: parentPath(path), entries: upTo, cached: false });
            setLoading(true);
            const fromSystem = c.total > 0;
            const total = fromSystem ? c.total : visit && visit.count > c.done ? visit.count : 0;
            useStore.setState({ loadingDir: { path, done: c.done, total, totalFrom: total ? (fromSystem ? "system" : "last-visit") : undefined, lastMs: visit?.ms, startedAt } });
            requestAnimationFrame(() => pacer.drew(performance.now() - t));
          });
        });
        if (!alive) return un();
        off = un;
      }
      try {
        const l = await api.listDir(path, ticket);
        pacer.cancel();
        if (alive) {
          setListing(l);
          if (isSshLoc(l.path)) void useTerminal.getState().openForSsh(l.path);
          if (Date.now() - startedAt > 800) rememberVisit(path, l.entries.length, Date.now() - startedAt);
        }
      } catch (e) {
        pacer.cancel();
        if (alive) setError(String(e));
      } finally {
        if (alive) finish();
      }
    })();
    return () => {
      alive = false;
      pacer.cancel();
      off?.();
      if (ticket !== undefined) void api.listDirCancel(ticket).catch(() => {});
      finish();
    };
  }, [path, version, settings.general.showHidden, progressive]);

  useEffect(() => {
    // Coming back after the window was out of sight: the selection and place in the list are kept.
    const resumed = takeSelection(path, parentPath, baseName);
    setSelected(new Set(resumed ?? []));
    setRenaming(null);
    useStore.setState({ selecting: false });
    if (!resumed) scroller.current?.scrollTo({ top: 0 });
    // On a phone, focusing would do nothing useful and can bring up the keyboard's accessory bar.
    if (!mobile) scroller.current?.focus();
  }, [path]); // eslint-disable-line react-hooks/exhaustive-deps
  // Leaving the folder view (to Settings, another page) ends select mode.
  useEffect(() => () => useStore.setState({ selecting: false }), []);

  const current = listing?.path === path ? listing : null;

  // ---- Folder sizes: measured in the background, shown as they arrive -----
  const wantSizes = adv<boolean>(settings, "files.folderSizes");
  const [folderSizes, setFolderSizes] = useState<Record<string, FolderSize>>({});
  const sizedPath = useRef("");
  // Sizes seen this session, so a folder revisited shows them before anything is asked.
  const remember = adv<boolean>(settings, "files.sizeCache");
  // The sizes the app remembers are sent the instant they are asked for. The ear for them must be open first,
  // or a size sent in that gap is lost and the folder shows "…" until it is next measured.
  const listening = useRef<Promise<unknown>>(Promise.resolve());
  useEffect(() => {
    if (!wantSizes) return;
    const off = on("folder-size", (m) => {
      if (m.dir !== path) return;
      if (remember) rememberSize(m);
      setFolderSizes((prev) => ({ ...prev, [m.name]: m }));
    });
    listening.current = off;
    return () => void off.then((f) => f());
  }, [path, wantSizes, remember]);
  useEffect(() => {
    if (!current || loading || !wantSizes || isRemote(current.path) || isArchiveLoc(current.path)) return;
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
      void listening.current.catch(() => {}).then(() => api.folderSizesStart(current.path, names, same, policy)).catch(() => {});
    }
  }, [current, wantSizes, loading]);

  const sized = useMemo(
    () =>
      current && wantSizes
        ? current.entries.map((e) => (e.isDir && folderSizes[e.name] ? { ...e, size: folderSizes[e.name].bytes } : e))
        : (current?.entries ?? []),
    [current, folderSizes, wantSizes],
  );
  // Details read from the files themselves (date accessed, dimensions, artist…), for the columns shown.
  const noDiskDetails = isRemote(path) || isArchiveLoc(path);
  const shown = useMemo(() => columnSet.order.map((id) => columnDef(id)).filter((d): d is ColumnDef => !!d), [columnSet]);
  // A column can sort by another's values (Length sorts by length in seconds).
  const sortDef = columnDef(sort.key);
  const sortSrc = (sortDef?.sortBy ? columnDef(sortDef.sortBy) : undefined) ?? sortDef;
  const sortNeedsDisk = !noDiskDetails && sortSrc?.source === "disk";
  // Columns being filtered by their values, and which of them are read from the files.
  const colFilterIds = Object.entries(filter.cols ?? {}).filter(([, v]) => v.length > 0).map(([id]) => id);
  const filterNeedsDisk = !noDiskDetails && colFilterIds.some((id) => columnDef(id)?.source === "disk");
  const wanted = useMemo(() => {
    const ids = noDiskDetails || !details ? [] : shown.filter((d) => d.source === "disk").map((d) => d.id);
    if (sortNeedsDisk && sortSrc && !ids.includes(sortSrc.id)) ids.push(sortSrc.id);
    for (const id of colFilterIds) if (columnDef(id)?.source === "disk" && !noDiskDetails && !ids.includes(id)) ids.push(id);
    return ids;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [shown, noDiskDetails, details, sortNeedsDisk, sort.key, colFilterIds.join()]);
  const [extra, setExtra] = useState<Record<string, Record<string, string>>>({});
  const asked = useRef(new Set<string>());
  useEffect(() => {
    setExtra({});
    asked.current = new Set();
  }, [path, version]);
  const fetchDetails = (names: string[]) => {
    if (!wanted.length) return;
    const missing = names.filter((n) => wanted.some((k) => !asked.current.has(`${n}\0${k}`)));
    for (let at = 0; at < missing.length; at += 200) {
      const chunk = missing.slice(at, at + 200);
      for (const n of chunk) for (const k of wanted) asked.current.add(`${n}\0${k}`);
      void api
        .columnValues(path, chunk, wanted)
        .then((rows) =>
          setExtra((prev) => {
            const next = { ...prev };
            for (const r of rows) next[r.name] = { ...next[r.name], ...r.values };
            return next;
          }),
        )
        .catch(() => {});
    }
  };
  const sorted = useMemo(
    () =>
      current
        ? sortEntries(sized, sort.key, sort.asc, settings.general.foldersFirst, sortSrc && (sortSrc.source === "disk" || sortSrc.derive || sortSrc.kind === "number") ? { kind: sortSrc.kind, get: (e) => rawValue(sortSrc, e, extra[e.name]) } : undefined)
        : [],
    // The fetched values only matter for the order when the order is by one of them.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [current, sized, sort, settings.general.foldersFirst, sortNeedsDisk ? extra : null],
  );
  const listed = useMemo(
    () => applyColumnFilters(applyFilter(sorted, filter), filter.cols, columnDef, extra),
    // The fetched values only matter when a column is being filtered by them.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [sorted, filter, filterNeedsDisk ? extra : null],
  );

  // Files of this folder that are being written: their size (and date) are kept up to date, nothing else is touched.
  // The selector gives text, so the list only redraws when one of those files actually changed.
  const liveEnabled = adv<boolean>(settings, "live.enabled");
  const liveSizes = liveEnabled && details && adv<boolean>(settings, "live.sizes");
  const growingHere = useLive((st) =>
    !liveSizes
      ? ""
      : Object.values(st.files)
          .filter((l) => l.growing && parentPath(l.path) === path)
          .map((l) => `${l.path}:${l.size}`)
          .join("|"),
  );
  const entries = useMemo(() => {
    if (!growingHere) return listed;
    const files = useLive.getState().files;
    return listed.map((e) => {
      const l = e.isDir ? undefined : files[joinPath(path, e.name)];
      return l?.growing ? { ...e, size: l.size, modified: l.modified ?? e.modified } : e;
    });
  }, [listed, growingHere, path]);

  // Follow the files that the listing shows changing (new, grown, touched a moment ago).
  const lastListing = useRef<{ path: string; seen: Map<string, { size: number; modified: number | null }> } | null>(null);
  useEffect(() => {
    if (!current || loading || !liveEnabled || isRemote(path) || isArchiveLoc(path)) return;
    const before = lastListing.current?.path === path ? lastListing.current.seen : null;
    const recentMs = Math.max(1, adv<number>(settings, "live.recentSeconds")) * 1000;
    const follow = changedSince(path, current.entries, before, recentMs);
    if (follow.length) followFiles(follow.slice(0, 64));
    lastListing.current = { path, seen: new Map(current.entries.map((e) => [e.name, { size: e.size, modified: e.modified }])) };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [current, loading, liveEnabled, path]);

  // A refresh you asked for shows: a bar across the top of the list and the list dips for a moment.
  const refreshedAt = useStore((st) => st.refreshedAt);
  const refreshing = usePulse(refreshedAt, 900);

  // Prewarm thumbnails in the background ahead of scroll: slow videos/docs on medium+, images too on high tiers.
  const prewarmMode = adv<string>(settings, "thumbs.videoPrewarm");
  const prewarmImagesMode = adv<string>(settings, "thumbs.prewarmImages");
  const budget = useStore((st) => st.budget);
  const strongBudget = budget?.tier === "full" || budget?.tier === "manual";
  const prewarmOn = prewarmMode === "on" || (prewarmMode === "auto" && strongBudget);
  const prewarmImgsOn = prewarmImagesMode === "on" || (prewarmImagesMode === "auto" && !!budget?.prewarmImages);
  useEffect(() => {
    if ((!prewarmOn && !prewarmImgsOn) || isRemote(path) || isArchiveLoc(path) || !current?.entries.length) return;
    const delayMs = (budget?.intensity ?? 50) >= 90 ? 0 : 300;
    const timer = setTimeout(() => {
      const slow = prewarmOn
        ? current.entries.filter((e) => !e.isDir && /\.(mp4|m4v|mov|mkv|avi|webm|wmv|flv|mpg|mpeg|3gp|mts|m2ts|pdf|docx?|xlsx?|pptx?)$/i.test(e.name))
        : [];
      const imgs = prewarmImgsOn
        ? current.entries.filter((e) => !e.isDir && /\.(jpe?g|png|gif|webp|bmp)$/i.test(e.name))
        : [];
      const all = [...slow, ...imgs].map((e) => fullPath(e.name));
      if (all.length) void api.thumbsPrewarm(all, settings.performance.thumbnailSize || 256).catch(() => {});
    }, delayMs);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [prewarmOn, prewarmImgsOn, path, current, budget?.intensity]);

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

  useEffect(() => {
    const keep = lastSelection.current;
    // One item ticked or unticked is an edit; anything bigger (cleared, all, a group, a new folder) replaces the selection.
    let changed = 0;
    selected.forEach((n) => keep.now.has(n) || changed++);
    keep.now.forEach((n) => selected.has(n) || changed++);
    if (keep.now.size > 0 && (changed > 1 || selected.size === 0)) keep.before = keep.now;
    keep.now = selected;
  }, [selected]);
  const restoreSelection = () => {
    const names = new Set(entries.map((e) => e.name));
    const before = [...lastSelection.current.before].filter((n) => names.has(n));
    if (before.length) setSelected(new Set(before));
  };

  // Commands from the command bar (Select all, Rename…) and a name to select
  // once it exists (after creating something).
  useEffect(() => {
    if (!command) return;
    if (command.name === "selectAll") setSelected(new Set(entries.map((e) => e.name)));
    else if (command.name === "selectNone") setSelected(new Set());
    else if (command.name === "selectGroup") setGroup("select");
    else if (command.name === "deselectGroup") setGroup("deselect");
    else if (command.name === "restoreSelection") restoreSelection();
    else if (command.name === "invert") setSelected(new Set(entries.filter((e) => !selected.has(e.name)).map((e) => e.name)));
    else if (command.name === "rename") startRename();
    else if (command.name === "select") setSelecting(true);
    else if (command.name === "newFolder") void create(true);
    else if (command.name === "newFile") void create(false);
    else if (command.name === "paste") paste();
    else if (command.name === "refresh") refresh();
    else if (command.name === "menu") showMenu(window.innerWidth, 0, null);
    else if (command.name === "open") {
      const first = entries.find((e) => selected.has(e.name));
      if (first) open(first);
    } else if (command.name === "quickLook") {
      const first = entries.find((e) => selected.has(e.name));
      if (first && !first.isDir) viewFile(first, true);
    }
  }, [command?.tick]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!pendingSelect || !current) return;
    const nameOf = (p: string) => p.slice(path.length).replace(/^[\\/]/, "");
    const name = nameOf(pendingSelect);
    if (current.entries.some((e) => e.name === name)) {
      // Several items can be asked for at once ("Show in folder" for a batch of downloads).
      const more = useStore.getState().pendingSelectMore.map(nameOf).filter((n) => current.entries.some((e) => e.name === n));
      setSelected(new Set([name, ...more]));
      useStore.setState({ pendingSelect: null, pendingSelectMore: [] });
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
      // One step per notch; trackpads send many small deltas, so accumulate.
      const r = wheelSteps(wheelDelta.current, e.deltaY);
      wheelDelta.current = r.carry;
      if (r.steps) {
        useStore.getState().zoomView(r.steps);
      }
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, []);

  // ---- Virtualization -----------------------------------------------------
  const compact = settings.appearance.density === "compact";
  const columnsPerRow = details ? 1 : Math.max(1, Math.floor((width - 16) / tileLayout.w));
  const rowCount = Math.ceil(entries.length / columnsPerRow);
  const customRow = adv<number>(settings, "ui.rowHeight");
  // A phone row is tall enough for a finger: an icon, the name, and a line of date and size.
  // A phone shows two lines of the name under an icon, so its tiles are a little taller.
  const tileH = tileLayout.h + (mobile ? 20 : 0);
  const detailsIcon = Math.round(baseUiIcon * zoom);
  const rowHeight = details ? (mobile ? Math.max(56, customRow) : Math.max(detailsIcon + 6, Math.round((customRow > 0 ? customRow : compact ? 26 : 34) * zoom))) : tileH;
  const headerHeight = details && !mobile ? 28 : 0;
  // On a phone the icons spread to fill the row instead of leaving a gap on the right.
  const tileW = mobile && !details ? Math.floor((width - 16) / columnsPerRow) : tileLayout.w;
  // Name keeps its own width and the list does not stretch to the pane's edge (the free
  // space on the right stays free). In a pane too narrow for that, Name gives way first,
  // so there is no sideways scrollbar unless the other columns alone are wider than the pane.
  const widthFor = (id: string) => liveWidths[id] ?? widthOf(columnSet, id);
  const othersWidth = shown.slice(1).reduce((sum, d) => sum + widthFor(d.id), 0);
  const nameWidth = Math.max(140, Math.min(widthFor("name"), width - 14 - othersWidth));
  const colWidth = (id: string) => (id === "name" ? nameWidth : widthFor(id));
  const rowsWidth = mobile ? width : nameWidth + othersWidth;
  const overscanRows = adv<number>(settings, "perf.overscanRows");
  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => scroller.current,
    estimateSize: () => rowHeight,
    // The sticky column header sits inside the scroller, above the rows.
    scrollMargin: headerHeight,
    paddingStart: details ? 0 : 8,
    // Scale overscan for both list and icon grid views; with lazy loading off, buffer 80 rows ahead.
    overscan: settings.performance.lazyLoading ? (details ? overscanRows : Math.max(2, Math.round(overscanRows / 2))) : 80,
  });
  useEffect(() => virtualizer.measure(), [rowHeight, virtualizer]);
  // Ask for the extra details of the rows on screen, a moment after scrolling settles.
  const visibleRows = virtualizer.getVirtualItems();
  const firstRow = visibleRows[0]?.index ?? 0;
  const lastRow = visibleRows.at(-1)?.index ?? 0;
  useEffect(() => {
    if (!wanted.length || !current) return;
    const t = setTimeout(() => fetchDetails(entries.slice(firstRow * columnsPerRow, (lastRow + 1) * columnsPerRow).map((e) => e.name)), 80);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [firstRow, lastRow, wanted.join(), entries, path]);
  // Ordering by one of them needs it for every item, not just those on screen.
  // Filtering by one needs it for every item too, and so does the list of values to choose from.
  const openFilterDisk = !!colFilterOpen && columnDef(colFilterOpen.id)?.source === "disk";
  useEffect(() => {
    if ((sortNeedsDisk || filterNeedsDisk || openFilterDisk) && current) fetchDetails(current.entries.map((e) => e.name));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sortNeedsDisk, filterNeedsDisk, openFilterDisk, sort.key, colFilterIds.join(), colFilterOpen?.id, current]);

  // ---- Actions ------------------------------------------------------------
  const fullPath = useCallback((name: string) => joinPath(path, name), [path]);
  const selectedPaths = () => entries.filter((e) => selected.has(e.name)).map((e) => fullPath(e.name));

  // With the setting on, the system's right-click entries for what is selected (or for the folder itself) are asked
  // for as soon as the selection settles, so the first right-click is as quick as the second.
  const prefetchOn = settings.general.shellExtras && !noDiskDetails && shellPrefetch(settings);
  useEffect(() => {
    if (!prefetchOn || loading) return;
    const timer = window.setTimeout(() => {
      const paths = selectedPaths();
      if (paths.length > 50) return;
      if (paths.length) prefetchShell(paths, false);
      else prefetchShell([path], true);
    }, 400);
    return () => window.clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [prefetchOn, loading, selected, path, entries]);

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
        // SSH text files open via nano/VS Code; byte-based files download a full copy to open locally.
        void openFile(target);
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

  // The rename box going away takes the keyboard focus with it; give it back so the arrow keys work straight away.
  const wasRenaming = useRef(false);
  useEffect(() => {
    if (renaming === null && wasRenaming.current && !mobile) scroller.current?.focus({ preventScroll: true });
    wasRenaming.current = renaming !== null;
  }, [renaming]); // eslint-disable-line react-hooks/exhaustive-deps
  // With one item selected the arrow keys start from it (a rename or a re-sort can move it to another row).
  useEffect(() => {
    if (selected.size !== 1) return;
    const i = entries.findIndex((x) => selected.has(x.name));
    if (i >= 0 && entries[cursor.current]?.name !== entries[i].name) cursor.current = i;
  }, [entries, selected]); // eslint-disable-line react-hooks/exhaustive-deps

  const commitRename = async (oldName: string, newName: string) => {
    setRenaming(null);
    if (!newName || newName === oldName) return;
    const made = await renamePath(fullPath(oldName), newName);
    if (made) setSelected(new Set([newName]));
  };

  // Inside an archive the terminal needs the contents unpacked first.
  const terminal = (dir: string, mode: "here" | "admin") => (isArchiveLoc(dir) ? extractAndOpenTerminal(dir, mode) : api.openTerminal(dir, mode).catch((e) => showToast(String(e))));
  const inArchiveHere = isArchiveLoc(path);
  const terminalLabel = (admin: boolean) => (inArchiveHere ? (admin ? "Unpack, then open terminal as administrator" : "Unpack, then open terminal here") : admin ? "Open terminal as administrator" : "Open terminal here");

  const refresh = () => useStore.getState().refreshTab();

  /** Right-clicking an unselected item selects just that item first. */
  const selectedPathsWith = (target: FileEntry): string[] => {
    if (selected.has(target.name)) return selectedPaths();
    // With checkboxes the ticked items stay ticked; the menu acts on the one clicked.
    if (!checks) setSelected(new Set([target.name]));
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
    // The program Open will use, when it is known (it is asked for as the menu opens and drawn in when it arrives).
    const openHint = single && !single.isDir && showOpener(settings) && openerApplies(fullPath(single.name)) ? openerHint(peekOpener(fullPath(single.name))) : undefined;
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
          single && { id: "open", label: "Open", hint: openHint, shortcut: sk("open"), run: () => open(single) },
          ...(single && !single.isDir && isSshLoc(fullPath(single.name))
            ? ([
                classifyRemoteFile(single.name) !== "binary" && {
                  id: "openSshNano",
                  label: "Edit with nano in terminal",
                  icon: "terminal",
                  run: () => void openSshFile(fullPath(single.name), "terminal"),
                },
                classifyRemoteFile(single.name) !== "binary" && {
                  id: "openSshVscode",
                  label: "Open in VS Code",
                  icon: "popout",
                  run: () => void openSshFile(fullPath(single.name), "vscode"),
                },
                {
                  id: "openSshCopy",
                  label: "Copy full file & open locally",
                  icon: "copy",
                  run: () => void openSshFile(fullPath(single.name), "copy"),
                },
              ] as (MenuItem | false)[])
            : []),
          picked.length > 0 && { id: "fileTools", label: "File tools", children: fileToolsItems(picked.map((x) => fullPath(x.name))) },
          single && !single.isDir && { id: "view", label: "View", shortcut: sk("view"), run: () => viewFile(single, false) },
          single && !single.isDir && { id: "quickLook", label: "Quick look", shortcut: sk("quickLook"), run: () => viewFile(single, true) },
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
          { id: "cut", label: "Cut", shortcut: sk("cut"), icon: "cut", separatorBefore: true, run: () => setClipboard({ paths, op: "move" }) },
          { id: "copy", label: "Copy", shortcut: sk("copy"), icon: "copy", run: () => setClipboard({ paths, op: "copy" }) },
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
          { id: "delete", label: "Delete", shortcut: sk("delete"), icon: "trash", run: () => void deletePaths(paths) },
          { id: "deletePermanent", label: "Delete permanently", shortcut: skShift("delete"), danger: true, run: () => void deletePaths(paths, true) },
          paths.length > 0 && { id: "rename", label: paths.length > 1 ? `Rename ${paths.length} items…` : "Rename", shortcut: sk("rename"), icon: "rename", run: startRename },
          single?.isDir && !isRemote(dirForTerminal) && { id: "terminal", label: terminalLabel(false), icon: "terminal", separatorBefore: true, run: () => void terminal(dirForTerminal, "here") },
          single?.isDir && !isRemote(dirForTerminal) && { id: "terminalAdmin", label: terminalLabel(true), run: () => void terminal(dirForTerminal, "admin") },
          single && !single.isDir && !isArchiveLoc(path) && isArchiveName(single.name) && { id: "archive-terminal", label: "Unpack, then open terminal in it", icon: "terminal", run: () => void extractAndOpenTerminal(archiveUrl(fullPath(single.name)), "here") },
          single?.isDir && { id: "analyze", label: "Analyze disk usage", icon: "chart", run: () => navigate(analyzeLocation(fullPath(single.name))) },
          paths.length > 0 && { id: "automate", label: "Automate with a task…", icon: "clock", separatorBefore: true, run: () => useStore.getState().openTasks(taskFromSelection(paths, path)) },
          single && { id: "permissions", label: "Permissions", icon: "shield", run: () => showPermissions(fullPath(single.name)) },
          { id: "properties", label: "Properties", icon: "info", separatorBefore: !(paths.length > 0), run: () => void showProperties(paths) },
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
          { id: "paste", label: "Paste", shortcut: sk("paste"), icon: "paste", separatorBefore: !!splitArchive(path), run: paste },
          { id: "newFolder", label: "New folder", shortcut: sk("newFolder"), icon: "folderOpen", separatorBefore: true, run: () => void create(true) },
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
            children: [...new Set(["name", "modified", "created", "type", "extension", "size", ...shown.map((d) => d.id)])].map((k) => ({
              id: `sort-${k}`,
              label: columnDef(k)?.label ?? k,
              checked: sort.key === k,
              run: () => setSort({ key: k, asc: sort.key === k ? !sort.asc : true }),
            })),
          },
          { id: "chooseDetails", label: "Choose details…", run: () => useStore.setState({ chooseDetailsOpen: true }) },
          { id: "refresh", label: "Refresh", shortcut: keyFor(settings, "refresh"), icon: "refresh", separatorBefore: true, run: refresh },
          { id: "copyPath", label: "Copy folder path", icon: "copy", run: () => void navigator.clipboard.writeText(path) },
          { id: "pin", label: pinned ? "Unpin this folder" : "Pin this folder", icon: "pin", run: () => togglePin(path) },
          !isRemote(path) && { id: "terminal", label: terminalLabel(false), icon: "terminal", separatorBefore: true, run: () => void terminal(path, "here") },
          !isRemote(path) && { id: "terminalAdmin", label: terminalLabel(true), run: () => void terminal(path, "admin") },
          { id: "analyze", label: "Analyze disk usage", icon: "chart", run: () => navigate(analyzeLocation(path)) },
          ...shell.map((m, i) => (i === 0 ? { ...m, separatorBefore: true } : m)),
          !isRemote(path) && { id: "automate", label: "Automate this folder with a task…", icon: "clock", separatorBefore: true, run: () => useStore.getState().openTasks(taskFromSelection([path], path)) },
          { id: "permissions", label: "Permissions", icon: "shield", separatorBefore: isRemote(path), run: () => showPermissions(path) },
          { id: "properties", label: "Properties", icon: "info", run: () => void showProperties([path]) },
          { id: "lock", label: "Lock this folder with passcode…", icon: "lock", separatorBefore: true, run: () => useStore.getState().openLockDialog(path) },
        ];
    const hidden = new Set(settings.contextMenu.hidden);
    // Things that only make sense for files on this computer.
    const localOnly = new Set(["pin", "share", "sendTo", "archive", "createShortcut", "deletePermanent", "terminal", "terminalAdmin", "analyze", "permissions", "properties", "newFile", "custom", "lock"]);
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
    const wantsShell = settings.general.shellExtras && !isRemote(path) && !isArchiveLoc(path);
    const paths = target ? selectedPathsWith(target) : [path];
    // What the system added last time for this selection is in the very first paint; the answer below refreshes it.
    const remembered = wantsShell ? peek(paths, !target) : null;
    let latest: MenuItem[] | undefined = remembered ?? undefined;
    setMenu({ x, y, items: buildMenu(target, latest) });
    // Which program Open uses: from memory it is already in the menu above, otherwise it is drawn in a moment later.
    if (target && !target.isDir && paths.length === 1 && showOpener(settings) && openerApplies(paths[0]) && !peekOpener(paths[0])) {
      void loadOpener(paths[0]).then(() => {
        if (ticket === menuTicket.current) setMenu({ x, y, items: buildMenu(target, latest) });
      }).catch(() => {});
    }
    if (!wantsShell) return;
    void fetchFor(paths, !target).then((shell) => {
      if (ticket !== menuTicket.current || shell.length === 0 || sameShell(remembered, shell)) return;
      latest = shell;
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

  // ---- Selection box (see useSelectionBand) ---------------------------------------
  const { band, startBand } = useSelectionBand({
    scroller,
    geometry: { count: entries.length, details, rowHeight, headerHeight, rowsWidth, columnsPerRow, tileW, tileH },
    keys: useMemo(() => entries.map((e) => e.name), [entries]),
    selected,
    sticky: checks,
    commit: (next) => commit(next),
    onLast: (i) => {
      cursor.current = i;
      anchor.current = i;
    },
  });

  /** Set when a plain press landed on an item inside a multi-selection; see itemProps. */
  const collapseOnClick = useRef<number | null>(null);
  const selectIndex = (i: number, e: { ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }, fromKeyboard = false) => {
    const entry = entries[i];
    if (!entry) return;
    cursor.current = i;
    const ctrl = e.ctrlKey || e.metaKey;
    // If the selection was changed some other way (select all, a rubber band), what is selected now is the base.
    if (selected !== lastSet.current) base.current = new Set(selected);
    if (checks) {
      // Checkbox mode: what is ticked stays ticked. A click ticks or unticks that one item, and
      // Shift+click turns over every item between the last click and this one (ticked ones become
      // unticked, the others ticked). The item clicked first keeps the state its click gave it.
      if (e.shiftKey) {
        const next = flipRange(entries.map((x) => x.name), base.current, anchor.current, i);
        commit(next);
      } else if (fromKeyboard) {
        anchor.current = i;
      } else {
        const next = toggled(selected, entry.name);
        base.current = new Set(next);
        anchor.current = i;
        commit(next);
      }
      return;
    }
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
    selectIndex(i, { ctrlKey: false, metaKey: false, shiftKey: shift }, true);
    virtualizer.scrollToIndex(Math.floor(i / columnsPerRow));
  };

  /** Open the viewer on a file, with the other files of the folder a key press away. */
  const viewFile = (e: FileEntry, quick: boolean) => {
    const siblings = entries.filter((x) => !x.isDir).map((x) => fullPath(x.name));
    const path = fullPath(e.name);
    const engine = String(settings.advanced["files.previewEngine"] ?? "builtin");
    const builtIn = () => useStore.getState().openViewer(path, { quick, siblings });
    if (quick && engine !== "builtin" && !isRemote(path) && !isArchiveLoc(path)) void api.previewExternal(path, engine).then((ok) => ok || builtIn()).catch(builtIn);
    else builtIn();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (renaming || (e.target as HTMLElement).tagName === "INPUT") return;
    const mod = e.ctrlKey || e.metaKey;
    const paths = selectedPaths();
    const first = entries.find((x) => selected.has(x.name));
    const extend = e.shiftKey;
    let handled = true;
    // Every key here is a shortcut from Settings → Keyboard and mouse (the defaults are the usual keys).
    const cmds = commandsFor(settings, e.nativeEvent);
    const is = (id: string) => cmds.includes(id);
    if (e.key === "Backspace" && typeFind && findText) typeInto(e.key);
    else if (is("open") && first) open(first);
    else if (is("openNewTab") && first?.isDir) newTab(fullPath(first.name), { background: true });
    else if (is("selectGroup")) setGroup("select");
    else if (is("deselectGroup")) setGroup("deselect");
    else if (is("invertSelection") || is("invertSelectionAlt")) setSelected(new Set(entries.filter((x) => !selected.has(x.name)).map((x) => x.name)));
    else if (is("restoreSelection")) restoreSelection();
    else if (is("view") && first && !first.isDir) viewFile(first, false);
    else if (is("rename") && first) startRename();
    else if (is("delete") && paths.length) void deletePaths(paths, e.shiftKey);
    else if (is("newFolder")) void create(true);
    else if (is("newFile")) void create(false);
    else if (is("properties") && paths.length) void showProperties(paths);
    else if (is("cursorDown")) moveCursor(columnsPerRow, extend);
    else if (is("cursorUp")) moveCursor(-columnsPerRow, extend);
    else if (is("cursorRight") && !details) moveCursor(1, extend);
    else if (is("cursorLeft") && !details) moveCursor(-1, extend);
    else if (is("cursorFirst")) moveCursor(-entries.length, extend);
    else if (is("cursorLast")) moveCursor(entries.length, extend);
    else if (is("pageDown")) moveCursor(columnsPerRow * 15, extend);
    else if (is("pageUp")) moveCursor(-columnsPerRow * 15, extend);
    else if (is("toggleCheck") && checks && entries[cursor.current]) selectIndex(cursor.current, { ctrlKey: false, metaKey: false, shiftKey: false });
    else if (is("quickLook") && first && !first.isDir) viewFile(first, true);
    else if (is("goUpList")) {
      const up = parentPath(path);
      if (up) navigate(up);
    } else if (is("selectAll")) setSelected(new Set(entries.map((x) => x.name)));
    else if (is("copy") && paths.length) setClipboard({ paths, op: "copy" });
    else if (is("cut") && paths.length) setClipboard({ paths, op: "move" });
    else if (is("paste")) paste();
    else if (is("copyPath") && paths.length) void navigator.clipboard.writeText(paths.join("\n"));
    else if (is("clearSelection")) {
      setSelected(new Set());
      setFindText("");
    } else if (typeFind && e.key.length === 1 && !mod && !e.altKey) typeInto(e.key);
    else if (e.key.length === 1 && !mod && !e.altKey) {
      // Type-ahead: jump to the first name starting with what was typed.
      const now = Date.now();
      const t = typeahead.current;
      t.text = now - t.at < adv<number>(settings, "files.typeAheadMs") ? t.text + e.key.toLowerCase() : e.key.toLowerCase();
      t.at = now;
      const i = entries.findIndex((x) => x.name.toLowerCase().startsWith(t.text));
      if (i >= 0) {
        selectIndex(i, { ctrlKey: false, metaKey: false, shiftKey: false }, true);
        virtualizer.scrollToIndex(Math.floor(i / columnsPerRow));
      }
    } else handled = false;
    if (handled) e.preventDefault();
  };

  const typeInto = (key: string) => {
    const text = nextText(findText, key);
    if (text === null) return;
    setFindText(text);
    clearTimeout(findTimer.current);
    findTimer.current = setTimeout(() => setFindText(""), adv<number>(settings, "files.typeFindResetSec") * 1000);
    const i = closestIndex(entries.map((x) => x.name), text);
    if (i >= 0) {
      selectIndex(i, { ctrlKey: false, metaKey: false, shiftKey: false }, true);
      virtualizer.scrollToIndex(Math.floor(i / columnsPerRow));
    }
  };
  useEffect(() => () => clearTimeout(findTimer.current), []);
  useEffect(() => setFindText(""), [path]);

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

  // ---- Touch (the phone layout) ---------------------------------------------
  // A tap opens. Pressing and holding starts select mode with that item ticked; from then
  // on a tap ticks or unticks. Android reports a long press as a "context menu" request;
  // iOS reports nothing, so a timer does the same job on both.
  const press = useRef<{ timer?: ReturnType<typeof setTimeout>; x: number; y: number; held: boolean }>({ x: 0, y: 0, held: false });
  const tick = (name: string, on?: boolean) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (on ?? !next.has(name)) next.add(name);
      else next.delete(name);
      return next;
    });
  const hold = (e: FileEntry) => {
    if (!press.current.held) {
      try {
        navigator.vibrate?.(12);
      } catch {
        /* no vibration motor, or not allowed */
      }
    }
    press.current.held = true;
    setSelecting(true);
    tick(e.name, true);
  };
  const touchProps = (e: FileEntry, i: number) => ({
    "data-selected": selected.has(e.name) || undefined,
    "data-cut": cut?.has(fullPath(e.name)) || undefined,
    "data-hidden": e.isHidden || undefined,
    "data-kind": kindOf(fullPath(e.name), e.isDir),
    onPointerDown: (ev: React.PointerEvent) => {
      clearTimeout(press.current.timer);
      press.current = { x: ev.clientX, y: ev.clientY, held: false };
      if (renaming || ev.button !== 0) return;
      press.current.timer = setTimeout(() => hold(e), 480);
    },
    onPointerMove: (ev: React.PointerEvent) => {
      // A finger that travels is scrolling, not holding.
      if (Math.abs(ev.clientX - press.current.x) > 10 || Math.abs(ev.clientY - press.current.y) > 10) clearTimeout(press.current.timer);
    },
    onPointerUp: () => clearTimeout(press.current.timer),
    onPointerCancel: () => clearTimeout(press.current.timer),
    onPointerLeave: () => clearTimeout(press.current.timer),
    onClick: (ev: React.MouseEvent) => {
      ev.stopPropagation();
      if (renaming) return;
      // The tap that ends a long press is not also a tap on the item.
      if (press.current.held) {
        press.current.held = false;
        return;
      }
      cursor.current = i;
      if (useStore.getState().selecting) tick(e.name);
      else open(e);
    },
    onContextMenu: (ev: React.MouseEvent) => {
      ev.preventDefault();
      ev.stopPropagation();
      clearTimeout(press.current.timer);
      hold(e);
    },
  });
  /** Phone layout: the items the bar along the bottom acts on. */
  const act = (what: "move" | "copy" | "share" | "delete" | "more", ev?: React.MouseEvent) => {
    const chosen = entries.filter((x) => selected.has(x.name));
    const paths = chosen.map((x) => fullPath(x.name));
    if (!paths.length) return;
    if (what === "move" || what === "copy") {
      setClipboard({ paths, op: what });
      setSelecting(false);
      setSelected(new Set());
      showToast(`Open the folder to ${what} ${paths.length === 1 ? "it" : "them"} to, then tap ${what === "move" ? "Move here" : "Copy here"}.`);
    } else if (what === "delete") {
      void deletePaths(paths).then((gone) => gone && setSelecting(false));
    } else if (what === "share") {
      if (chosen.some((x) => x.isDir) || isRemote(path) || isArchiveLoc(path)) return void useStore.setState({ shareFor: paths });
      const said = shareOutside(paths);
      if (said === null) useStore.setState({ shareFor: paths });
      else if (said) showToast(said);
    } else {
      const r = (ev?.currentTarget as HTMLElement | undefined)?.getBoundingClientRect();
      showMenu(r?.left ?? 0, r?.top ?? 0, chosen[0]);
    }
  };

  const itemProps = (e: FileEntry, i: number) => (mobile ? touchProps(e, i) : desktopProps(e, i));
  const desktopProps = (e: FileEntry, i: number) => ({
    // A folder takes what is dropped on it; so does an archive file, which adds to it.
    "data-drop": e.isDir || isArchiveDrop(e) ? "folder" : undefined,
    "data-drop-path": e.isDir ? fullPath(e.name) : isArchiveDrop(e) ? archiveUrl(fullPath(e.name)) : undefined,
    onPointerDown: (ev: React.PointerEvent) => {
      // Ctrl and Shift are for choosing several items, not for dragging.
      if (renaming || ev.ctrlKey || ev.shiftKey || ev.metaKey) return;
      beginDrag(ev, () => {
        if (!selected.has(e.name) && !checks) setSelected(new Set([e.name]));
        return dragPayload(e);
      });
    },
    "data-selected": selected.has(e.name) || undefined,
    "data-cut": cut?.has(fullPath(e.name)) || undefined,
    "data-hidden": e.isHidden || undefined,
    "data-kind": kindOf(fullPath(e.name), e.isDir),
    onMouseDown: (ev: React.MouseEvent) => {
      collapseOnClick.current = null;
      const plain = ev.button === 0 && !ev.ctrlKey && !ev.shiftKey && !ev.metaKey;
      if (checks && plain && singleOpen(e)) {
        // A click that opens the item does not also tick it.
        cursor.current = i;
      } else if (plain && !checks && selected.size > 1 && selected.has(e.name) && adv<boolean>(settings, "files.keepSelectionOnDrag")) {
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
      if (singleOpen(e) && ev.button === 0 && !ev.ctrlKey && !ev.shiftKey && !ev.metaKey) open(e);
    },
    onDoubleClick: () => !singleOpen(e) && open(e),
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

  /** The size shown for a folder: its total when folder sizes are on and it has been worked out. */
  const sizeText = (e: FileEntry) =>
    !wantSizes || e.isSymlink || isRemote(path) || isArchiveLoc(path) ? "" : folderSizes[e.name] ? `${folderSizes[e.name].complete ? "" : "≥ "}${formatSize(folderSizes[e.name].bytes)}` : "…";

  /** The selection checkbox beside an item (Settings > Files and lists > Show selection checkboxes). */
  const rowCheck = (e: FileEntry, index: number, tile = false) => (
    <input
      type="checkbox"
      className="row-check"
      data-tile={tile || undefined}
      aria-label={`Select ${e.name}`}
      checked={selected.has(e.name)}
      tabIndex={-1}
      // The click is handled on the row (it also takes Shift); this only shows the state.
      onChange={() => {}}
      onMouseDown={(ev) => {
        ev.stopPropagation();
        selectIndex(index, ev);
      }}
      onClick={(ev) => ev.stopPropagation()}
      onDoubleClick={(ev) => ev.stopPropagation()}
    />
  );

  // ---- Column header with drag-to-resize -------------------------------------
  const setWidths = (widths: Record<string, number>) => useStore.getState().setColumns({ ...columnSet, widths: { ...columnSet.widths, ...widths } }, "here");
  const resize = (def: ColumnDef, ev: React.PointerEvent) => {
    ev.preventDefault();
    ev.stopPropagation();
    const target = ev.currentTarget;
    target.setPointerCapture(ev.pointerId);
    const startX = ev.clientX;
    // Start from the width the column has now (Name may be narrower than it is set to).
    const start = colWidth(def.id);
    let latest = start;
    const move = (m: PointerEvent) => {
      latest = Math.max(def.min, Math.min(COLUMN_MAX_WIDTH, Math.round(start + (m.clientX - startX))));
      setLiveWidths({ [def.id]: latest });
    };
    const up = () => {
      target.removeEventListener("pointermove", move as EventListener);
      target.removeEventListener("pointerup", up);
      setLiveWidths({});
      setWidths({ [def.id]: latest });
    };
    target.addEventListener("pointermove", move as EventListener);
    target.addEventListener("pointerup", up);
  };

  /** The width a column needs to show everything in it (the longest value or the heading). */
  const fitWidth = (def: ColumnDef): number => {
    const ctx = document.createElement("canvas").getContext("2d");
    const sample = scroller.current?.querySelector(".cell") ?? scroller.current?.querySelector(".column-header");
    if (!ctx || !sample) return def.width;
    ctx.font = getComputedStyle(sample).font || "13px sans-serif";
    let widest = ctx.measureText(def.label).width + 30;
    const list = sized.length > 3000 ? sized.slice(0, 3000) : sized;
    for (const e of list) {
      const text = def.id === "name" ? e.name : cellValue(def, e, extra[e.name]);
      widest = Math.max(widest, ctx.measureText(text).width + (def.id === "name" ? 64 : 24));
    }
    return Math.max(def.min, Math.min(700, Math.ceil(widest)));
  };
  const sizeToFit = (def: ColumnDef) => setWidths({ [def.id]: fitWidth(def) });

  const sortWords = (d: ColumnDef): [string, string] =>
    d.kind === "date" ? ["Oldest first", "Newest first"] : d.kind === "size" || d.kind === "number" ? ["Smallest first", "Largest first"] : ["A to Z", "Z to A"];

  const setColFilter = (id: string, values: string[]) => {
    const cols = { ...(filter.cols ?? {}) };
    if (values.length) cols[id] = values;
    else delete cols[id];
    useStore.getState().setFilter({ cols });
  };

  // Drag a heading to another place in the row; Name stays first.
  const dragHeader = (def: ColumnDef, ev: React.PointerEvent) => {
    if (def.id === "name" || ev.button !== 0) return;
    const startX = ev.clientX;
    let moving = false;
    const dropIndex = (x: number) => {
      const cells = [...(headerRef.current?.querySelectorAll<HTMLElement>(".column-cell") ?? [])];
      let to = cells.length;
      for (let i = 1; i < cells.length; i++) {
        const r = cells[i].getBoundingClientRect();
        if (x < r.left + r.width / 2) {
          to = i;
          break;
        }
      }
      return to;
    };
    const move = (m: PointerEvent) => {
      if (!moving && Math.abs(m.clientX - startX) < 6) return;
      moving = true;
      setDragCol({ id: def.id, to: dropIndex(m.clientX) });
    };
    const up = (m: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      setDragCol(null);
      if (!moving) return;
      // The click that follows a drag is not a request to sort.
      justDragged.current = true;
      setTimeout(() => (justDragged.current = false), 0);
      const from = columnSet.order.indexOf(def.id);
      let to = dropIndex(m.clientX);
      if (to > from) to -= 1;
      useStore.getState().setColumns(moveColumnTo(columnSet, def.id, to), "here");
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const openColFilter = (def: ColumnDef, x: number, y: number) => setColFilterOpen({ id: def.id, x, y });

  // The menu of one column: what to do with it, then the choice of columns.
  const columnMenu = (def: ColumnDef | null, x: number, y: number, anchorEl?: HTMLElement) => {
    const common = ["modified", "type", "size", "created", "extension", "accessed", "dimensions", "length", "artist"];
    const here = (set: typeof columnSet) => useStore.getState().setColumns(set, "here");
    const pos = def ? columnSet.order.indexOf(def.id) : -1;
    const [asc, desc] = def ? sortWords(def) : ["", ""];
    const active = def ? (filter.cols?.[def.id]?.length ?? 0) > 0 : false;
    const own: MenuItem[] = def
      ? [
          { id: "col-sort-asc", label: `Sort: ${asc}`, checked: sort.key === def.id && sort.asc, run: () => setSort({ key: def.id, asc: true }) },
          { id: "col-sort-desc", label: `Sort: ${desc}`, checked: sort.key === def.id && !sort.asc, run: () => setSort({ key: def.id, asc: false }) },
          ...(def.id !== "name"
            ? [
                {
                  id: "col-filter",
                  label: active ? `Filter by ${def.label} (on)…` : `Filter by ${def.label}…`,
                  separatorBefore: true,
                  disabled: def.source === "disk" && noDiskDetails,
                  run: () => {
                    const r = anchorEl?.getBoundingClientRect();
                    openColFilter(def, r ? r.left : x, r ? r.bottom : y);
                  },
                },
              ]
            : []),
          ...(active ? [{ id: "col-filter-clear", label: `Clear ${def.label} filter`, run: () => setColFilter(def.id, []) }] : []),
          { id: "col-fit", label: "Size to fit", separatorBefore: true, run: () => sizeToFit(def) },
          { id: "col-reset", label: "Reset width", run: () => setWidths({ [def.id]: def.width }) },
          ...(pos > 1 ? [{ id: "col-left", label: "Move left", run: () => here(moveColumn(columnSet, def.id, -1)) }] : []),
          ...(pos >= 1 && pos < columnSet.order.length - 1 ? [{ id: "col-right", label: "Move right", run: () => here(moveColumn(columnSet, def.id, 1)) }] : []),
          ...(def.id !== "name"
            ? [
                {
                  id: "col-hide",
                  label: `Hide ${def.label}`,
                  run: () => {
                    if (active) setColFilter(def.id, []);
                    here(hideColumn(columnSet, def.id));
                  },
                },
              ]
            : []),
        ]
      : [];
    setMenu({
      x,
      y,
      items: [
        ...own,
        ...common.map((id, i) => {
          const d = columnDef(id)!;
          const on = columnSet.order.includes(id);
          return {
            id: `col-${id}`,
            label: d.label,
            checked: on,
            separatorBefore: i === 0 && own.length > 0,
            disabled: d.source === "disk" && noDiskDetails,
            run: () => {
              if (on && (filter.cols?.[id]?.length ?? 0) > 0) setColFilter(id, []);
              here(on ? hideColumn(columnSet, id) : showColumn(columnSet, id));
            },
          };
        }),
        { id: "col-more", label: "More…", separatorBefore: true, run: () => useStore.setState({ chooseDetailsOpen: true }) },
      ],
    });
  };
  const headerMenu = (x: number, y: number) => columnMenu(null, x, y);

  const header = (def: ColumnDef) => {
    const filtered = (filter.cols?.[def.id]?.length ?? 0) > 0;
    const index = columnSet.order.indexOf(def.id);
    return (
      <div
        className="column-cell"
        key={def.id}
        style={{ width: colWidth(def.id) }}
        data-dragging={dragCol?.id === def.id || undefined}
        data-drop-before={(dragCol && dragCol.to === index && dragCol.id !== def.id && dragCol.to !== columnSet.order.indexOf(dragCol.id) + 1) || undefined}
        data-drop-after={(dragCol && dragCol.to === columnSet.order.length && index === columnSet.order.length - 1 && dragCol.id !== def.id) || undefined}
        onContextMenu={(ev) => {
          ev.preventDefault();
          ev.stopPropagation();
          columnMenu(def, ev.clientX, ev.clientY, ev.currentTarget);
        }}
      >
        <button
          className="column-header"
          data-column={def.id}
          data-sorted={sort.key === def.id ? (sort.asc ? "asc" : "desc") : undefined}
          data-align={def.kind === "size" ? "end" : undefined}
          data-filtered={filtered || undefined}
          title={def.description}
          onPointerDown={(e) => dragHeader(def, e)}
          onClick={() => !justDragged.current && setSort({ key: def.id, asc: sort.key === def.id ? !sort.asc : true })}
        >
          {def.label}
        </button>
        {(def.id !== "name" || filtered) && !(def.source === "disk" && noDiskDetails) && (
          <button
            className="column-funnel"
            data-on={filtered || undefined}
            aria-label={`Filter by ${def.label}`}
            title={filtered ? `Filtered by ${def.label}` : `Filter by ${def.label}`}
            onPointerDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              const r = e.currentTarget.parentElement!.getBoundingClientRect();
              openColFilter(def, r.left, r.bottom);
            }}
          >
            <Icon name="filter" size={11} />
          </button>
        )}
        <div className="column-grip" role="separator" aria-label={`Resize ${def.label}`} onPointerDown={(e) => resize(def, e)} onDoubleClick={() => sizeToFit(def)} title="Drag to resize; double-click to fit" />
      </div>
    );
  };

  const totalSize = entries.reduce((s, e) => s + e.size, 0);
  const selectedSize = entries.filter((e) => selected.has(e.name)).reduce((s, e) => s + e.size, 0);
  const filtered = isFiltering(filter);

  return (
    <div className="file-view" data-view={details ? "details" : "grid"} data-level={level} data-selecting={(mobile && selecting) || undefined}>
      {group && (
        <SelectGroup
          mode={group}
          entries={entries}
          folder={path}
          onClose={() => setGroup(null)}
          onApply={(names) => setSelected(group === "select" ? new Set([...selected, ...names]) : new Set([...selected].filter((n) => !names.includes(n))))}
        />
      )}
      <div
        ref={scroller}
        className="file-scroller"
        data-scroll-keep="files"
        data-zoomed={zoom !== 1 || undefined}
        style={zoom !== 1 ? ({ fontSize: `calc(var(--font-size) * ${zoom})`, "--view-zoom": zoom } as React.CSSProperties) : undefined}
        data-refreshing={refreshing}
        data-drop="folder"
        data-drop-path={path}
        tabIndex={0}
        onKeyDown={onKeyDown}
        // Typing goes to the folder only while it has the focus, so a click on it must give it.
        onMouseDownCapture={() => !mobile && scroller.current?.focus({ preventScroll: true })}
        onMouseDown={mobile ? undefined : startBand}
        onContextMenu={(ev) => {
          ev.preventDefault();
          // On a phone a long press on empty space does nothing; the menu is in the bar on top.
          if (mobile) return;
          if (!checks) setSelected(new Set());
          showMenu(ev.clientX, ev.clientY, null);
        }}
      >
        {details && !mobile && (
          <div
            ref={headerRef}
            className="file-header"
            style={{ width: rowsWidth + 14 }}
            onMouseDown={(e) => e.stopPropagation()}
            onContextMenu={(ev) => {
              ev.preventDefault();
              ev.stopPropagation();
              headerMenu(ev.clientX, ev.clientY);
            }}
          >
            {checks && (
              <span className="row-check-head">
                <input
                  type="checkbox"
                  aria-label="Select all items"
                  checked={entries.length > 0 && selected.size === entries.length}
                  ref={(el) => {
                    if (el) el.indeterminate = selected.size > 0 && selected.size < entries.length;
                  }}
                  onChange={() => setSelected(selected.size === entries.length ? new Set() : new Set(entries.map((x) => x.name)))}
                />
              </span>
            )}
            {shown.map(header)}
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
            <MacAccessHelp error={error} />
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
                  if (mobile)
                    return (
                      <div key={e.name} className="file-row m-row" style={{ transform: `translateY(${row.start}px)`, height: rowHeight }} {...itemProps(e, row.index)}>
                        <span className="m-check" aria-hidden="true"><Icon name="check" size={14} /></span>
                        <span className="m-thumb"><Thumb path={fullPath(e.name)} name={e.name} isDir={e.isDir} modified={e.modified} size={40} iconSize={30} view="small" /></span>
                        <span className="m-text">
                          {nameCell(e)}
                          <span className="m-meta">
                            {shown
                              .filter((d) => d.id !== "name" && d.id !== "type")
                              .slice(0, 3)
                              .map((d) => (d.id === "size" && e.isDir ? sizeText(e) : cellValue(d, e, extra[e.name])))
                              .filter(Boolean)
                              .join("  ·  ")}
                          </span>
                        </span>
                        {e.isDir && <span className="m-chevron" aria-hidden="true"><Icon name="chevron" size={14} /></span>}
                        <button
                          className="m-more"
                          aria-label={`More actions for ${e.name}`}
                          onPointerDown={(ev) => ev.stopPropagation()}
                          onClick={(ev) => {
                            ev.stopPropagation();
                            const r = ev.currentTarget.getBoundingClientRect();
                            cursor.current = row.index;
                            showMenu(r.left, r.bottom, e);
                          }}
                        >
                          <Icon name="more" size={18} />
                        </button>
                      </div>
                    );
                  return (
                    <div
                      key={e.name}
                      className="file-row"
                      style={{ transform: `translateY(${row.start - headerHeight}px)`, height: rowHeight, width: rowsWidth }}
                      {...itemProps(e, row.index)}
                    >
                      {shown.map((d) =>
                        d.id === "name" ? (
                          <div className="cell cell-name" key="name" style={{ width: nameWidth }}>
                            {checks && rowCheck(e, row.index)}
                            <Thumb path={fullPath(e.name)} name={e.name} isDir={e.isDir} modified={e.modified} size={Math.round((baseUiIcon + 2) * zoom)} iconSize={detailsIcon} view="details" />
                            {nameCell(e)}
                          </div>
                        ) : (
                          <div className="cell" key={d.id} data-kind={d.kind} data-column={d.id} style={{ width: colWidth(d.id) }}>
                            {d.id === "size" && e.isDir ? sizeText(e) : cellValue(d, e, extra[e.name])}
                          </div>
                        ),
                      )}
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
                      style={{ width: tileW, height: tileH - 4 }}
                      title={mobile ? undefined : e.name}
                      {...itemProps(e, row.index * columnsPerRow + k)}
                    >
                      {mobile && <span className="m-check" aria-hidden="true"><Icon name="check" size={14} /></span>}
                      {checks && rowCheck(e, row.index * columnsPerRow + k, true)}
                      <Thumb path={fullPath(e.name)} name={e.name} isDir={e.isDir} modified={e.modified} size={tileLayout.icon} view={level} />
                      {nameCell(e)}
                    </div>
                  ))}
                </div>
              ),
            )}
          </div>
        )}
        {band && <div className="select-band" style={{ left: band.l, top: band.t, width: band.w, height: band.h }} />}
      </div>
      {mobile && selecting && (
        <>
          <div className="m-select-top" role="toolbar" aria-label="Selection">
            <button className="m-icon-button" aria-label="Stop selecting" onClick={() => { setSelecting(false); setSelected(new Set()); }}>
              <Icon name="close" size={20} />
            </button>
            <strong>{selected.size === 0 ? "Select items" : `${selected.size.toLocaleString()} selected`}</strong>
            {selectedSize > 0 && <span className="muted">{formatSize(selectedSize)}</span>}
            <span className="grow" />
            <button className="m-text-button" onClick={() => setSelected(selected.size === entries.length ? new Set() : new Set(entries.map((x) => x.name)))}>
              {selected.size === entries.length && entries.length > 0 ? "None" : "All"}
            </button>
          </div>
          <div className="m-actions" role="toolbar" aria-label="Actions for the selected items" data-empty={selected.size === 0 || undefined}>
            {(
              [
                ["move", "Move", "cut"],
                ["copy", "Copy", "copy"],
                ["share", "Share", "share"],
                ["delete", "Delete", "trash"],
                ["more", "More", "more"],
              ] as const
            ).map(([id, label, icon]) => (
              <button key={id} disabled={selected.size === 0} data-danger={id === "delete" || undefined} onClick={(ev) => act(id, ev)}>
                <Icon name={icon} size={22} />
                <span>{label}</span>
              </button>
            ))}
          </div>
        </>
      )}
      {typeFind && findText && (
        <div className="type-find" role="status">
          Find: <strong>{findText}</strong>
        </div>
      )}
      <div className="status-bar">
        <span>
          {entries.length.toLocaleString()} item{entries.length === 1 ? "" : "s"}
          {loading ? " so far, still loading…" : ""}
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
      {colFilterOpen && columnDef(colFilterOpen.id) && (
        <ColumnFilter
          label={columnDef(colFilterOpen.id)!.label}
          x={colFilterOpen.x}
          y={colFilterOpen.y}
          facets={columnFacets(columnDef(colFilterOpen.id)!, current?.entries ?? [], extra)}
          selected={filter.cols?.[colFilterOpen.id] ?? []}
          loading={openFilterDisk && (current?.entries ?? []).some((e) => extra[e.name] === undefined)}
          onChange={(v) => setColFilter(colFilterOpen.id, v)}
          onClose={() => setColFilterOpen(null)}
        />
      )}
      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
    </div>
  );
}
