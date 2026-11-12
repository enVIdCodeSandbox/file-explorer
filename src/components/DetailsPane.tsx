// Details pane: everything the OS knows about the selection, plus folder
// statistics and checksums computed on demand in the background.

import { useEffect, useState } from "react";
import { api, type Details, type FolderStats } from "../api";
import { useStore } from "../store";
import { formatCount, formatDate, formatSize, kindOf, typeLabel } from "../util/format";
import { baseName, parentPath, parseLocation } from "../util/path";
import { FileIcon } from "./Icons";
import { Thumb } from "./Thumb";

function Row({ label, value, mono }: { label: string; value: React.ReactNode; mono?: boolean }) {
  if (value == null || value === "") return null;
  return (
    <>
      <dt>{label}</dt>
      <dd className={mono ? "mono" : undefined}>{value}</dd>
    </>
  );
}

/** What a group of selected items adds up to. */
function MultiSummary({ paths }: { paths: string[] }) {
  const [items, setItems] = useState<Details[] | null>(null);
  const [folders, setFolders] = useState<FolderStats | "loading" | null>(null);
  const key = paths.join("\n");

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
  const fileBytes = files.reduce((n, d) => n + d.size, 0);
  const onDisk = files.reduce((n, d) => n + (d.sizeOnDisk ?? d.size), 0);
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
        <Row label="Location" value={parents.size === 1 ? [...parents][0] : `${parents.size} different folders`} mono />
        <Row label="Oldest change" value={times.length ? formatDate(Math.min(...times)) : undefined} />
        <Row label="Newest change" value={times.length ? formatDate(Math.max(...times)) : undefined} />
        <Row label="Read-only" value={items.some((d) => d.readonly) ? `${items.filter((d) => d.readonly).length} of ${items.length}` : undefined} />
        <Row label="Hidden" value={items.some((d) => d.hidden) ? `${items.filter((d) => d.hidden).length} of ${items.length}` : undefined} />
        {paths.length > items.length && <Row label="Note" value={`Totals cover the first ${items.length} items.`} />}
      </dl>
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

  useEffect(() => {
    setDetails(null);
    setStats(null);
    setHash(null);
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
        <MultiSummary paths={selection} />
      </aside>
    );
  }
  if (!target || !details || details.path !== target) {
    return (
      <aside className="details-pane" data-embedded={embedded || undefined}>
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
      <div className="details-preview">
        <Thumb path={d.path} name={d.name} isDir={isDir} modified={d.modified} size={160} iconSize={72} />
      </div>
      <div className="details-title">
        <strong>{d.name}</strong>
        <span className="muted">{isDir ? "File folder" : typeLabel(d.name, false)}</span>
      </div>

      <dl className="details-list">
        <Row label="Location" value={d.parent} mono />
        {!isDir && <Row label="Size" value={`${formatSize(d.size)} (${formatCount(d.size)} bytes)`} />}
        {!isDir && d.sizeOnDisk != null && <Row label="Size on disk" value={formatSize(d.sizeOnDisk)} />}
        {isDir && <Row label="Contains" value={d.itemCount != null ? `${formatCount(d.itemCount)} items` : null} />}
        <Row label="Modified" value={formatDate(d.modified)} />
        <Row label="Created" value={formatDate(d.created)} />
        <Row label="Accessed" value={formatDate(d.accessed)} />
        <Row label="Content type" value={d.mime} />
        <Row label="Dimensions" value={d.image ? `${d.image[0]} × ${d.image[1]} px` : null} />
        <Row label="Link target" value={d.symlinkTarget} mono />
        <Row label="Read-only" value={d.readonly ? "Yes" : null} />
        <Row label="Hidden" value={d.hidden ? "Yes" : null} />
        <Row label="Attributes" value={d.attributes.join(", ")} />
        {d.unix && (
          <>
            <Row label="Permissions" value={`${d.unix.mode} (${d.unix.octal})`} mono />
            <Row label="Owner / group" value={`${d.unix.uid} / ${d.unix.gid}`} />
            <Row label="Hard links" value={d.unix.hardLinks > 1 ? d.unix.hardLinks : null} />
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

      {!isDir && (
        <div className="details-section">
          {hash === null ? (
            <button className="secondary" onClick={calcHash}>
              Compute SHA-256
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

      {isDir && (
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
