// Non-folder pages: Home, This PC, search results and disk usage.

import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useMemo, useRef, useState } from "react";
import { mergeHits } from "../util/searchHits";
import { api, on, type Drive, type Hit, type SearchResponse } from "../api";
import { ask, deletePaths, thisPcLabel, useStore } from "../store";
import { driveLetter, driveName, driveRemote, hidesAddress, withoutLetter } from "../util/drives";
import { applyFilter } from "../util/filter";
import { formatCount, formatDate, formatSize, kindOf } from "../util/format";
import { analyzeLocation, baseName, parentPath } from "../util/path";
import { ContextMenu, type MenuState } from "./ContextMenu";
import { FileIcon, Icon } from "./Icons";

// ---------------------------------------------------------------------------
// Home: pins and recents. Nothing appears here that the user did not pin or open.
// ---------------------------------------------------------------------------

export function HomeView() {
  const pins = useStore((s) => s.pins);
  const recent = useStore((s) => s.recent);
  const { navigate, addRecent, clearRecent, showToast } = useStore.getState();

  const openPath = (path: string, isDir: boolean) => {
    if (isDir) navigate(path);
    else {
      addRecent(path, false);
      api.openItem(path).catch((e) => showToast(String(e)));
    }
  };

  return (
    <div className="page home-view">
      <h2>Pinned</h2>
      {pins.length === 0 ? (
        <p className="muted">Nothing pinned yet. Right-click any file or folder and choose "Pin to sidebar".</p>
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

  const groups: [string, Drive[]][] = [
    ["Devices and drives", drives.filter((d) => d.media !== "network")],
    ["Network locations", drives.filter((d) => d.media === "network")],
  ];

  return (
    <div className="page drives-view">
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
                    ? "Not reachable"
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
                        const alias = appearance.driveNames?.[d.mount];
                        setMenu({
                          x: e.clientX,
                          y: e.clientY,
                          items: [
                            { id: "open", label: "Open", run: () => navigate(d.mount) },
                            { id: "analyze", label: "Analyze disk usage", run: () => navigate(analyzeLocation(d.mount)) },
                            { id: "rename", label: "Rename in this app…", icon: "rename", shortcut: "F2", separatorBefore: true, run: () => void renameDrive(d) },
                            ...(alias?.name && alias.base === d.name ? [{ id: "resetName", label: "Use the system's name", run: () => setAlias(d, { name: "" }) }] : []),
                            ...(d.media === "network"
                              ? [{ id: "address", label: hidesAddress(d, appearance) ? "Show server address" : "Hide server address", run: () => setAlias(d, { hideAddress: !hidesAddress(d, appearance) }) }]
                              : []),
                          ],
                        });
                      }}
                      title={`${d.mount}\nDouble-click to open`}
                    >
                      {showSpace && appearance.driveChart === "donut" && d.online ? (
                        <Donut fraction={shown} />
                      ) : (
                        <Icon name={d.media === "network" ? "network" : "drive"} size={34} />
                      )}
                      <div className="drive-info">
                        <strong>{driveName(d, appearance)}</strong>
                        {showSpace && appearance.driveChart === "bar" && d.online && (
                          <div className="meter" role="img" aria-label={text}>
                            <div className="meter-fill" style={{ width: `${shown * 100}%` }} />
                          </div>
                        )}
                        {showSpace && <span className="muted drive-space">{text}</span>}
                        {driveRemote(d, appearance) && <span className="muted path" title={d.remote ?? ""}>{d.remote}</span>}
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
      {drives.length === 0 && <p className="muted">{fresh ? "No drives found." : "Looking for drives…"}</p>}
      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Search results
// ---------------------------------------------------------------------------

let searchSeq = 0;

export function SearchView({ query, scope }: { query: string; scope: string | null }) {
  const settings = useStore((s) => s.settings)!;
  const indexStatus = useStore((s) => s.indexStatus);
  const { navigate, newTab, addRecent, showToast } = useStore.getState();
  const [result, setResult] = useState<SearchResponse | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const scroller = useRef<HTMLDivElement>(null);

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
    const sub = on("search-hits", (b) => {
      if (!alive || b.id !== id) return;
      setResult((cur) => (cur ? { ...cur, hits: mergeHits(cur.hits, b.hits, limit), searching: !b.done } : cur));
      setScanned(b.scanned);
      if (b.done) setBusy(false);
    });
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
  }, [query, scope]);

  const filter = useStore((s) => s.filter);
  const hits = useMemo(() => {
    const all = result?.hits ?? [];
    // The search engine has no date syntax, so the date part of the filter is applied here.
    if (filter.modified === "any") return all;
    const asEntries = all.map((h) => ({ name: h.name, isDir: h.isDir, isSymlink: false, isHidden: false, size: h.size, modified: h.modified, created: null }));
    const keep = new Set(applyFilter(asEntries, { ...filter, text: "", kinds: [], extensions: [], size: "any" }).map((e) => e.name));
    return all.filter((h) => keep.has(h.name));
  }, [result, filter]);
  const rowHeight = settings.appearance.density === "compact" ? 26 : 34;
  const virtualizer = useVirtualizer({
    count: hits.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => rowHeight,
    overscan: 12,
  });

  // Opening a result keeps the results: folders go to a new tab by default.
  const open = (h: Hit) => {
    if (h.isDir) {
      if (settings.general.openSearchResultInNewTab) newTab(h.path);
      else navigate(h.path);
    } else {
      addRecent(h.path, false);
      api.openItem(h.path).catch((e) => showToast(String(e)));
    }
  };
  const reveal = (h: Hit) => {
    const dir = parentPath(h.path);
    if (!dir) return;
    if (settings.general.openSearchResultInNewTab) newTab(dir);
    else navigate(dir);
  };

  const ms = result ? (result.elapsedMs < 10 ? result.elapsedMs.toFixed(1) : Math.round(result.elapsedMs)) : 0;
  const count = `${formatCount(hits.length)}${hits.length >= settings.search.limit ? "+" : ""} result${hits.length === 1 ? "" : "s"}`;
  const summary = error
    ? error
    : !result
      ? "Searching…"
      : result.searching
        ? `${count} so far · looking through folders that are not indexed yet (${formatCount(scanned)} items checked)`
        : `${count} in ${ms} ms · ${formatCount(result.searchedEntries)} items searched ${
            result.source === "index" ? "from the index" : "by reading the folder"
          }${result.complete ? "" : " (partial)"}`;

  return (
    <div className="file-view search-view">
      <div className="search-summary">
        <span>{summary}</span>
        <span className="muted">
          {scope ? `in ${scope}` : "everywhere indexed"}
          {indexStatus?.state === "scanning"
            ? ` · indexing drives (${indexStatus.drivesReady} of ${indexStatus.drivesTotal} ready)`
            : ""}
        </span>
      </div>
      {busy && <div className="loading-bar" />}
      <div ref={scroller} className="file-scroller" tabIndex={0}>
        {result && hits.length === 0 ? (
          <div className="empty-state">
            <strong>No matches for "{query}"</strong>
            <span>Filters: ext:pdf · type:folder · size:&gt;10mb · wildcards like *.txt</span>
          </div>
        ) : (
          <div className="file-rows" style={{ height: virtualizer.getTotalSize() }}>
            {virtualizer.getVirtualItems().map((row) => {
              const h = hits[row.index];
              return (
                <div
                  key={h.path}
                  className="file-row search-row"
                  style={{ transform: `translateY(${row.start}px)`, height: rowHeight }}
                  data-selected={selected === h.path || undefined}
                  onMouseDown={() => setSelected(h.path)}
                  onDoubleClick={() => open(h)}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    setSelected(h.path);
                    setMenu({
                      x: e.clientX,
                      y: e.clientY,
                      items: [
                        { id: "open", label: "Open", run: () => open(h) },
                        { id: "reveal", label: "Open containing folder", run: () => reveal(h) },
                        { id: "copyPath", label: "Copy as path", separatorBefore: true, run: () => void navigator.clipboard.writeText(h.path) },
                        { id: "delete", label: "Delete", separatorBefore: true, run: () => void deletePaths([h.path]) },
                      ],
                    });
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
        )}
      </div>
      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
    </div>
  );
}
