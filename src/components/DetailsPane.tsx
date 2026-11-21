// Details pane: everything the OS knows about the selection, plus folder
// statistics and checksums computed on demand in the background.

import { useEffect, useMemo, useState } from "react";
import { api, type Details, type FolderStats } from "../api";
import { useStore } from "../store";
import { adv } from "../settings/advanced";
import { isLiveAt, liveKey, liveSummary, useLive, type LiveFile } from "../util/liveFiles";
import { loadingNotice } from "../util/progressive";
import { formatCount, formatDate, formatDuration, formatSize, kindOf, typeLabel } from "../util/format";
import { baseName, isArchiveLoc, isRemote, maskAddresses, parentPath, parseLocation } from "../util/path";
import { FileIcon } from "./Icons";
import { Thumb } from "./Thumb";
import { loadOpener, openerApplies, peekOpener, showOpener } from "../util/opener";

function Row({ label, value, mono }: { label: string; value: React.ReactNode; mono?: boolean }) {
  if (value == null || value === "") return null;
  return (
    <div className="details-row">
      <dt>{label}</dt>
      <dd className={mono ? "mono" : undefined}>{value}</dd>
    </div>
  );
}

/** "Opens with": the program a double-click starts, with its picture, and a way to change it. */
function OpensWith({ path, isDir }: { path: string; isDir: boolean }) {
  const [opener, setOpener] = useState(() => peekOpener(path, true));
  useEffect(() => {
    let alive = true;
    setOpener(peekOpener(path, true));
    if (!isDir) void loadOpener(path, true).then((o) => alive && setOpener(o)).catch(() => {});
    return () => {
      alive = false;
    };
  }, [path, isDir]);
  const change = () => useStore.getState().chooseApp(path);
  if (isDir) return <Row label="Opens in" value="This window" />;
  if (!opener) return null;
  const words = opener.kind === "app" ? opener.name : opener.kind === "runs" ? "Runs itself" : opener.kind === "missing" ? `${opener.name || "The program"} is missing` : "No program set";
  const icon = opener.iconUrl ? <img src={opener.iconUrl} width={16} height={16} alt="" draggable={false} /> : null;
  return (
    <Row
      label="Opens with"
      value={
        <span className="opens-with" data-kind={opener.kind} title={opener.id || undefined}>
          {icon}
          <span className="opens-with-name">{words}</span>
          {opener.kind !== "runs" && (
            <button className="link-btn" onClick={change}>
              {opener.kind === "app" ? "Change…" : "Choose…"}
            </button>
          )}
        </span>
      }
    />
  );
}

/** The files among these paths that are being written right now (empty when following is off). */
function useGrowing(paths: string[], on: boolean): LiveFile[] {
  const key = useLive((st) => (on ? liveKey(st.files, paths) : ""));
  return useMemo(() => {
    if (!key) return [];
    const files = useLive.getState().files;
    return paths.map((p) => files[p]).filter((l): l is LiveFile => !!l && l.growing);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
}

/** A transfer of this app that is writing into the folder of this file, if any. */
function transferFor(path: string) {
  const dir = parentPath(path);
  return Object.values(useStore.getState().transfers).find((t) => t.phase === "running" && dir != null && t.destDir === dir);
}

/** How a growing file is doing: how much, how fast, for how long. */
function LiveRows({ file, name }: { file: LiveFile; name?: string }) {
  const sum = liveSummary(file, Date.now());
  const t = transferFor(file.path);
  return (
    <dl className="details-list live-rows" data-live>
      {name && <div className="details-row"><dt>File</dt><dd>{name}</dd></div>}
      <Row label="Status" value={t ? "Being copied by this app" : "Being written"} />
      <Row label="Written so far" value={`${formatSize(file.size)}${sum.added > 0 ? ` (+${formatSize(sum.added)} since it was seen)` : ""}`} />
      <Row label="Speed now" value={file.rate > 0 ? `${formatSize(file.rate)}/s` : "paused"} />
      <Row label="Average speed" value={sum.average > 0 ? `${formatSize(sum.average)}/s` : null} />
      <Row label="Fastest" value={file.peak > 0 ? `${formatSize(file.peak)}/s` : null} />
      <Row label="Followed for" value={formatDuration(sum.seconds)} />
      <Row label="Last change" value={sum.sinceChange < 1.5 ? "just now" : `${Math.round(sum.sinceChange)} s ago`} />
      {t && t.bytesTotal > 0 && <Row label="This copy" value={`${Math.min(100, Math.round((t.bytesDone / t.bytesTotal) * 100))}% of ${formatSize(t.bytesTotal)}${t.etaSecs ? `, about ${formatDuration(t.etaSecs)} left` : ""}`} />}
    </dl>
  );
}

/** What a group of selected items adds up to. */
function MultiSummary({ paths }: { paths: string[] }) {
  const [items, setItems] = useState<Details[] | null>(null);
  const [folders, setFolders] = useState<FolderStats | "loading" | null>(null);
  const key = paths.join("\n");
  const settings = useStore((st) => st.settings);
  const liveOn = !!settings && adv<boolean>(settings, "live.enabled") && adv<boolean>(settings, "live.details");
  const growing = useGrowing(paths, liveOn);

  useEffect(() => {
    setItems(null);
    setFolders(null);
    let alive = true;
    // Short pause so dragging a selection over many rows does not stat them all at each step.
    const t = setTimeout(async () => {
      const got: Details[] = [];
      const queue = paths.slice(0, 500);
      await Promise.all(
        Array.from({ length: 6 }, async () => {
          for (let p = queue.shift(); p !== undefined && alive; p = queue.shift()) {
            const d = await api.getDetails(p).catch(() => null);
            if (d) got.push(d);
          }
        }),
      );
      if (alive) setItems(got);
    }, 120);
    return () => {
      alive = false;
      clearTimeout(t);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  const title = <div className="details-title"><strong>{paths.length.toLocaleString()} items selected</strong></div>;
  if (!items) return <>{title}<p className="muted">Adding up…</p></>;

  const dirs = items.filter((d) => d.kind === "folder");
  const files = items.filter((d) => d.kind !== "folder");
  // Files being written count with the size they have now.
  const liveSize = (d: Details) => growing.find((l) => l.path === d.path)?.size ?? d.size;
  const fileBytes = files.reduce((n, d) => n + liveSize(d), 0);
  const onDisk = files.reduce((n, d) => n + (growing.some((l) => l.path === d.path) ? liveSize(d) : (d.sizeOnDisk ?? d.size)), 0);
  const times = items.map((d) => d.modified).filter((m): m is number => m != null);
  const parents = new Set(items.map((d) => d.parent));
  const byType = new Map<string, { count: number; bytes: number }>();
  for (const d of files) {
    const label = typeLabel(d.name, false);
    const e = byType.get(label) ?? { count: 0, bytes: 0 };
    byType.set(label, { count: e.count + 1, bytes: e.bytes + d.size });
  }
  const types = [...byType.entries()].sort((a, b) => b[1].bytes - a[1].bytes);
  const big = [...files].sort((a, b) => b.size - a.size)[0];
  const stats = folders && folders !== "loading" ? folders : null;
  const measure = () => {
    setFolders("loading");
    void Promise.all(dirs.map((d) => api.folderStats(d.path).catch(() => null))).then((all) => {
      const ok = all.filter((x): x is FolderStats => !!x);
      setFolders({
        files: ok.reduce((n, x) => n + x.files, 0),
        folders: ok.reduce((n, x) => n + x.folders, 0),
        totalBytes: ok.reduce((n, x) => n + x.totalBytes, 0),
        maxDepth: Math.max(0, ...ok.map((x) => x.maxDepth)),
        largest: [],
        byType: [],
        newest: null,
        oldest: null,
        emptyFolders: ok.reduce((n, x) => n + x.emptyFolders, 0),
        complete: ok.every((x) => x.complete),
      });
    });
  };

  return (
    <>
      {title}
      <dl className="details-list">
        <Row label="Contains" value={[files.length && `${formatCount(files.length)} file${files.length === 1 ? "" : "s"}`, dirs.length && `${formatCount(dirs.length)} folder${dirs.length === 1 ? "" : "s"}`].filter(Boolean).join(" and ")} />
        <Row label="Size" value={files.length ? `${formatSize(fileBytes)}${dirs.length ? " in the files" : ""}` : undefined} />
        <Row label="On disk" value={files.length && onDisk !== fileBytes ? formatSize(onDisk) : undefined} />
        <Row label="Largest file" value={big ? `${big.name} (${formatSize(big.size)})` : undefined} />
        <Row label="Location" value={parents.size === 1 ? maskAddresses([...parents][0] ?? "") : `${parents.size} different folders`} mono />
        <Row label="Oldest change" value={times.length ? formatDate(Math.min(...times)) : undefined} />
        <Row label="Newest change" value={times.length ? formatDate(Math.max(...times)) : undefined} />
        <Row label="Read-only" value={items.some((d) => d.readonly) ? `${items.filter((d) => d.readonly).length} of ${items.length}` : undefined} />
        <Row label="Hidden" value={items.some((d) => d.hidden) ? `${items.filter((d) => d.hidden).length} of ${items.length}` : undefined} />
        {paths.length > items.length && <Row label="Note" value={`Totals cover the first ${items.length} items.`} />}
      </dl>
      {growing.length > 0 && (
        <div className="details-section" data-live>
          <strong>Being written now ({growing.length})</strong>
          <dl className="details-list">
            <Row label="Total speed" value={`${formatSize(growing.reduce((n, l) => n + l.rate, 0))}/s`} />
          </dl>
          {growing.map((l) => (
            <LiveRows key={l.path} file={l} name={baseName(l.path)} />
          ))}
        </div>
      )}
      {types.length > 0 && (
        <div className="details-section">
          <strong>Types</strong>
          {types.slice(0, 8).map(([label, t]) => (
            <div key={label} className="type-row">
              <span className="type-name" title={label}>{label}</span>
              <span className="muted">×{t.count}</span>
              <span className="grow" />
              <span className="type-size">{formatSize(t.bytes)}</span>
            </div>
          ))}
        </div>
      )}
      {dirs.length > 0 && (
        <div className="details-section">
          {stats ? (
            <dl className="details-list">
              <Row label="Inside folders" value={`${formatCount(stats.files)} files, ${formatCount(stats.folders)} folders`} />
              <Row label="Folders' size" value={`${stats.complete ? "" : "≥ "}${formatSize(stats.totalBytes)}`} />
              <Row label="Everything" value={`${formatSize(stats.totalBytes + fileBytes)}`} />
              <Row label="Empty folders" value={stats.emptyFolders ? formatCount(stats.emptyFolders) : undefined} />
            </dl>
          ) : (
            <button className="secondary" disabled={folders === "loading"} onClick={measure}>
              {folders === "loading" ? "Measuring…" : `Measure ${dirs.length === 1 ? "the folder" : `${dirs.length} folders`}`}
            </button>
          )}
        </div>
      )}
    </>
  );
}

/** Says the folder is still being read, and when it should be done. */
function LoadingNotice() {
  const p = useStore((s) => s.loadingDir);
  const [, tick] = useState(0);
  useEffect(() => {
    if (!p) return;
    const t = setInterval(() => !document.hidden && tick((n) => n + 1), 500);
    return () => clearInterval(t);
  }, [p]);
  if (!p) return null;
  const n = loadingNotice(p, Date.now(), formatDuration);
  return (
    <div className="loading-notice" role="status">
      <strong>{n.title}</strong>
      <span>{n.detail}</span>
      <div className="loading-bar" data-indeterminate={n.fraction == null ? "" : undefined} aria-hidden="true">
        <i style={n.fraction == null ? undefined : { width: `${Math.round(n.fraction * 100)}%` }} />
      </div>
    </div>
  );
}

export function DetailsPane({ paths, embedded }: { paths?: string[]; embedded?: boolean } = {}) {
  const liveSelection = useStore((s) => s.selection);
  const selection = paths ?? liveSelection;
  const location = useStore((s) => (paths ? "" : (s.tabs.find((t) => t.id === s.activeTab)?.location ?? "")));
  const loc = parseLocation(location);
  // One selected item, or the folder being viewed when nothing is selected.
  const target = selection.length === 1 ? selection[0] : selection.length === 0 && loc.kind === "folder" ? loc.path : null;

  // Refetch when the item's folder (or the folder itself) changes on disk.
  const version = useStore((s) =>
    target ? (s.dirVersion[target] ?? 0) + (s.dirVersion[parentPath(target) ?? ""] ?? 0) : 0,
  );
  const [details, setDetails] = useState<Details | null>(null);
  const [stats, setStats] = useState<FolderStats | "loading" | null>(null);
  const [hash, setHash] = useState<string | "loading" | null>(null);

  // A different item: start clean. The same item changing on disk: keep showing it and swap the numbers in place.
  useEffect(() => {
    setDetails(null);
    setStats(null);
    setHash(null);
  }, [target]);

  const settings = useStore((st) => st.settings);
  const liveOn = !!settings && adv<boolean>(settings, "live.enabled") && adv<boolean>(settings, "live.details");
  const growing = useGrowing(target ? [target] : [], liveOn);
  const live = growing[0];
  const blockHash = !!settings && adv<boolean>(settings, "live.blockHash") && !!target && (!!live || isLiveAt(target));

  useEffect(() => {
    if (!target) return;
    let alive = true;
    // Brief delay so arrowing quickly through a list does not stat every row.
    const t = setTimeout(() => {
      api
        .getDetails(target)
        .then((d) => alive && setDetails(d))
        .catch(() => {});
    }, 60);
    return () => {
      alive = false;
      clearTimeout(t);
    };
  }, [target, version]);

  if (selection.length > 1) {
    return (
      <aside className="details-pane" data-embedded={embedded || undefined}>
        {!paths && <LoadingNotice />}
        <MultiSummary paths={selection} />
      </aside>
    );
  }
  if (!target || !details || details.path !== target) {
    return (
      <aside className="details-pane" data-embedded={embedded || undefined}>
        {!paths && <LoadingNotice />}
        <p className="muted">{target ? "" : "Select a file or folder to see its details."}</p>
      </aside>
    );
  }

  const d = details;
  const isDir = d.kind === "folder";
  const calcStats = () => {
    setStats("loading");
    api.folderStats(d.path).then(setStats).catch(() => setStats(null));
  };
  const calcHash = () => {
    setHash("loading");
    api.hashFile(d.path).then(setHash).catch(() => setHash(null));
  };
  const max = stats && stats !== "loading" ? Math.max(1, ...stats.byType.map((t) => t.bytes)) : 1;

  return (
    <aside className="details-pane" data-embedded={embedded || undefined}>
      {!paths && <LoadingNotice />}
      <div className="details-preview">
        <Thumb path={d.path} name={d.name} isDir={isDir} modified={d.modified} size={160} iconSize={72} view="pane" />
      </div>
      <div className="details-title">
        <strong>{d.name}</strong>
        <span className="muted">{isDir ? "File folder" : typeLabel(d.name, false)}</span>
      </div>

      <dl className="details-list">
        <Row label="Location" value={d.parent ? maskAddresses(d.parent) : d.parent} mono />
        {!isDir && <Row label="Size" value={`${formatSize(live?.size ?? d.size)} (${formatCount(live?.size ?? d.size)} bytes)`} />}
        {!isDir && d.sizeOnDisk != null && <Row label="Size on disk" value={formatSize(d.sizeOnDisk)} />}
        {isDir && <Row label="Contains" value={d.itemCount != null ? `${formatCount(d.itemCount)} ${d.itemCount === 1 ? "item" : "items"}` : null} />}
        <Row label="Modified" value={formatDate(live?.modified ?? d.modified)} />
        <Row label="Created" value={formatDate(d.created)} />
        <Row label="Accessed" value={formatDate(d.accessed)} />
        <Row label="Content type" value={d.mime} />
        {showOpener(settings) && openerApplies(d.path) && <OpensWith path={d.path} isDir={isDir} />}
        <Row label="Dimensions" value={d.image ? `${d.image[0]} × ${d.image[1]} px` : null} />
        {d.media.map((f) => (
          <Row key={f.label} label={f.label} value={f.value} />
        ))}
        <Row label="Link target" value={d.symlinkTarget} mono />
        <Row label="Read-only" value={d.readonly ? "Yes" : null} />
        <Row label="Hidden" value={d.hidden ? "Yes" : null} />
        <Row label="Attributes" value={d.attributes.join(", ")} />
        {d.unix && (
          <>
            <Row label="Permissions" value={`${d.unix.mode} (${d.unix.octal})`} mono />
            <Row label="Owner / group" value={`${d.unix.uid} / ${d.unix.gid}`} />
            <Row label="Hard links" value={!isDir && d.unix.hardLinks > 1 ? d.unix.hardLinks : null} />
            <Row label="Inode" value={d.unix.inode} />
          </>
        )}
        {d.volume && (
          <>
            <Row label="Drive" value={`${d.volume.name} (${d.volume.fs.toUpperCase() || "unknown"})`} />
            <Row label="Free on drive" value={`${formatSize(d.volume.available)} of ${formatSize(d.volume.total)}`} />
          </>
        )}
      </dl>

      {live && (
        <div className="details-section" data-live>
          <strong>Being written now</strong>
          <LiveRows file={live} />
        </div>
      )}

      {!isDir && !isRemote(d.path) && !isArchiveLoc(d.path) && (
        <div className="details-section">
          {hash === null ? (
            <button className="secondary" disabled={blockHash} title={blockHash ? "The file is still being written. This works once it stops growing." : undefined} onClick={calcHash}>
              {blockHash ? "Compute SHA-256 (file still being written)" : "Compute SHA-256"}
            </button>
          ) : hash === "loading" ? (
            <span className="muted">Hashing…</span>
          ) : (
            <dl className="details-list">
              <Row label="SHA-256" value={hash} mono />
            </dl>
          )}
        </div>
      )}

      {isDir && !isRemote(d.path) && !isArchiveLoc(d.path) && (
        <div className="details-section">
          {stats === null ? (
            <button className="secondary" onClick={calcStats}>
              Calculate folder size and contents
            </button>
          ) : stats === "loading" ? (
            <>
              <div className="loading-bar" />
              <span className="muted">Measuring folder…</span>
            </>
          ) : (
            <>
              <dl className="details-list">
                <Row label="Total size" value={`${formatSize(stats.totalBytes)}${stats.complete ? "" : " (partial)"}`} />
                <Row label="Files" value={formatCount(stats.files)} />
                <Row label="Folders" value={formatCount(stats.folders)} />
                <Row label="Empty folders" value={stats.emptyFolders ? formatCount(stats.emptyFolders) : null} />
                <Row label="Deepest level" value={stats.maxDepth} />
                <Row label="Newest file" value={stats.newest ? `${baseName(stats.newest.path)} · ${formatDate(stats.newest.value)}` : null} />
                <Row label="Oldest file" value={stats.oldest ? `${baseName(stats.oldest.path)} · ${formatDate(stats.oldest.value)}` : null} />
              </dl>
              {stats.byType.length > 0 && <h4>Space by type</h4>}
              {stats.byType.map((t) => (
                <div key={t.extension} className="type-row" title={`${formatCount(t.count)} files`}>
                  <span className="type-name">.{t.extension}</span>
                  <div className="meter">
                    <div className="meter-fill" style={{ width: `${(t.bytes / max) * 100}%` }} />
                  </div>
                  <span className="type-size">{formatSize(t.bytes)}</span>
                </div>
              ))}
              {stats.largest.length > 0 && <h4>Largest files</h4>}
              {stats.largest.slice(0, 5).map((f) => (
                <div key={f.path} className="type-row" title={f.path}>
                  <FileIcon kind={kindOf(f.path, false)} size={14} />
                  <span className="grow file-name">{baseName(f.path)}</span>
                  <span className="type-size">{formatSize(f.value)}</span>
                </div>
              ))}
            </>
          )}
        </div>
      )}
    </aside>
  );
}
