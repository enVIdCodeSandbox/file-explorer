// Two more ways to look at folders, added next to the normal view and off by default:
//  - a second pane beside the file list (Total Commander style: copy and move between the two panes),
//  - Miller columns (OneCommander / Finder style: one column per folder level, a preview at the end).

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type FileEntry } from "../api";
import { deletePaths, openFile, useStore } from "../store";
import { baseName, isArchiveLoc, isRemote, joinPath, parentPath } from "../util/path";
import { formatDate, formatSize } from "../util/format";
import { FileIcon, Icon } from "./Icons";
import { Thumb } from "./Thumb";

/** The folders from the top down to `path`: C:\, C:\a, C:\a\b. */
export function ancestors(path: string): string[] {
  const out: string[] = [];
  let cur: string | null = path;
  while (cur) {
    out.unshift(cur);
    const up: string | null = parentPath(cur);
    if (!up || up === cur) break;
    cur = up;
  }
  return out;
}

const byName = (a: FileEntry, b: FileEntry) => (a.isDir === b.isDir ? a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" }) : a.isDir ? -1 : 1);

function useFolder(path: string | null) {
  const version = useStore((s) => (path ? s.dirVersion[path] ?? 0 : 0));
  const hidden = useStore((s) => s.settings?.general.showHidden ?? false);
  const [entries, setEntries] = useState<FileEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!path) return;
    let alive = true;
    setError(null);
    api
      .listDir(path)
      .then((l) => alive && setEntries(l.entries.filter((e) => hidden || !e.isHidden).sort(byName)))
      .catch((e) => alive && (setEntries([]), setError(String(e).replace(/^Error: /, ""))));
    return () => {
      alive = false;
    };
  }, [path, version, hidden]);
  return { entries, error, loading: !!path && entries === null };
}

// ---- The second pane ----------------------------------------------------------------------------------

const SECOND_KEY = "fast.secondPane";

export function SecondPane({ leftFolder }: { leftFolder: string }) {
  const [path, setPath] = useState<string>(() => {
    try {
      return localStorage.getItem(SECOND_KEY) || leftFolder;
    } catch {
      return leftFolder;
    }
  });
  const [typed, setTyped] = useState(path);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [anchor, setAnchor] = useState<string | null>(null);
  const { entries, error, loading } = useFolder(path);
  const root = useRef<HTMLDivElement>(null);
  const store = useStore.getState;

  const go = useCallback((p: string) => {
    setPath(p);
    setTyped(p);
    setPicked(new Set());
    setAnchor(null);
    try {
      localStorage.setItem(SECOND_KEY, p);
    } catch {
      /* not saved */
    }
  }, []);
  const up = () => {
    const p = parentPath(path);
    if (p) go(p);
  };
  const full = (n: string) => joinPath(path, n);
  const chosen = [...picked].map(full);

  const open = (e: FileEntry) => (e.isDir ? go(full(e.name)) : void openFile(full(e.name)));
  const click = (e: FileEntry, ev: React.MouseEvent) => {
    root.current?.focus();
    if (ev.shiftKey && anchor && entries) {
      const a = entries.findIndex((x) => x.name === anchor);
      const b = entries.findIndex((x) => x.name === e.name);
      setPicked(new Set(entries.slice(Math.min(a, b), Math.max(a, b) + 1).map((x) => x.name)));
    } else if (ev.ctrlKey || ev.metaKey) {
      setPicked((s) => {
        const n = new Set(s);
        n.has(e.name) ? n.delete(e.name) : n.add(e.name);
        return n;
      });
      setAnchor(e.name);
    } else {
      setPicked(new Set([e.name]));
      setAnchor(e.name);
    }
  };

  const transfer = (op: "copy" | "move", from: "this" | "other") => {
    const other = (() => {
      const l = store().tabs.find((t) => t.id === store().activeTab)?.location;
      return l && !isRemote(l) && !isArchiveLoc(l) ? l : leftFolder;
    })();
    const sources = from === "this" ? chosen : store().selection;
    const dest = from === "this" ? other : path;
    if (!sources.length) {
      store().showToast(from === "this" ? "Select something in the right pane first." : "Select something in the left pane first.");
      return;
    }
    void store().startTransfer(sources, dest, op);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if ((e.target as HTMLElement).tagName === "INPUT") return;
    const i = entries ? entries.findIndex((x) => x.name === anchor) : -1;
    const move = (d: number) => {
      if (!entries?.length) return;
      const n = entries[Math.max(0, Math.min(entries.length - 1, (i < 0 ? (d > 0 ? -1 : entries.length) : i) + d))];
      setAnchor(n.name);
      setPicked(e.shiftKey ? new Set([...picked, n.name]) : new Set([n.name]));
    };
    if (e.key === "ArrowDown") move(1);
    else if (e.key === "ArrowUp") move(-1);
    else if (e.key === "Home") entries?.[0] && (setAnchor(entries[0].name), setPicked(new Set([entries[0].name])));
    else if (e.key === "End" && entries?.length) (setAnchor(entries.at(-1)!.name), setPicked(new Set([entries.at(-1)!.name])));
    else if (e.key === "Enter" && entries?.[i]) open(entries[i]);
    else if (e.key === "Backspace") up();
    else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a") setPicked(new Set((entries ?? []).map((x) => x.name)));
    else if (e.key === "F5") transfer("copy", "this");
    else if (e.key === "F6") transfer("move", "this");
    else if (e.key === "Delete" && chosen.length) void deletePaths(chosen, e.shiftKey).then(() => store().bumpDirs([path]));
    else if (e.key === "Tab") document.querySelector<HTMLElement>(".file-scroller")?.focus();
    else if (e.key === "F3" && entries?.[i] && !entries[i].isDir) store().openViewer(full(entries[i].name), { siblings: entries.filter((x) => !x.isDir).map((x) => full(x.name)) });
    else return;
    e.preventDefault();
    e.stopPropagation();
  };

  return (
    <div ref={root} className="second-pane" tabIndex={0} onKeyDown={onKeyDown} aria-label="Second pane">
      <div className="second-pane-bar">
        <button className="icon-button" aria-label="Up one level" title="Up (Backspace)" disabled={!parentPath(path)} onClick={up}><Icon name="up" size={14} /></button>
        <input className="text-input second-pane-path" aria-label="Folder of the second pane" value={typed} spellCheck={false} onChange={(e) => setTyped(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") { e.stopPropagation(); go(typed); } else e.stopPropagation(); }} />
        <button className="icon-button" aria-label="Show the folder of the left pane" title="Same folder as the left pane" onClick={() => go(leftFolder)}>=</button>
        <button className="icon-button" aria-label="Close the second pane" title="Close the second pane" onClick={() => useStore.getState().updateSettings((s) => void (s.advanced["view.dualPane"] = false))}><Icon name="close" size={14} /></button>
      </div>
      <div className="second-pane-actions">
        <button title="Copy the left pane's selection here" onClick={() => transfer("copy", "other")}>← Copy</button>
        <button title="Move the left pane's selection here" onClick={() => transfer("move", "other")}>← Move</button>
        <span className="grow" />
        <button title="Copy this pane's selection to the left folder (F5)" onClick={() => transfer("copy", "this")}>Copy →</button>
        <button title="Move this pane's selection to the left folder (F6)" onClick={() => transfer("move", "this")}>Move →</button>
      </div>
      <div className="second-pane-list" role="listbox" aria-multiselectable="true" aria-label="Items">
        {loading && <div className="loading-bar" />}
        {error && <div className="empty-state"><span>{error}</span></div>}
        {entries?.map((e) => (
          <div key={e.name} className="second-pane-row" role="option" aria-selected={picked.has(e.name)} data-selected={picked.has(e.name) || undefined} onClick={(ev) => click(e, ev)} onDoubleClick={() => open(e)}>
            <FileIcon kind={e.isDir ? "folder" : "file"} name={e.name} size={16} />
            <span className="second-pane-name" title={e.name}>{e.name}</span>
            <span className="muted second-pane-size">{e.isDir ? "" : formatSize(e.size)}</span>
            <span className="muted second-pane-date">{e.modified ? formatDate(e.modified) : ""}</span>
          </div>
        ))}
        {entries && entries.length === 0 && !error && <div className="empty-state"><span>This folder is empty.</span></div>}
      </div>
      <div className="second-pane-status muted">{entries ? `${entries.length} items${picked.size ? `, ${picked.size} selected` : ""}` : ""}</div>
    </div>
  );
}

// ---- Miller columns -------------------------------------------------------------------------------------------

function Column({ path, chosen, focus, onPick, onOpen }: { path: string; chosen: string | null; focus: boolean; onPick: (e: FileEntry) => void; onOpen: (e: FileEntry) => void }) {
  const { entries, error, loading } = useFolder(path);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => ref.current?.querySelector<HTMLElement>("[data-chosen]")?.scrollIntoView({ block: "nearest" }), [chosen, entries]);
  return (
    <div ref={ref} className="miller-col" data-focus={focus || undefined} role="listbox" aria-label={baseName(path) || path}>
      {loading && <div className="loading-bar" />}
      {error && <div className="empty-state"><span>{error}</span></div>}
      {entries?.map((e) => (
        <div key={e.name} className="miller-row" role="option" aria-selected={chosen === e.name} data-chosen={chosen === e.name || undefined} onClick={() => onPick(e)} onDoubleClick={() => onOpen(e)}>
          <FileIcon kind={e.isDir ? "folder" : "file"} name={e.name} size={16} />
          <span className="miller-name" title={e.name}>{e.name}</span>
          {e.isDir && <Icon name="expand" size={9} />}
        </div>
      ))}
      {entries && entries.length === 0 && !error && <div className="empty-state"><span>Empty</span></div>}
    </div>
  );
}

export function MillerColumns({ path }: { path: string }) {
  const navigate = useStore((s) => s.navigate);
  // The columns: every folder from the top down to the one being viewed, then whatever was opened beyond it.
  const base = useMemo(() => ancestors(path), [path]);
  const [deeper, setDeeper] = useState<string[]>([]);
  const [file, setFile] = useState<FileEntry | null>(null);
  const [focusCol, setFocusCol] = useState(0);
  const [chosen, setChosen] = useState<Record<string, string>>({});
  const root = useRef<HTMLDivElement>(null);
  const strip = useRef<HTMLDivElement>(null);
  const cols = [...base, ...deeper];
  useEffect(() => {
    setDeeper([]);
    setFile(null);
    setFocusCol(base.length - 1);
    setChosen(Object.fromEntries(base.slice(0, -1).map((p, i) => [p, baseName(base[i + 1])])));
  }, [path]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => void (strip.current && (strip.current.scrollLeft = strip.current.scrollWidth)), [cols.length, file]);

  const pick = (col: number, e: FileEntry) => {
    const here = cols[col];
    const next = joinPath(here, e.name);
    root.current?.focus();
    setFocusCol(col);
    setChosen((c) => ({ ...c, [here]: e.name }));
    const keep = cols.slice(0, col + 1);
    if (e.isDir) {
      setFile(null);
      const rest = keep.length > base.length ? keep.slice(base.length) : [];
      setDeeper([...rest, next]);
      useStore.getState().setSelection([next]);
    } else {
      setFile(e);
      setDeeper(keep.length > base.length ? keep.slice(base.length) : []);
      useStore.getState().setSelection([next]);
    }
  };
  const open = (col: number, e: FileEntry) => (e.isDir ? navigate(joinPath(cols[col], e.name)) : void openFile(joinPath(cols[col], e.name)));

  const onKeyDown = async (ev: React.KeyboardEvent) => {
    const here = cols[focusCol];
    if (!here) return;
    const list = (await api.listDir(here).catch(() => null))?.entries.filter((x) => useStore.getState().settings?.general.showHidden || !x.isHidden).sort(byName) ?? [];
    const i = list.findIndex((x) => x.name === chosen[here]);
    if (ev.key === "ArrowDown" && list.length) pick(focusCol, list[Math.min(list.length - 1, i + 1)]);
    else if (ev.key === "ArrowUp" && list.length) pick(focusCol, list[Math.max(0, i - 1)]);
    else if (ev.key === "ArrowRight") {
      const next = cols[focusCol + 1];
      if (next) {
        setFocusCol(focusCol + 1);
        const first = (await api.listDir(next).catch(() => null))?.entries.filter((x) => useStore.getState().settings?.general.showHidden || !x.isHidden).sort(byName)[0];
        if (first && !chosen[next]) pick(focusCol + 1, first);
      }
    } else if (ev.key === "ArrowLeft" && focusCol > 0) setFocusCol(focusCol - 1);
    else if (ev.key === "Enter" && list[i]) open(focusCol, list[i]);
    else if (ev.key === "F3" && file) useStore.getState().openViewer(joinPath(here, file.name), { siblings: [joinPath(here, file.name)] });
    else return;
    ev.preventDefault();
    ev.stopPropagation();
  };

  const filePath = file ? joinPath(cols[Math.min(cols.length - 1, focusCol)], file.name) : null;
  return (
    <div ref={root} className="miller" tabIndex={0} onKeyDown={(e) => void onKeyDown(e)} aria-label="Columns">
      <div ref={strip} className="miller-strip">
        {cols.map((p, i) => (
          <Column key={p} path={p} chosen={chosen[p] ?? null} focus={focusCol === i} onPick={(e) => pick(i, e)} onOpen={(e) => open(i, e)} />
        ))}
        {file && filePath && (
          <div className="miller-preview" aria-label="Preview">
            <div className="miller-preview-pic"><Thumb path={filePath} name={file.name} isDir={false} modified={file.modified} size={200} iconSize={72} view="xl" /></div>
            <strong className="miller-preview-name">{file.name}</strong>
            <span className="muted">{formatSize(file.size)}</span>
            <span className="muted">{file.modified ? formatDate(file.modified) : ""}</span>
            <div className="miller-preview-buttons">
              <button onClick={() => void openFile(filePath)}>Open</button>
              <button onClick={() => useStore.getState().openViewer(filePath, { siblings: [filePath] })}>View</button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
