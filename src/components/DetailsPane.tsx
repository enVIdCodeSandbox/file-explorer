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
        <div className="details-title">
          <strong>{selection.length} items selected</strong>
        </div>
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
