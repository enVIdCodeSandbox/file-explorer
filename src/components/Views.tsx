// Non-folder pages: Home, This PC, search results and disk usage.

import { useVirtualizer } from "@tanstack/react-virtual";
import { useSelectionBand } from "./useSelectionBand";
import { useEffect, useMemo, useRef, useState } from "react";
import { mergeHits } from "../util/searchHits";
import { levelHits } from "../util/levelSearch";
import { api, on, type Device, type Drive, type Hit, type SearchResponse } from "../api";
import { ask, compressPaths, deletePaths, extractArchive, isArchiveName, openFile, promptText, renamePath, thisPcLabel, useStore } from "../store";
import { useShellItems } from "./shellMenu";
import { driveLetter, driveName, driveRemote, hidesAddress, withoutLetter } from "../util/drives";
import { iconMenuItems } from "./IconPicker";
import { driveSlot } from "../util/customIcons";
import { deviceSlot, deviceTarget, driveTarget } from "../util/iconTarget";
import { KIND_LABEL, deviceIcon, deviceStatus, isMainDevice, openDevice } from "../util/devices";
import { applyFilter, isFiltering } from "../util/filter";
import { adv } from "../settings/advanced";
import { beginDrag, type DragPayload } from "../dnd";
import type { FileEntry } from "../api";
import { extensionOf, typeLabel } from "../util/format";
import { rankHits } from "../util/searchHits";
import type { SortKey } from "../store";
import { formatCount, formatDate, formatSize, kindOf } from "../util/format";
import { analyzeLocation, baseName, isRemote, maskAddresses, parentPath } from "../util/path";
import { ContextMenu, type MenuItem, type MenuState } from "./ContextMenu";
import { driveMenuItems, networkMenuItems } from "./driveActions";
import { FileIcon, Icon } from "./Icons";

// ---------------------------------------------------------------------------
// Home: pins and recents. Nothing appears here that the user did not pin or open.
// ---------------------------------------------------------------------------

export function HomeView() {
  const pins = useStore((s) => s.pins);
  const recent = useStore((s) => s.recent);
  const { navigate, addRecent, clearRecent } = useStore.getState();

  const openPath = (path: string, isDir: boolean) => {
    if (isDir) navigate(path);
    else {
      addRecent(path, false);
      void openFile(path);
    }
  };

  return (
    <div className="page home-view">
      <h2>Pinned</h2>
      {pins.length === 0 ? (
        <p className="muted">Nothing pinned yet. Drag a folder onto Pinned in the sidebar, or right-click it and choose "Pin to sidebar".</p>
      ) : (
        <div className="card-grid">
          {pins.map((p) => (
            <button key={p.path} className="card pin-card" title={p.path} onClick={() => openPath(p.path, !p.isFile)}>
              <FileIcon kind={p.isFile ? kindOf(p.label, false) : "folder"} name={p.isFile ? p.label : undefined} size={32} />
              <span>{p.label}</span>
            </button>
          ))}
        </div>
      )}

      <div className="section-head">
        <h2>Recent</h2>
        {recent.length > 0 && (
          <button className="link-button" onClick={clearRecent}>
            Clear
          </button>
        )}
      </div>
      {recent.length === 0 ? (
        <p className="muted">Folders and files you open will be listed here.</p>
      ) : (
        <div className="simple-list">
          {recent.slice(0, 40).map((r) => (
            <button key={r.path} className="simple-row" onClick={() => openPath(r.path, r.isDir)} title={r.path}>
              <FileIcon kind={kindOf(r.path, r.isDir)} name={baseName(r.path)} />
              <span className="grow">{baseName(r.path)}</span>
              <span className="muted path">{parentPath(r.path) ?? ""}</span>
              <span className="muted date">{formatDate(r.at)}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// This PC
// ---------------------------------------------------------------------------

const MEDIA_LABEL: Record<Drive["media"], string> = {
  ssd: "SSD",
  hdd: "Hard disk",
  network: "Network share",
  removable: "Removable",
  unknown: "Disk",
};

function Donut({ fraction }: { fraction: number }) {
  const r = 26;
  const c = 2 * Math.PI * r;
  return (
    <svg className="donut" width="68" height="68" viewBox="0 0 68 68" role="img" aria-label={`${Math.round(fraction * 100)}% used`}>
      <circle cx="34" cy="34" r={r} className="donut-track" />
      <circle
        cx="34"
        cy="34"
        r={r}
        className="donut-value"
        strokeDasharray={`${c * fraction} ${c}`}
        transform="rotate(-90 34 34)"
      />
      <text x="34" y="38" textAnchor="middle">
        {Math.round(fraction * 100)}%
      </text>
    </svg>
  );
}

export function DrivesView() {
  const drives = useStore((s) => s.drives);
  const fresh = useStore((s) => s.drivesFresh);
  const devices = useStore((s) => s.devices);
  const os = useStore((s) => s.info?.os);
  const appearance = useStore((s) => s.settings!.appearance);
  const { navigate, updateSettings } = useStore.getState();
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [folded, setFolded] = useState<Record<string, boolean>>({});
  const showSpace = appearance.showDriveSpace;

  // Local names are remembered with the system's name at the time. If the
  // drive is later renamed in the system, that newer name is shown instead.
  const setAlias = (d: Drive, patch: { name?: string; hideAddress?: boolean | null }) =>
    updateSettings((s) => {
      const cur = s.appearance.driveNames?.[d.mount] ?? { name: "", base: d.name, hideAddress: null };
      const next = { ...cur, ...patch };
      if (patch.name !== undefined) next.base = d.name;
      const names = { ...(s.appearance.driveNames ?? {}) };
      if (!next.name && next.hideAddress == null) delete names[d.mount];
      else names[d.mount] = next;
      s.appearance.driveNames = names;
    });

  const renameDrive = async (d: Drive) => {
    const r = await ask({
      title: "Rename drive",
      message: `This changes the name shown in this app only. If you rename the drive in the system later, that name is used instead. Leave empty to use the system's name.${
        driveLetter(d) ? ` The drive letter (${driveLetter(d)}) is always shown and cannot be changed.` : ""
      }`,
      input: withoutLetter(driveName(d, appearance)),
      buttons: [
        { id: "ok", label: "Rename", primary: true },
        { id: "cancel", label: "Cancel" },
      ],
    });
    if (r?.button !== "ok") return;
    // A typed letter is ignored: only the name before it can change.
    const name = withoutLetter(r.input.trim());
    setAlias(d, { name: name === withoutLetter(d.name) ? "" : name });
  };

  const deviceGroups: [string, Device[]][] = [
    ["Phones and cameras", devices.filter(isMainDevice)],
    ["Other devices", devices.filter((d) => !isMainDevice(d))],
  ];

  const groups: [string, Drive[]][] = [
    ["Devices and drives", drives.filter((d) => d.media !== "network")],
    ["Network locations", drives.filter((d) => d.media === "network")],
  ];

  return (
    <div
      className="page drives-view"
      onContextMenu={(e) => {
        e.preventDefault();
        setMenu({ x: e.clientX, y: e.clientY, items: networkMenuItems(drives, os, appearance) });
      }}
    >
      <div className="section-head">
        <h2>{thisPcLabel(os)}</h2>
        <label className="inline-toggle" title="Show how much space each drive has">
          <input
            type="checkbox"
            checked={showSpace}
            onChange={(e) => updateSettings((s) => void (s.appearance.showDriveSpace = e.target.checked))}
          />
          Show space
        </label>
        <div className="segmented" role="group" aria-label="Drive display">
          {(["bar", "donut"] as const).map((c) => (
            <button
              key={c}
              data-on={appearance.driveChart === c || undefined}
              onClick={() => updateSettings((s) => void (s.appearance.driveChart = c))}
            >
              {c === "bar" ? "Bars" : "Rings"}
            </button>
          ))}
          <span className="segmented-gap" />
          {(["used", "free"] as const).map((m) => (
            <button
              key={m}
              disabled={!showSpace}
              data-on={appearance.driveMetric === m || undefined}
              onClick={() => updateSettings((s) => void (s.appearance.driveMetric = m))}
            >
              {m === "used" ? "Used" : "Free"}
            </button>
          ))}
        </div>
      </div>
      {!fresh && drives.length > 0 && <div className="loading-bar" title="Refreshing drive information" />}
      {groups.map(
        ([title, list]) =>
          list.length > 0 && (
            <section key={title}>
              <h3>
                <button className="section-fold" aria-expanded={!folded[title]} onClick={() => setFolded({ ...folded, [title]: !folded[title] })}>
                  <Icon name={folded[title] ? "chevron" : "expand"} size={10} /> {title} <span className="muted">({list.length})</span>
                </button>
              </h3>
              {!folded[title] && <div className="card-grid drives">
                {list.map((d) => {
                  const used = d.total - d.available;
                  const usedFraction = d.total > 0 ? used / d.total : 0;
                  const shown = appearance.driveMetric === "used" ? usedFraction : 1 - usedFraction;
                  const text = !d.online
                    ? d.removable ? "No media" : "Not reachable"
                    : appearance.driveMetric === "used"
                      ? `${formatSize(used)} used of ${formatSize(d.total)}`
                      : `${formatSize(d.available)} free of ${formatSize(d.total)}`;
                  return (
                    <button
                      key={d.mount}
                      className="card drive-card"
                      data-offline={!d.online || undefined}
                      data-nearly-full={usedFraction > 0.9 || undefined}
                      data-chart={appearance.driveChart}
                      onDoubleClick={() => navigate(d.mount)}
                      onKeyDown={(e) => e.key === "F2" && void renameDrive(d)}
                      onContextMenu={(e) => {
                        e.preventDefault();
                        e.stopPropagation();
                        const alias = appearance.driveNames?.[d.mount];
                        setMenu({
                          x: e.clientX,
                          y: e.clientY,
                          items: [
                            { id: "open", label: "Open", run: () => navigate(d.mount) },
                            ...driveMenuItems(d, { os, drives, appearance }),
                            { id: "rename", label: "Rename in this app…", icon: "rename", shortcut: "F2", separatorBefore: true, run: () => void renameDrive(d) },
                            ...(alias?.name && alias.base === d.name ? [{ id: "resetName", label: "Use the system's name", run: () => setAlias(d, { name: "" }) }] : []),
                            ...iconMenuItems(driveTarget(d.mount, driveName(d, appearance), d.media)).map((m, i) => (i === 0 ? { ...m, separatorBefore: true } : m)),
                          ],
                        });
                      }}
                      title={`${hidesAddress(d, appearance) ? maskAddresses(d.mount, true) : d.mount}\nDouble-click to open`}
                    >
                      {showSpace && appearance.driveChart === "donut" && d.online ? (
                        <Donut fraction={shown} />
                      ) : (
                        <Icon name={d.media === "network" ? "network" : "drive"} slot={driveSlot(d.mount)} size={34} />
                      )}
                      <div className="drive-info">
                        <strong>{driveName(d, appearance)}</strong>
                        {showSpace && appearance.driveChart === "bar" && d.online && (
                          <div className="meter" role="img" aria-label={text}>
                            <div className="meter-fill" style={{ width: `${shown * 100}%` }} />
                          </div>
                        )}
                        {showSpace && <span className="muted drive-space">{text}</span>}
                        {driveRemote(d, appearance) && <span className="muted path">{d.remote}</span>}
                        <span className="muted">
                          {MEDIA_LABEL[d.media]}
                          {d.fs ? ` · ${d.fs.toUpperCase()}` : ""}
                          {d.readOnly ? " · Read-only" : ""}
                        </span>
                      </div>
                    </button>
                  );
                })}
              </div>}
            </section>
          ),
      )}
      {deviceGroups.map(
        ([title, list]) =>
          list.length > 0 && (
            <section key={title}>
              <h3>
                <button className="section-fold" aria-expanded={!folded[title]} onClick={() => setFolded({ ...folded, [title]: !folded[title] })}>
                  <Icon name={folded[title] ? "chevron" : "expand"} size={10} /> {title} <span className="muted">({list.length})</span>
                </button>
              </h3>
              {!folded[title] && (
                <div className="card-grid drives">
                  {list.map((d) => (
                    <button
                      key={d.id}
                      className="card drive-card"
                      onDoubleClick={() => void openDevice(d)}
                      onKeyDown={(e) => e.key === "Enter" && void openDevice(d)}
                      onContextMenu={(e) => {
                        e.preventDefault();
                        e.stopPropagation();
                        setMenu({
                          x: e.clientX,
                          y: e.clientY,
                          items: [
                            { id: "open", label: d.path ? "Open" : d.connectable ? "Connect and open" : d.open ? "Open in the system's file tool" : "Open", run: () => void openDevice(d), disabled: !d.path && !d.connectable && !d.open },
                            ...iconMenuItems(deviceTarget(d.id, d.name, deviceIcon(d))).map((m, i) => (i === 0 ? { ...m, separatorBefore: true } : m)),
                          ],
                        });
                      }}
                      title={`${d.name}\n${d.note || deviceStatus(d)}\nDouble-click to open`}
                    >
                      <Icon name={deviceIcon(d)} slot={deviceSlot(d.id)} size={34} />
                      <div className="drive-info">
                        <strong>{d.name}</strong>
                        <span className="muted">{deviceStatus(d)}</span>
                        {d.note && <span className="muted drive-note">{d.note}</span>}
                        <span className="muted">
                          {KIND_LABEL[d.kind]}
                          {d.maker && !d.name.toLowerCase().includes(d.maker.toLowerCase()) ? ` · ${d.maker}` : ""}
                        </span>
                      </div>
                    </button>
                  ))}
                </div>
              )}
            </section>
          ),
      )}
      {drives.length === 0 && devices.length === 0 && <p className="muted">{fresh ? "No drives found." : "Looking for drives…"}</p>}
      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Search results
// ---------------------------------------------------------------------------

let searchSeq = 0;

type HitSortKey = "relevance" | SortKey;

const HIT_SORTS: [HitSortKey, string][] = [
  ["relevance", "Best match"],
  ["name", "Name"],
  ["modified", "Date modified"],
  ["size", "Size"],
  ["type", "Type"],
  ["extension", "Extension"],
];

/** A hit as the folder filter and sorter understand an entry. */
function asEntry(h: Hit): FileEntry {
  return { name: h.name, isDir: h.isDir, isSymlink: false, isHidden: false, size: h.size, modified: h.modified, created: null };
}

const collate = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

function sortHits(list: Hit[], key: HitSortKey, asc: boolean, foldersFirst: boolean): Hit[] {
  if (key === "relevance") return asc ? list : [...list].reverse();
  const dir = asc ? 1 : -1;
  return [...list].sort((a, b) => {
    if (foldersFirst && a.isDir !== b.isDir) return a.isDir ? -1 : 1;
    let r = 0;
    if (key === "size") r = a.size - b.size;
    else if (key === "modified") r = (a.modified ?? 0) - (b.modified ?? 0);
    else if (key === "type") r = collate.compare(typeLabel(a.name, a.isDir), typeLabel(b.name, b.isDir));
    else if (key === "extension") r = collate.compare(extensionOf(a.name), extensionOf(b.name));
    return (r || collate.compare(a.name, b.name) || rankHits(a, b)) * dir;
  });
}

export function SearchView({ query, scope, mode = "tree" }: { query: string; scope: string | null; mode?: "level" | "tree" | "global" }) {
  const settings = useStore((s) => s.settings)!;
  const indexStatus = useStore((s) => s.indexStatus);
  const clipboard = useStore((s) => s.clipboard);
  const filter = useStore((s) => s.filter);
  const { navigate, newTab, addRecent, showToast, setClipboard, pasteInto, setFilter } = useStore.getState();
  const [result, setResult] = useState<SearchResponse | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [sort, setSort] = useState<{ key: HitSortKey; asc: boolean }>({ key: "relevance", asc: true });
  const [menu, setMenu] = useState<MenuState | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const anchor = useRef(0);
  const cursor = useRef(0);
  const collapseOnClick = useRef<string | null>(null);

  // Results arrive in two parts: what the index knew at once, then batches
  // streamed from folders it does not cover yet. Each search has its own id,
  // so late batches from an older query are ignored.
  const [scanned, setScanned] = useState(0);
  useEffect(() => {
    let alive = true;
    const id = ++searchSeq;
    const limit = settings.search.limit;
    setBusy(true);
    setError(null);
    setScanned(0);
    setSelected(new Set());
    const sub = on("search-hits", (b) => {
      if (!alive || b.id !== id) return;
      setResult((cur) => (cur ? { ...cur, hits: mergeHits(cur.hits, b.hits, limit), searching: !b.done } : cur));
      setScanned(b.scanned);
      if (b.done) setBusy(false);
    });
    // One level only: the folder's own listing, matched by name. Instant, and nothing deeper is read.
    if (mode === "level" && scope) {
      void api
        .listDir(scope)
        .then((l) => {
          if (!alive) return;
          const t0 = performance.now();
          const hits = levelHits(scope, l.entries, query);
          setResult({ hits, source: "live", complete: true, elapsedMs: performance.now() - t0, searchedEntries: l.entries.length, searching: false });
          setBusy(false);
        })
        .catch((e) => alive && (setError(String(e)), setBusy(false)));
      return () => {
        alive = false;
        void sub.then((off) => off());
      };
    }
    // Subscribe before asking, so no early batch can be missed.
    void sub
      .then(() => api.search(query, scope, id))
      .then((r) => {
        if (!alive) return;
        // A batch can beat the response here: keep anything already merged.
        setResult((cur) => (cur && cur.hits.length ? { ...r, hits: mergeHits(r.hits, cur.hits, limit), searching: r.searching && cur.searching !== false } : r));
        if (!r.searching) setBusy(false);
      })
      .catch((e) => {
        // "superseded" means a newer query replaced this one.
        if (alive && String(e) !== "superseded") (setError(String(e)), setBusy(false));
      });
    return () => {
      alive = false;
      void sub.then((off) => off());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, scope, mode]);

  // Filter with the same panel as a folder (Filter in the command bar) or the
  // strip below, then sort. Entries are matched back to hits by identity, so two
  // files with the same name in different folders stay separate.
  const all = result?.hits ?? [];
  const hits = useMemo(() => {
    const entries = all.map((h) => ({ h, e: asEntry(h) }));
    const byEntry = new Map(entries.map((x) => [x.e, x.h]));
    const kept = applyFilter(entries.map((x) => x.e), filter).map((e) => byEntry.get(e)!);
    return sortHits(kept, sort.key, sort.asc, sort.key !== "relevance" && settings.general.foldersFirst !== false);
  }, [all, filter, sort, settings.general]);

  // What can be chosen to narrow by, from what was found.
  const kindFacets = useMemo(() => {
    const counts = new Map<string, number>();
    for (const h of all) counts.set(kindOf(h.name, h.isDir), (counts.get(kindOf(h.name, h.isDir)) ?? 0) + 1);
    return [...counts.entries()].sort((a, b) => b[1] - a[1]).slice(0, 12);
  }, [all]);
  const narrowed = isFiltering(filter);

  // Keep the selection to what is still listed.
  useEffect(() => {
    setSelected((cur) => {
      if (!cur.size) return cur;
      const live = new Set(hits.map((h) => h.path));
      const next = new Set([...cur].filter((p) => live.has(p)));
      return next.size === cur.size ? cur : next;
    });
  }, [hits]);

  const rowHeight = settings.appearance.density === "compact" ? 26 : 34;
  const virtualizer = useVirtualizer({
    count: hits.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => rowHeight,
    overscan: 12,
  });

  // Drag over empty space to select what the box touches (see useSelectionBand).
  const { band, startBand } = useSelectionBand({
    scroller,
    geometry: { count: hits.length, details: true, rowHeight, headerHeight: 28, rowsWidth: 1e9, columnsPerRow: 1, tileW: 0, tileH: 0 },
    keys: useMemo(() => hits.map((h) => h.path), [hits]),
    selected,
    commit: setSelected,
    onLast: (i) => {
      cursor.current = i;
      anchor.current = i;
    },
  });

  // Opening a result keeps the results: folders go to a new tab by default.
  const open = (h: Hit) => {
    if (h.isDir) {
      if (settings.general.openSearchResultInNewTab) newTab(h.path);
      else navigate(h.path);
    } else {
      addRecent(h.path, false);
      void openFile(h.path);
    }
  };
  const reveal = (h: Hit) => {
    const dir = parentPath(h.path);
    if (!dir) return;
    if (settings.general.openSearchResultInNewTab) newTab(dir);
    else navigate(dir);
  };

  const chosen = () => hits.filter((h) => selected.has(h.path));
  const pasteTarget = (): string | null => {
    const picked = chosen();
    if (picked.length === 1 && picked[0].isDir) return picked[0].path;
    return scope;
  };
  const paste = () => {
    const dest = pasteTarget();
    if (dest) void pasteInto(dest);
    else showToast("Select a folder in the results to paste into.");
  };

  // ---- Selection: the same rules as a folder -------------------------------
  const select = (i: number, e: { ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }) => {
    const h = hits[i];
    if (!h) return;
    cursor.current = i;
    if (e.shiftKey) {
      const [lo, hi] = [Math.min(anchor.current, i), Math.max(anchor.current, i)];
      const next = e.ctrlKey || e.metaKey ? new Set(selected) : new Set<string>();
      for (const x of hits.slice(lo, hi + 1)) next.add(x.path);
      setSelected(next);
    } else if (e.ctrlKey || e.metaKey) {
      const next = new Set(selected);
      next.has(h.path) ? next.delete(h.path) : next.add(h.path);
      anchor.current = i;
      setSelected(next);
    } else {
      anchor.current = i;
      setSelected(new Set([h.path]));
    }
  };
  const move = (delta: number, shift: boolean) => {
    const i = Math.max(0, Math.min(hits.length - 1, cursor.current + delta));
    select(i, { ctrlKey: false, metaKey: false, shiftKey: shift });
    virtualizer.scrollToIndex(i);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const mod = e.ctrlKey || e.metaKey;
    const picked = chosen();
    const paths = picked.map((h) => h.path);
    let handled = true;
    if (e.key === "ArrowDown") move(1, e.shiftKey);
    else if (e.key === "ArrowUp") move(-1, e.shiftKey);
    else if (e.key === "Home") move(-hits.length, e.shiftKey);
    else if (e.key === "End") move(hits.length, e.shiftKey);
    else if (e.key === "PageDown") move(15, e.shiftKey);
    else if (e.key === "PageUp") move(-15, e.shiftKey);
    else if (e.key === "Enter" && picked.length) picked.forEach(open);
    else if (e.key === "Delete" && paths.length) void deletePaths(paths, e.shiftKey);
    else if (e.key === "Escape") setSelected(new Set());
    else if (mod && e.key.toLowerCase() === "a") setSelected(new Set(hits.map((h) => h.path)));
    else if (mod && e.key.toLowerCase() === "c" && paths.length) setClipboard({ paths, op: "copy" });
    else if (mod && e.key.toLowerCase() === "x" && paths.length) setClipboard({ paths, op: "move" });
    else if (mod && e.key.toLowerCase() === "v") paste();
    else handled = false;
    if (handled) {
      e.preventDefault();
      e.stopPropagation();
    }
  };

  // ---- Right-click ----------------------------------------------------------
  // The same entries as the menu in a folder (pin, share, send to, compress, rename, terminal,
  // custom actions, the system's own entries...), plus the ones that only make sense here.
  const menuTicket = useRef(0);
  const { fetchFor } = useShellItems();
  const buildMenu = (picked: Hit[], shell: MenuItem[] = []): MenuItem[] => {
    const paths = picked.map((p) => p.path);
    const one = picked.length === 1 ? picked[0] : null;
    const dest = one?.isDir ? one.path : scope;
    const { pins, places, drives, settings: cfg } = useStore.getState();
    const pinned = one ? pins.some((x) => x.path === one.path) : false;
    const startTransfer = useStore.getState().startTransfer;
    const here = new Set(paths.map((x) => parentPath(x)));
    const sendTo: MenuItem[] = [
      ...places.filter((x) => ["Desktop", "Documents", "Downloads"].includes(x.label)).map((x) => ({ label: x.label, path: x.path, icon: "folderOpen" })),
      ...drives.filter((d) => d.online && !d.readOnly && d.mount !== "/").map((d) => ({ label: d.name, path: d.mount, icon: d.media === "network" ? "network" : "drive" })),
    ]
      .filter((d) => !here.has(d.path))
      .map((d) => ({ id: `sendto-${d.path}`, label: d.label, icon: d.icon, run: () => void startTransfer(paths, d.path, "copy") }));
    const custom: MenuItem[] = (cfg?.customActions ?? [])
      .filter((a) => {
        if (!picked.length) return false;
        if (a.appliesTo === "file" && picked.some((e) => e.isDir)) return false;
        if (a.appliesTo === "folder" && picked.some((e) => !e.isDir)) return false;
        const exts = a.extensions.split(",").map((x) => x.trim().toLowerCase().replace(/^\./, "")).filter(Boolean);
        return !exts.length || picked.every((e) => !e.isDir && exts.includes(e.name.split(".").pop()?.toLowerCase() ?? ""));
      })
      .map((a) => ({ id: `custom-${a.id}`, label: a.label, icon: "newItem", run: () => void api.runCustomAction(a.id, paths).catch((err) => showToast(String(err))) }));
    const dirOf = (h: Hit) => (h.isDir ? h.path : (parentPath(h.path) ?? h.path));
    const terminal = (dir: string, mode: "here" | "admin") => api.openTerminal(dir, mode).catch((e) => showToast(String(e)));
    const rename = async (h: Hit) => {
      const name = await promptText("Rename", h.name);
      if (!name || name === h.name) return;
      const made = await renamePath(h.path, name);
      if (!made) return;
      setResult((cur) => (cur ? { ...cur, hits: cur.hits.map((x) => (x.path === h.path ? { ...x, path: made, name } : x)) } : cur));
      setSelected(new Set([made]));
    };
    const items: (MenuItem | false | null | undefined)[] = [
      one && { id: "open", label: "Open", shortcut: "Enter", run: () => open(one) },
      one?.isDir && { id: "openNewTab", label: "Open in new tab", run: () => newTab(one.path, { background: true }) },
      one && !one.isDir && { id: "openWith", label: "Open with…", run: () => useStore.getState().chooseApp(one.path) },
      one && { id: "reveal", label: "Open containing folder", icon: "folderOpen", run: () => reveal(one) },
      one && { id: "revealNewTab", label: "Open containing folder in new tab", run: () => { const d = parentPath(one.path); if (d) newTab(d, { background: true }); } },
      ...shell.map((m, i) => (i === 0 ? { ...m, separatorBefore: true } : m)),
      ...custom.map((m, i) => (i === 0 ? { ...m, separatorBefore: true } : m)),
      one && { id: "pin", label: pinned ? "Unpin from sidebar" : "Pin to sidebar", icon: "pin", separatorBefore: shell.length === 0 && custom.length === 0, run: () => useStore.getState().togglePin(one.path, !one.isDir) },
      { id: "copyPath", label: "Copy as path", icon: "copy", run: () => void navigator.clipboard.writeText(paths.join("\n")) },
      {
        id: "share",
        label: "Share",
        icon: "share",
        children: [
          { id: "share-link", label: "Temporary download link…", icon: "share", run: () => useStore.setState({ shareFor: paths }) },
          { id: "share-path", separatorBefore: true, label: "Copy as path", run: () => void navigator.clipboard.writeText(paths.join("\n")) },
          { id: "share-name", label: "Copy name", run: () => void navigator.clipboard.writeText(picked.map((x) => x.name).join("\n")) },
        ],
      },
      sendTo.length > 0 && { id: "sendTo", label: "Send to", icon: "forward", children: sendTo },
      { id: "archive-zip", label: "Compress to ZIP", icon: "archive", run: () => void compressPaths(paths) },
      one && !one.isDir && isArchiveName(one.name) && { id: "archive-here", label: "Extract here", icon: "archive", run: () => void extractArchive(one.path, true) },
      one && !one.isDir && isArchiveName(one.name) && { id: "archive-to", label: `Extract to "${one.name.replace(/(\.tar)?\.[^.]+$/i, "")}"`, icon: "archive", run: () => void extractArchive(one.path, false) },
      { id: "cut", label: "Cut", icon: "cut", shortcut: "Ctrl+X", separatorBefore: true, run: () => setClipboard({ paths, op: "move" }) },
      { id: "copy", label: "Copy", icon: "copy", shortcut: "Ctrl+C", run: () => setClipboard({ paths, op: "copy" }) },
      { id: "paste", label: one?.isDir ? "Paste into this folder" : "Paste", icon: "paste", shortcut: "Ctrl+V", disabled: !dest, run: () => dest && void pasteInto(dest) },
      one && { id: "createShortcut", label: "Create shortcut", separatorBefore: true, run: () => void api.createShortcut(one.path).then(() => showToast("Shortcut created.")).catch((e) => showToast(String(e))) },
      { id: "delete", label: "Delete", icon: "trash", shortcut: "Del", run: () => void deletePaths(paths) },
      { id: "deletePermanent", label: "Delete permanently", shortcut: "Shift+Del", danger: true, run: () => void deletePaths(paths, true) },
      one && { id: "rename", label: "Rename", shortcut: "F2", icon: "rename", run: () => void rename(one) },
      one && { id: "terminal", label: "Open terminal here", icon: "terminal", separatorBefore: true, run: () => void terminal(dirOf(one), "here") },
      one && { id: "terminalAdmin", label: "Open terminal as administrator", run: () => void terminal(dirOf(one), "admin") },
      one?.isDir && { id: "analyze", label: "Analyze disk usage", icon: "chart", run: () => navigate(analyzeLocation(one.path)) },
      { id: "properties", label: "Properties", icon: "info", separatorBefore: true, run: () => useStore.setState({ properties: paths }) },
      one?.isDir && { id: "lock", label: "Lock with passcode…", icon: "lock", separatorBefore: true, run: () => useStore.getState().openLockDialog(one.path) },
    ];
    const hidden = new Set(cfg?.contextMenu.hidden ?? []);
    return items.filter((i): i is MenuItem => !!i && !hidden.has(i.id) && !hidden.has(i.id.replace(/-.*/, "")));
  };

  const showMenu = (x: number, y: number, h: Hit) => {
    let picked = chosen();
    if (!selected.has(h.path)) {
      picked = [h];
      setSelected(new Set([h.path]));
    }
    const ticket = ++menuTicket.current;
    setMenu({ x, y, items: buildMenu(picked) });
    // The system's own entries arrive a moment later; the menu never waits for them.
    if (!settings.general.shellExtras || picked.some((p) => isRemote(p.path))) return;
    void fetchFor(picked.map((p) => p.path), false).then((shell) => {
      if (ticket !== menuTicket.current || shell.length === 0) return;
      setMenu({ x, y, items: buildMenu(picked, shell) });
    });
  };

  // ---- Dragging: the whole selection travels, like in a folder -----------------
  const dragPayload = (h: Hit): DragPayload => {
    const group = selected.has(h.path) ? hits.filter((x) => selected.has(x.path)) : [h];
    return { paths: group.map((x) => x.path), dirs: group.map((x) => x.isDir), label: group.length === 1 ? group[0].name : `${group.length} items` };
  };

  const cutPaths = clipboard?.op === "move" ? new Set(clipboard.paths) : null;
  const ms = result ? (result.elapsedMs < 10 ? result.elapsedMs.toFixed(1) : Math.round(result.elapsedMs)) : 0;
  const count = `${formatCount(hits.length)}${!narrowed && hits.length >= settings.search.limit ? "+" : ""} result${hits.length === 1 ? "" : "s"}${
    narrowed && all.length !== hits.length ? ` of ${formatCount(all.length)}` : ""
  }`;
  const summary = error
    ? error
    : !result
      ? "Searching…"
      : result.searching
        ? `${count} so far · looking through folders that are not indexed yet (${formatCount(scanned)} items checked)`
        : `${count} in ${ms} ms · ${formatCount(result.searchedEntries)} items searched ${
            result.source === "index" ? "from the index" : "by reading the folder"
          }${result.complete ? "" : " (partial)"}`;
  const selectedBytes = chosen().reduce((n, h) => n + (h.isDir ? 0 : h.size), 0);

  const headerCell = (key: HitSortKey, label: string, cls: string) => (
    <div className={`cell ${cls} header-cell`} role="columnheader" aria-sort={sort.key === key ? (sort.asc ? "ascending" : "descending") : undefined}
      onClick={() => setSort({ key, asc: sort.key === key ? !sort.asc : true })}>
      {label}
      {sort.key === key && <span className="sort-arrow">{sort.asc ? "▲" : "▼"}</span>}
    </div>
  );

  return (
    <div className="file-view search-view">
      <div className="search-summary">
        <span>{summary}</span>
        <span className="muted">
          {scope ? `in ${scope}` : "everywhere indexed"}
          {indexStatus?.state === "scanning" ? ` · indexing drives (${indexStatus.drivesReady} of ${indexStatus.drivesTotal} ready)` : ""}
        </span>
      </div>
      <div className="search-controls">
        <label>
          Sort
          <select value={sort.key} onChange={(e) => setSort({ key: e.target.value as HitSortKey, asc: true })}>
            {HIT_SORTS.map(([k, l]) => <option key={k} value={k}>{l}</option>)}
          </select>
        </label>
        <button className="icon-button" aria-label={sort.asc ? "Ascending" : "Descending"} title={sort.asc ? "Ascending (click for descending)" : "Descending (click for ascending)"} onClick={() => setSort({ ...sort, asc: !sort.asc })}>
          {sort.asc ? "▲" : "▼"}
        </button>
        <label>
          Type
          <select
            value={filter.notKinds[0] ? `!${filter.notKinds[0]}` : filter.kinds[0] ?? ""}
            onChange={(e) => {
              const v = e.target.value;
              if (v.startsWith("!")) setFilter({ kinds: [], notKinds: [v.slice(1)] });
              else setFilter({ kinds: v ? [v] : [], notKinds: [] });
            }}
          >
            <option value="">All</option>
            <optgroup label="Is">
              {kindFacets.map(([k, n]) => <option key={k} value={k}>{k} ({n})</option>)}
            </optgroup>
            <optgroup label="Is not">
              {kindFacets.map(([k, n]) => <option key={`!${k}`} value={`!${k}`}>not {k} ({all.length - n})</option>)}
            </optgroup>
          </select>
        </label>
        <label>
          Size
          <select
            value={filter.notSize !== "any" ? `!${filter.notSize}` : filter.size}
            onChange={(e) => {
              const v = e.target.value;
              if (v.startsWith("!")) setFilter({ size: "any", notSize: v.slice(1) });
              else setFilter({ size: v, notSize: "any" });
            }}
          >
            <option value="any">Any</option>
            <optgroup label="Is">
              <option value="empty">Empty</option>
              <option value="small">Under 1 MB</option>
              <option value="medium">1 – 100 MB</option>
              <option value="large">Over 100 MB</option>
            </optgroup>
            <optgroup label="Is not">
              <option value="!empty">Not empty</option>
              <option value="!small">Not under 1 MB</option>
              <option value="!medium">Not 1 – 100 MB</option>
              <option value="!large">Not over 100 MB</option>
            </optgroup>
          </select>
        </label>
        <label>
          Modified
          <select
            value={filter.notModified !== "any" ? `!${filter.notModified}` : filter.modified}
            onChange={(e) => {
              const v = e.target.value;
              if (v.startsWith("!")) setFilter({ modified: "any", notModified: v.slice(1) });
              else setFilter({ modified: v, notModified: "any" });
            }}
          >
            <option value="any">Any time</option>
            <optgroup label="Is">
              <option value="today">Today</option>
              <option value="week">Past week</option>
              <option value="month">Past month</option>
              <option value="year">Past year</option>
              <option value="older">Older than a year</option>
            </optgroup>
            <optgroup label="Is not">
              <option value="!today">Not today</option>
              <option value="!week">Not in the past week</option>
              <option value="!month">Not in the past month</option>
              <option value="!year">Not in the past year</option>
              <option value="!older">Not older than a year</option>
            </optgroup>
          </select>
        </label>
        <label>
          Name is not
          <input className="text-input" value={filter.notText} placeholder="doesn't contain" onChange={(e) => setFilter({ notText: e.target.value })} />
        </label>
        {narrowed && <button className="link-button" onClick={() => setFilter(null)}>Clear filters</button>}
      </div>
      {busy && <div className="loading-bar" />}
      <div ref={scroller} className="file-scroller" tabIndex={0} onKeyDown={onKeyDown} onMouseDown={startBand}>
        {result && hits.length === 0 ? (
          <div className="empty-state">
            <strong>{narrowed && all.length ? "Nothing matches the filters" : `No matches for "${query}"`}</strong>
            <span>{narrowed && all.length ? `${formatCount(all.length)} found before filtering` : "Filters: ext:pdf · type:folder · size:>10mb · wildcards like *.txt"}</span>
          </div>
        ) : (
          <>
            <div className="file-header search-header" role="row" onMouseDown={(e) => e.stopPropagation()}>
              {headerCell("name", "Name", "cell-name")}
              <div className="cell cell-path header-cell">Folder</div>
              {headerCell("modified", "Date modified", "cell-date")}
              {headerCell("size", "Size", "cell-size")}
            </div>
            <div className="file-rows" style={{ height: virtualizer.getTotalSize() }}>
              {virtualizer.getVirtualItems().map((row) => {
                const h = hits[row.index];
                const i = row.index;
                return (
                  <div
                    key={h.path}
                    className="file-row search-row"
                    style={{ transform: `translateY(${row.start}px)`, height: rowHeight }}
                    data-selected={selected.has(h.path) || undefined}
                    data-cut={cutPaths?.has(h.path) || undefined}
                    data-drop={h.isDir ? "folder" : undefined}
                    data-drop-path={h.isDir ? h.path : undefined}
                    onPointerDown={(ev) => {
                      if (ev.ctrlKey || ev.shiftKey || ev.metaKey) return;
                      beginDrag(ev, () => {
                        if (!selected.has(h.path)) setSelected(new Set([h.path]));
                        return dragPayload(h);
                      });
                    }}
                    onMouseDown={(ev) => {
                      ev.stopPropagation();
                      collapseOnClick.current = null;
                      if (ev.button !== 0) return;
                      const plain = !ev.ctrlKey && !ev.shiftKey && !ev.metaKey;
                      if (plain && selected.size > 1 && selected.has(h.path) && adv<boolean>(settings, "files.keepSelectionOnDrag")) {
                        collapseOnClick.current = h.path;
                        cursor.current = i;
                      } else select(i, ev);
                      scroller.current?.focus({ preventScroll: true });
                    }}
                    onClick={() => {
                      if (collapseOnClick.current === h.path) {
                        collapseOnClick.current = null;
                        select(i, { ctrlKey: false, metaKey: false, shiftKey: false });
                      }
                    }}
                    onDoubleClick={() => open(h)}
                    onContextMenu={(e) => {
                      e.preventDefault();
                      e.stopPropagation();
                      showMenu(e.clientX, e.clientY, h);
                    }}
                  >
                    <div className="cell cell-name">
                      <FileIcon kind={kindOf(h.name, h.isDir)} name={h.name} />
                      <span className="file-name">{h.name}</span>
                    </div>
                    <div className="cell cell-path" title={h.path}>
                      {parentPath(h.path)}
                    </div>
                    <div className="cell cell-date">{formatDate(h.modified)}</div>
                    <div className="cell cell-size">{h.isDir ? "" : formatSize(h.size)}</div>
                  </div>
                );
              })}
            </div>
          </>
        )}
        {band && <div className="select-band" style={{ left: band.l, top: band.t, width: band.w, height: band.h }} />}
      </div>
      {selected.size > 0 && (
        <div className="search-selection">
          {selected.size} selected{selectedBytes ? ` · ${formatSize(selectedBytes)}` : ""}
        </div>
      )}
      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
    </div>
  );
}
