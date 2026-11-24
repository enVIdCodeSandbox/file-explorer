// Disk usage: a treemap of where the space went, with file types, the largest
// files and folder contents beside it. Zoom into folders, isolate file types,
// hide small files, search, sort, and right-click anything for its menu.

import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useRef, useState } from "react";
import { ScanActivity } from "./ScanActivity";
import { MacAccessHelp, MacAccessNote } from "./MacAccess";
import { api, on, type Analysis, type AnalysisProgress, type SizeNode } from "../api";
import { deletePaths, openFile, showPermissions, showProperties, useStore } from "../store";
import {
  CATEGORIES,
  NO_EXT,
  categoryOf,
  extOf,
  findNode,
  hideSmall,
  isolate,
  matchesFilter,
  nodeTypes,
  sortLargest,
  withoutPaths,
  type ListSort,
} from "../util/analysis";
import { formatCount, formatDate, formatDuration, formatSize, kindOf, unitBytes } from "../util/format";
import { analyzeLocation, baseName, isInside, parentPath } from "../util/path";
import { colorFor, layoutNested, typeColor, type Tile } from "../util/treemap";
import { ContextMenu, type MenuItem, type MenuState } from "./ContextMenu";
import { FileIcon, Icon } from "./Icons";
import { sameShell, useShellItems } from "./shellMenu";

type Tab = "types" | "largest" | "folders";

const minSizes = (): [number, string][] => [
  [0, "Any size"],
  ...([1, 10, 100].map((n) => [n * unitBytes(2), `${formatSize(n * unitBytes(2))} or more`]) as [number, string][]),
  [unitBytes(3), `${formatSize(unitBytes(3))} or more`],
];

/** What the menu needs to know about the thing that was clicked. */
interface Target {
  path: string;
  name: string;
  isDir: boolean;
}

export function AnalyzerView({ path }: { path: string }) {
  const { navigate, newTab, showToast } = useStore.getState();
  const drives = useStore((s) => s.drives);
  const shellExtras = useStore((s) => s.settings!.general.shellExtras);
  const [data, setData] = useState<{ analysis: Analysis; takenAt: number } | null>(null);
  const [progress, setProgress] = useState<AnalysisProgress | null>(null);
  const [scanning, setScanning] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [focus, setFocus] = useState(path);
  const [hover, setHover] = useState<{ tile: Tile; x: number; y: number } | null>(null);
  const [removed, setRemoved] = useState<Set<string>>(new Set());
  const [size, setSize] = useState({ w: 600, h: 400 });
  const [nonce, setNonce] = useState(0);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [tab, setTab] = useState<Tab>("types");
  const [exts, setExts] = useState<ReadonlySet<string>>(new Set());
  const [minBytes, setMinBytes] = useState(0);
  const [text, setText] = useState("");
  const [notText, setNotText] = useState("");
  const [sort, setSort] = useState<{ key: ListSort; asc: boolean }>({ key: "size", asc: false });
  const canvas = useRef<HTMLCanvasElement>(null);
  const wrap = useRef<HTMLDivElement>(null);
  const menuTicket = useRef(0);
  const { fetchFor, peek } = useShellItems();

  // Show the last saved result at once, then refresh it in the background.
  useEffect(() => {
    let alive = true;
    setData(null);
    setProgress(null);
    setError(null);
    setScanning(true);
    setFocus(path);
    setRemoved(new Set());
    setExts(new Set());
    void api.analysisSnapshot(path).then((snap) => alive && snap && setData({ analysis: snap.analysis, takenAt: snap.takenAt })).catch(() => {});
    const subs = [
      on("analysis-progress", (p) => alive && p.path === path && setProgress(p)),
      // What has been measured so far. A finished earlier result stays on
      // screen instead; this only fills an empty view or replaces an older partial one.
      on("analysis-partial", (r) => {
        if (!alive || r.path !== path) return;
        setData((cur) => (cur && cur.analysis.complete ? cur : { analysis: r.analysis, takenAt: r.takenAt }));
      }),
      on("analysis-done", (r) => {
        if (!alive || r.path !== path) return;
        setData({ analysis: r.analysis, takenAt: r.takenAt });
        setScanning(false);
        setProgress(null);
      }),
      on("analysis-failed", ([p, message]) => {
        if (!alive || p !== path) return;
        setError(message);
        setScanning(false);
      }),
    ];
    api.analyzeStart(path).catch((e) => alive && (setError(String(e)), setScanning(false)));
    return () => {
      alive = false;
      subs.forEach((s) => void s.then((off) => off()));
      void api.analyzeCancel(path).catch(() => {});
    };
  }, [path, nonce]);

  useEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setSize({ w: el.clientWidth, h: el.clientHeight }));
    ro.observe(el);
    return () => ro.disconnect();
  }, [data]);

  const analysis = data?.analysis ?? null;
  const names = analysis?.extNames ?? [];

  // The tree after deletions, then the focused folder, then what the filters keep.
  const live = useMemo(() => (analysis ? withoutPaths(analysis.tree, removed) : null), [analysis, removed]);
  const focused = useMemo(() => (live ? (findNode(live, focus) ?? live) : null), [live, focus]);
  const shown = useMemo(() => {
    if (!focused) return null;
    const isolated = exts.size ? isolate(focused, exts, names) : focused;
    return isolated ? hideSmall(isolated, minBytes) : null;
  }, [focused, exts, names, minBytes]);
  const tiles = useMemo(() => (shown ? layoutNested(shown, { x: 0, y: 0, w: size.w, h: size.h }) : []), [shown, size.w, size.h]);
  const needle = text.trim().toLowerCase();
  const hoverTile = hover?.tile ?? null;

  useEffect(() => {
    const c = canvas.current;
    if (!c) return;
    const dpr = window.devicePixelRatio || 1;
    c.width = size.w * dpr;
    c.height = size.h * dpr;
    const ctx = c.getContext("2d")!;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, size.w, size.h);
    const textColor = getComputedStyle(c).getPropertyValue("--treemap-text") || "#fff";
    ctx.font = "11px system-ui, sans-serif";
    ctx.textBaseline = "top";
    const frameFill = (depth: number) => `hsl(220 9% ${17 + Math.min(depth, 6) * 2.5}%)`;
    const fits = (text: string, room: number) => {
      if (ctx.measureText(text).width <= room) return text;
      let end = text.length;
      while (end > 1 && ctx.measureText(`${text.slice(0, end)}…`).width > room) end--;
      return end > 1 ? `${text.slice(0, end)}…` : "";
    };
    for (const t of tiles) {
      const dim = needle && !t.frame && !t.node.isDir && !t.node.name.toLowerCase().includes(needle);
      ctx.globalAlpha = dim ? 0.2 : 1;
      if (t.frame) {
        ctx.fillStyle = frameFill(t.depth);
        ctx.fillRect(t.x, t.y, t.w, t.h);
        ctx.strokeStyle = "rgba(255,255,255,.14)";
        ctx.lineWidth = 1;
        ctx.strokeRect(t.x + 0.5, t.y + 0.5, t.w - 1, t.h - 1);
        if (t.header) {
          ctx.fillStyle = textColor;
          ctx.font = "600 11px system-ui, sans-serif";
          const label = fits(t.node.name, t.w - 10 - 56);
          ctx.fillText(label, t.x + 5, t.y + 4);
          ctx.font = "11px system-ui, sans-serif";
          ctx.globalAlpha = 0.65;
          ctx.textAlign = "right";
          ctx.fillText(formatSize(t.node.size), t.x + t.w - 5, t.y + 4);
          ctx.textAlign = "left";
          ctx.globalAlpha = 1;
        }
        continue;
      }
      // A file takes its type's color; a folder too small to open takes the color of what mostly fills it.
      const lead = t.node.isDir ? t.node.types[0] : undefined;
      ctx.fillStyle = t.node.isDir && lead ? typeColor(names[lead[0]] === NO_EXT ? "" : (names[lead[0]] ?? "")) : colorFor(t.node.name, t.node.isDir);
      ctx.fillRect(t.x + 0.5, t.y + 0.5, Math.max(0, t.w - 1), Math.max(0, t.h - 1));
      // A soft highlight gives the cushion look that separates neighbors.
      const g = ctx.createLinearGradient(t.x, t.y, t.x + t.w, t.y + t.h);
      g.addColorStop(0, "rgba(255,255,255,.2)");
      g.addColorStop(1, "rgba(0,0,0,.25)");
      ctx.fillStyle = g;
      ctx.fillRect(t.x + 0.5, t.y + 0.5, Math.max(0, t.w - 1), Math.max(0, t.h - 1));
      if (t.w > 56 && t.h > 28) {
        ctx.fillStyle = textColor;
        const name = fits(t.node.name, t.w - 10);
        if (name) ctx.fillText(name, t.x + 5, t.y + 5);
        if (t.h > 42) {
          ctx.globalAlpha = 0.75;
          ctx.fillText(formatSize(t.node.size), t.x + 5, t.y + 18);
        }
      }
    }
    ctx.globalAlpha = 1;
  }, [tiles, size, needle, names]);

  const tileAt = (e: React.MouseEvent): Tile | undefined => {
    const r = canvas.current!.getBoundingClientRect();
    const [x, y] = [e.clientX - r.left, e.clientY - r.top];
    // Contents are listed after their folder's frame, so the last hit is the deepest.
    for (let i = tiles.length - 1; i >= 0; i--) {
      const t = tiles[i];
      if (x >= t.x && x < t.x + t.w && y >= t.y && y < t.y + t.h) return t;
    }
    return undefined;
  };

  /** The top-level child of the focused folder that contains a tile. */
  const zoomTarget = (t: Tile): SizeNode | undefined => shown?.children.find((c) => c.isDir && c.path && isInside(t.node.path, c.path));

  const remove = async (p: string) => {
    if (await deletePaths([p])) setRemoved((r) => new Set(r).add(p));
  };

  const toggleExt = (ext: string, add: boolean) =>
    setExts((cur) => {
      const next = new Set(add ? cur : []);
      if (cur.has(ext) && (add || cur.size === 1)) next.delete(ext);
      else next.add(ext);
      if (add && cur.has(ext)) {
        const without = new Set(cur);
        without.delete(ext);
        return without;
      }
      return next;
    });

  const zoomOut = () => {
    if (!analysis) return;
    const up = parentPath(focus);
    if (up && isInside(up, path)) setFocus(up);
    else setFocus(path);
  };

  // ---- Right-click menu for a tile or a list row ---------------------------
  const menuFor = (t: Target, shell: MenuItem[] = []): MenuItem[] => {
    const inTree = t.isDir && analysis && findNode(analysis.tree, t.path);
    const ext = t.isDir ? null : extOf(t.name);
    return [
      { id: "open", label: "Open", icon: "folderOpen", run: () => (t.isDir ? navigate(t.path) : void openFile(t.path)) },
      ...(t.isDir ? [{ id: "newTab", label: "Open in new tab", run: () => newTab(t.path) }] : []),
      {
        id: "reveal",
        label: "Show in its folder",
        run: () => {
          const dir = parentPath(t.path);
          if (dir) {
            navigate(dir);
            useStore.setState({ pendingSelect: t.path });
          }
        },
      },
      ...(inTree ? [{ id: "zoom", label: "Zoom into this folder", separatorBefore: true, run: () => setFocus(t.path) }] : []),
      { id: "zoomOut", label: "Zoom out", separatorBefore: !inTree, disabled: focus === path, run: zoomOut },
      ...(ext
        ? [
            {
              id: "isolate",
              label: exts.has(ext) ? `Stop isolating .${ext}` : `Show only ${ext === NO_EXT ? "files without an extension" : `.${ext} files`}`,
              run: () => toggleExt(ext, false),
            },
          ]
        : []),
      ...(t.isDir ? [{ id: "analyze", label: "Measure this folder on its own", run: () => navigate(analyzeLocation(t.path)) }] : []),
      { id: "copyPath", label: "Copy as path", icon: "copy", separatorBefore: true, run: () => void navigator.clipboard.writeText(t.path).then(() => showToast("Path copied.")) },
      { id: "permissions", label: "Permissions", icon: "shield", run: () => showPermissions(t.path) },
      { id: "properties", label: "Properties", icon: "info", run: () => void showProperties([t.path]) },
      { id: "delete", label: "Delete", icon: "trash", danger: true, separatorBefore: true, run: () => void remove(t.path) },
      ...shell.map((m, i) => (i === 0 ? { ...m, separatorBefore: true } : m)),
    ];
  };

  /** Show the built-in menu at once; add the system's entries when they arrive. */
  const openMenu = (x: number, y: number, t: Target) => {
    const ticket = ++menuTicket.current;
    const remembered = shellExtras ? peek([t.path], false) : null;
    setMenu({ x, y, items: menuFor(t, remembered ?? undefined) });
    if (!shellExtras) return;
    void fetchFor([t.path], false).then((shell) => {
      if (ticket === menuTicket.current && shell.length && !sameShell(remembered, shell)) setMenu({ x, y, items: menuFor(t, shell) });
    });
  };

  const scan = (target: string) => navigate(analyzeLocation(target), { replace: true });
  const chooseFolder = async () => {
    try {
      const picked = await openDialog({ directory: true, multiple: false, defaultPath: path, title: "Choose a folder to measure" });
      if (typeof picked === "string") scan(picked);
    } catch (e) {
      showToast(String(e));
    }
  };
  // The drive that holds the folder being measured, for the picker.
  const currentDrive = drives.filter((d) => isInside(path, d.mount)).sort((a, b) => b.mount.length - a.mount.length)[0];

  const picker = (
    <div className="analyzer-scope">
      <label>
        <span className="muted">Measure</span>
        <select
          aria-label="Drive to measure"
          value={currentDrive && path === currentDrive.mount ? currentDrive.mount : "__folder"}
          onChange={(e) => e.target.value !== "__folder" && scan(e.target.value)}
        >
          {currentDrive && path !== currentDrive.mount && <option value="__folder">Folder: {baseName(path)}</option>}
          {drives.map((d) => (
            <option key={d.mount} value={d.mount}>
              {d.name}
            </option>
          ))}
        </select>
      </label>
      <button className="secondary" onClick={() => void chooseFolder()}>
        Choose folder…
      </button>
      <button className="secondary" disabled={scanning} onClick={() => setNonce(nonce + 1)} title="Measure again from scratch">
        Rescan
      </button>
    </div>
  );

  // The banner shows what is on screen and whether it is still being updated.
  const partial = data != null && !data.analysis.complete;
  const banner = scanning ? (
    <div className="analysis-banner" data-state={partial ? "partial" : "updating"} role="status">
      <div className="loading-bar" />
      <span>
        {partial ? "Partial result · " : data ? `Showing the scan from ${formatDate(data.takenAt * 1000)} · ` : "Measuring · "}
        <strong>updating</strong>
        {progress ? ` — ${formatCount(progress.entries)} items, ${formatSize(progress.bytes)} found so far (${formatDuration(progress.elapsedSecs)})` : "…"}
        {partial && (
          <span className="partial-note">
            {" "}
            This does not reflect all available files. Wait until it finishes indexing. It is saved as it goes and refreshes every so often.
          </span>
        )}
      </span>
      {progress?.activity && <ScanActivity path={path} progress={progress} />}
    </div>
  ) : partial ? (
    <div className="analysis-banner" data-state="partial" role="status">
      <span>
        Partial result from an earlier scan that did not finish · <span className="partial-note">This does not reflect all available files.</span>
      </span>
    </div>
  ) : data ? (
    <div className="analysis-banner" data-state="current" role="status">
      <span>Up to date · measured {formatDate(data.takenAt * 1000)}</span>
    </div>
  ) : null;

  if (error && !data) {
    return (
      <div className="page">
        {picker}
        <div className="empty-state" data-error>{error}<MacAccessHelp error={error} /></div>
      </div>
    );
  }
  if (!analysis || !focused) {
    return (
      <div className="page">
        {picker}
        {banner}
        <p className="muted">
          Measuring {path}…
          {progress ? ` ${formatCount(progress.entries)} items so far` : ""}
        </p>
      </div>
    );
  }

  // ---- Side lists ----------------------------------------------------------
  const filter = { text, minBytes, exts, notText };
  const trail: string[] = [];
  for (let p: string | null = focus; p && p.length >= path.length; p = parentPath(p)) trail.unshift(p);

  const typeRows = nodeTypes(focused, names);
  const typeTotal = Math.max(1, focused.size);
  const categories = CATEGORIES.map((c) => ({ c, rows: typeRows.filter((r) => categoryOf(r.ext) === c) })).filter((g) => g.rows.length);

  const largest = sortLargest(
    analysis.largest.filter((f) => !removed.has(f.path) && isInside(f.path, focus) && matchesFilter(f.name, f.size, filter)),
    sort.key,
    sort.asc,
  );
  const contents = [...focused.children]
    .filter((c) => c.name !== "(other)" && matchesFilter(c.name, c.size, { ...filter, exts: new Set<string>() }))
    .sort((a, b) => {
      const d = sort.asc ? 1 : -1;
      if (sort.key === "name") return d * a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" });
      if (sort.key === "type") return d * (Number(b.isDir) - Number(a.isDir) || extOf(a.name).localeCompare(extOf(b.name)));
      return d * (a.size - b.size);
    });

  const sortControl = (
    <div className="analyzer-sort">
      <select aria-label="Sort by" value={sort.key} onChange={(e) => setSort({ ...sort, key: e.target.value as ListSort })}>
        <option value="size">Size</option>
        <option value="name">Name</option>
        <option value="type">Type</option>
        {tab === "largest" && <option value="modified">Date modified</option>}
      </select>
      <button className="icon-button" aria-label={sort.asc ? "Ascending" : "Descending"} title={sort.asc ? "Smallest first" : "Largest first"} onClick={() => setSort({ ...sort, asc: !sort.asc })}>
        <Icon name="sort" size={14} />
      </button>
    </div>
  );

  return (
    <div
      className="page analyzer-view"
      tabIndex={-1}
      onKeyDown={(e) => {
        if ((e.target as HTMLElement).tagName === "INPUT") return;
        if (e.key === "Backspace") zoomOut();
        else if (e.key === "Escape") setExts(new Set());
      }}
    >
      <MacAccessNote />
      <div className="section-head">
        <div className="breadcrumbs">
          {trail.map((p, i) => (
            <span key={p} className="crumb-wrap">
              <button className="crumb" onClick={() => setFocus(p)}>
                {i === 0 ? p : baseName(p)}
              </button>
              {i < trail.length - 1 && <Icon name="chevron" size={10} />}
            </span>
          ))}
        </div>
        <span className="muted">
          {formatSize(shown?.size ?? 0)}
          {exts.size || minBytes ? ` of ${formatSize(focused.size)}` : ""} · {formatCount(analysis.fileCount)} files{analysis.complete ? "" : " (partial)"}
        </span>
      </div>
      {picker}
      {banner}
      <div className="analyzer-filters">
        <button className="secondary" disabled={focus === path} onClick={zoomOut} title="Zoom out one level (Backspace)">
          <Icon name="up" size={14} /> Zoom out
        </button>
        <button className="secondary" disabled={focus === path} onClick={() => setFocus(path)}>
          Whole view
        </button>
        <div className="search-box analyzer-search">
          <Icon name="search" size={14} />
          <input value={text} placeholder="Find a name" aria-label="Find a name" onChange={(e) => setText(e.target.value)} />
        </div>
        <div className="search-box analyzer-search">
          <Icon name="filter" size={14} />
          <input value={notText} placeholder="Name is not…" aria-label="Name does not contain" onChange={(e) => setNotText(e.target.value)} />
        </div>
        <select aria-label="Hide small files" value={minBytes} onChange={(e) => setMinBytes(Number(e.target.value))}>
          {minSizes().map(([v, label]) => (
            <option key={v} value={v}>
              {label}
            </option>
          ))}
        </select>
        {[...exts].map((x) => (
          <button key={x} className="chip" data-on onClick={() => toggleExt(x, true)} title="Stop isolating this type">
            {x === NO_EXT ? x : `.${x}`} ✕
          </button>
        ))}
        {exts.size > 0 && (
          <button className="link-button" onClick={() => setExts(new Set())}>
            Show all types
          </button>
        )}
      </div>
      <div className="analyzer-body">
        <div ref={wrap} className="treemap-wrap">
          <canvas
            ref={canvas}
            className="treemap"
            style={{ width: size.w, height: size.h }}
            onMouseMove={(e) => {
              const t = tileAt(e);
              setHover(t ? { tile: t, x: e.clientX, y: e.clientY } : null);
            }}
            onMouseLeave={() => setHover(null)}
            onClick={(e) => {
              const t = tileAt(e);
              const target = t && zoomTarget(t);
              if (target) setFocus(target.path);
            }}
            onDoubleClick={(e) => {
              const t = tileAt(e);
              const dir = t?.node.path ? (t.node.isDir ? t.node.path : parentPath(t.node.path)) : null;
              if (dir) navigate(dir);
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              const t = tileAt(e);
              if (t?.node.path) openMenu(e.clientX, e.clientY, { path: t.node.path, name: t.node.name, isDir: t.node.isDir });
            }}
          />
          {hoverTile && <div className="treemap-hover" style={{ left: hoverTile.x, top: hoverTile.y, width: hoverTile.w, height: hoverTile.h }} />}
          {shown && shown.size === 0 && <div className="treemap-empty muted">Nothing matches these filters.</div>}
          {hover && (
            <div className="tooltip" style={{ left: hover.x + 14, top: hover.y + 14 }}>
              <strong>{hover.tile.node.name}</strong>
              <span>{formatSize(hover.tile.node.size)} · {((hover.tile.node.size / Math.max(1, shown?.size ?? 1)) * 100).toFixed(1)}%</span>
              {hover.tile.node.isDir &&
                nodeTypes(hover.tile.node, names)
                  .slice(0, 3)
                  .map((t) => (
                    <span key={t.ext} className="muted">
                      {t.ext === NO_EXT ? t.ext : `.${t.ext}`} · {formatSize(t.bytes)}
                    </span>
                  ))}
              <span className="muted">{hover.tile.node.path}</span>
              <span className="muted">Click to zoom · double-click to open the folder · right-click for actions</span>
            </div>
          )}
        </div>
        <aside className="largest-list">
          <div className="segmented analyzer-tabs" role="tablist">
            {(
              [
                ["types", "File types"],
                ["largest", "Largest files"],
                ["folders", "Contents"],
              ] as [Tab, string][]
            ).map(([id, label]) => (
              <button key={id} role="tab" aria-selected={tab === id} data-on={tab === id || undefined} onClick={() => setTab(id)}>
                {label}
              </button>
            ))}
          </div>

          {tab === "types" && (
            <>
              <div className="chips analyzer-categories">
                {categories.map(({ c, rows }) => (
                  <button
                    key={c}
                    className="chip"
                    data-on={rows.every((r) => exts.has(r.ext)) || undefined}
                    title={`Show only ${c.toLowerCase()}: ${formatSize(rows.reduce((s, r) => s + r.bytes, 0))}`}
                    onClick={() => setExts(new Set(rows.map((r) => r.ext)))}
                  >
                    {c}
                  </button>
                ))}
              </div>
              {typeRows.length === 0 && <p className="muted">No file types recorded for this view.</p>}
              {typeRows.map((r) => (
                <button
                  key={r.ext}
                  className="type-line"
                  data-on={exts.has(r.ext) || undefined}
                  title="Click to show only this type. Ctrl-click adds more."
                  onClick={(e) => toggleExt(r.ext, e.ctrlKey || e.metaKey || e.shiftKey)}
                >
                  <span className="type-swatch" style={{ background: typeColor(r.ext === NO_EXT ? "" : r.ext) }} />
                  <span className="grow file-name">{r.ext === NO_EXT ? r.ext : `.${r.ext}`}</span>
                  <span className="muted">{((r.bytes / typeTotal) * 100).toFixed(1)}%</span>
                  <strong>{formatSize(r.bytes)}</strong>
                </button>
              ))}
            </>
          )}

          {tab !== "types" && (
            <div className="analyzer-list-head">
              <span className="muted grow">{tab === "largest" ? `${formatCount(largest.length)} files` : `${formatCount(contents.length)} items`}</span>
              {sortControl}
            </div>
          )}

          {tab === "largest" && (
            <>
              {largest.length === 0 && <p className="muted">No files here match.</p>}
              {largest.slice(0, 200).map((f) => (
                <div
                  key={f.path}
                  className="largest-row"
                  title={f.path}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    openMenu(e.clientX, e.clientY, { path: f.path, name: f.name, isDir: false });
                  }}
                >
                  <FileIcon kind={kindOf(f.name, false)} name={f.name} />
                  <div className="grow">
                    <div className="file-name">{f.name}</div>
                    <div className="muted path">{parentPath(f.path)}</div>
                  </div>
                  <strong>{formatSize(f.size)}</strong>
                  <button
                    className="icon-button"
                    aria-label="Open containing folder"
                    title="Open containing folder"
                    onClick={() => {
                      const d = parentPath(f.path);
                      if (d) navigate(d);
                    }}
                  >
                    <Icon name="folderOpen" />
                  </button>
                  <button className="icon-button danger" aria-label={`Delete ${f.name}`} title="Delete" onClick={() => void remove(f.path)}>
                    <Icon name="trash" />
                  </button>
                </div>
              ))}
            </>
          )}

          {tab === "folders" && (
            <>
              {contents.length === 0 && <p className="muted">Nothing here matches.</p>}
              {contents.map((c) => (
                <button
                  key={c.path || c.name}
                  className="contents-row"
                  title={c.path}
                  onClick={() => c.isDir && c.path && setFocus(c.path)}
                  onDoubleClick={() => c.path && (c.isDir ? navigate(c.path) : void openFile(c.path))}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    if (c.path) openMenu(e.clientX, e.clientY, { path: c.path, name: c.name, isDir: c.isDir });
                  }}
                >
                  <FileIcon kind={kindOf(c.name, c.isDir)} name={c.name} />
                  <div className="grow">
                    <div className="file-name">{c.name}</div>
                    <div className="meter">
                      <div className="meter-fill" style={{ width: `${Math.min(100, (c.size / typeTotal) * 100)}%`, background: colorFor(c.name, c.isDir) }} />
                    </div>
                  </div>
                  <span className="muted">{((c.size / typeTotal) * 100).toFixed(1)}%</span>
                  <strong>{formatSize(c.size)}</strong>
                </button>
              ))}
            </>
          )}
        </aside>
      </div>
      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
    </div>
  );
}
