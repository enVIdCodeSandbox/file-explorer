// Two more ways to look at folders, added next to the normal view and off by default:
//  - a second pane beside the file list (Total Commander style: copy and move between the two panes),
//  - Miller columns (OneCommander / Finder style: one column per folder level, a preview at the end).

import { commandsFor } from "../settings/keys";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type FileEntry } from "../api";
import { deletePaths, openFile, useStore } from "../store";
import { baseName, joinPath, parentPath } from "../util/path";
import { formatDate, formatSize } from "../util/format";
import { FileIcon, Icon } from "./Icons";
import { Thumb } from "./Thumb";
import { beginDrag } from "../dnd";
import { PaneTabs } from "./PaneTabs";
import { cyclePane, focusPane, navigatePane, otherPane, stepPane, useSplit } from "../split/state";

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

// ---- The extra panes ----------------------------------------------------------------------------------

/** An extra pane of the split view (slot 1 to 7); the normal file list is slot 0. The folder is the one in its open tab. */
export function SplitPane({ slot, leftFolder }: { slot: number; leftFolder: string }) {
  const tab = useSplit((s) => {
    const p = s.panes[slot];
    return p ? (p.tabs.find((t) => t.id === p.active) ?? p.tabs[0]) : undefined;
  });
  const path = tab?.path ?? leftFolder;
  const tabId = tab?.id ?? "";
  const [typed, setTyped] = useState(path);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [anchor, setAnchor] = useState<string | null>(null);
  const { entries, error, loading } = useFolder(path);
  const root = useRef<HTMLDivElement>(null);
  const store = useStore.getState;

  // A different tab or folder starts with nothing chosen and the new address in the box.
  useEffect(() => {
    setTyped(path);
    setPicked(new Set());
    setAnchor(null);
  }, [tabId, path]);
  const go = useCallback((p: string) => navigatePane(slot, p), [slot]);
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

  // "The other pane" is the one that had the focus before this one (the normal file list to begin with).
  const target = () => otherPane(slot, leftFolder);
  const transfer = (op: "copy" | "move", from: "this" | "main") => {
    const other = target();
    const sources = from === "this" ? chosen : store().selection;
    const dest = from === "this" ? other.path : path;
    if (!sources.length) {
      store().showToast(from === "this" ? "Select something in this pane first." : "Select something in the main file list first.");
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
    const cmds = commandsFor(useStore.getState().settings, e.nativeEvent);
    const is = (id: string) => cmds.includes(id);
    if (is("cursorDown")) move(1);
    else if (is("cursorUp")) move(-1);
    else if (is("cursorFirst")) entries?.[0] && (setAnchor(entries[0].name), setPicked(new Set([entries[0].name])));
    else if (is("cursorLast") && entries?.length) (setAnchor(entries.at(-1)!.name), setPicked(new Set([entries.at(-1)!.name])));
    else if (is("open") && entries?.[i]) open(entries[i]);
    else if (is("goUpList")) up();
    else if (is("selectAll")) setPicked(new Set((entries ?? []).map((x) => x.name)));
    else if (is("copyToOtherPane")) transfer("copy", "this");
    else if (is("moveToOtherPane")) transfer("move", "this");
    else if (is("delete") && chosen.length) void deletePaths(chosen, e.shiftKey).then(() => store().bumpDirs([path]));
    else if (is("nextPane")) cyclePane(slot, 1);
    else if (is("prevPane")) cyclePane(slot, -1);
    else if (is("back")) stepPane(slot, -1);
    else if (is("forward")) stepPane(slot, 1);
    else if (is("view") && entries?.[i] && !entries[i].isDir) store().openViewer(full(entries[i].name), { siblings: entries.filter((x) => !x.isDir).map((x) => full(x.name)) });
    else return;
    e.preventDefault();
    e.stopPropagation();
  };

  return (
    <div ref={root} className="second-pane" tabIndex={0} data-split-pane={slot} onKeyDown={onKeyDown} onFocusCapture={() => focusPane(slot)} onMouseDownCapture={() => focusPane(slot)} aria-label={`Pane ${slot + 1}`}>
      <PaneTabs slot={slot} />
      <div className="second-pane-bar">
        <button className="icon-button" aria-label="Back" title="Back (Alt+Left)" disabled={!tab?.back.length} onClick={() => stepPane(slot, -1)}><Icon name="back" size={14} /></button>
        <button className="icon-button" aria-label="Forward" title="Forward (Alt+Right)" disabled={!tab?.fwd.length} onClick={() => stepPane(slot, 1)}><Icon name="forward" size={14} /></button>
        <button className="icon-button" aria-label="Up one level" title="Up (Backspace)" disabled={!parentPath(path)} onClick={up}><Icon name="up" size={14} /></button>
        <input className="text-input second-pane-path" aria-label={`Folder of pane ${slot + 1}`} value={typed} spellCheck={false} onChange={(e) => setTyped(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") { e.stopPropagation(); go(typed); } else e.stopPropagation(); }} />
        <button className="icon-button" aria-label="Show the folder of the main file list" title="Same folder as the main file list" onClick={() => go(leftFolder)}>=</button>
      </div>
      <div className="second-pane-actions">
        <button title="Copy what is selected in the main file list into this pane's folder" onClick={() => transfer("copy", "main")}>← Copy</button>
        <button title="Move what is selected in the main file list into this pane's folder" onClick={() => transfer("move", "main")}>← Move</button>
        <span className="grow" />
        <button title="Copy this pane's selection to the other pane (F5)" onClick={() => transfer("copy", "this")}>Copy →</button>
        <button title="Move this pane's selection to the other pane (F6)" onClick={() => transfer("move", "this")}>Move →</button>
      </div>
      <div className="second-pane-list" role="listbox" aria-multiselectable="true" aria-label="Items" data-drop="folder" data-drop-path={path}>
        {loading && <div className="loading-bar" />}
        {error && <div className="empty-state"><span>{error}</span></div>}
        {entries?.map((e) => (
          <div key={e.name} className="second-pane-row" role="option" aria-selected={picked.has(e.name)} data-selected={picked.has(e.name) || undefined} onClick={(ev) => click(e, ev)} onDoubleClick={() => open(e)}
            data-drop={e.isDir ? "folder" : undefined} data-drop-path={e.isDir ? full(e.name) : undefined}
            onPointerDown={(ev) => beginDrag(ev, () => {
              const names = picked.has(e.name) ? [...picked] : [e.name];
              const items = (entries ?? []).filter((x) => names.includes(x.name));
              return { paths: items.map((x) => full(x.name)), dirs: items.map((x) => x.isDir), label: items.length === 1 ? items[0].name : `${items.length} items` };
            })}>
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
    const cmds = commandsFor(useStore.getState().settings, ev.nativeEvent);
    const is = (id: string) => cmds.includes(id);
    if (is("cursorDown") && list.length) pick(focusCol, list[Math.min(list.length - 1, i + 1)]);
    else if (is("cursorUp") && list.length) pick(focusCol, list[Math.max(0, i - 1)]);
    else if (is("cursorRight")) {
      const next = cols[focusCol + 1];
      if (next) {
        setFocusCol(focusCol + 1);
        const first = (await api.listDir(next).catch(() => null))?.entries.filter((x) => useStore.getState().settings?.general.showHidden || !x.isHidden).sort(byName)[0];
        if (first && !chosen[next]) pick(focusCol + 1, first);
      }
    } else if (is("cursorLeft") && focusCol > 0) setFocusCol(focusCol - 1);
    else if (is("open") && list[i]) open(focusCol, list[i]);
    else if (is("view") && file) useStore.getState().openViewer(joinPath(here, file.name), { siblings: [joinPath(here, file.name)] });
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
