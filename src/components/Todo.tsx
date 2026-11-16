// A to-do list and notes for each folder (OneCommander's integrated to-do). Kept on this computer, by folder path.

import { useEffect, useRef, useState } from "react";
import { useStore } from "../store";
import { baseName } from "../util/path";
import { Icon } from "./Icons";

export interface TodoItem {
  id: string;
  text: string;
  done: boolean;
}
export interface FolderNotes {
  items: TodoItem[];
  note: string;
}

const KEY = "fast.todo";

export function loadNotes(): Record<string, FolderNotes> {
  try {
    return JSON.parse(localStorage.getItem(KEY) ?? "{}") as Record<string, FolderNotes>;
  } catch {
    return {};
  }
}
function saveNotes(all: Record<string, FolderNotes>) {
  // Folders with nothing left are forgotten.
  const kept = Object.fromEntries(Object.entries(all).filter(([, v]) => v.items.length || v.note.trim()));
  try {
    localStorage.setItem(KEY, JSON.stringify(kept));
  } catch {
    /* not saved */
  }
}

/** How many things are still to do in a folder. */
export const openCount = (path: string) => loadNotes()[path]?.items.filter((i) => !i.done).length ?? 0;

export function TodoDialog() {
  const path = useStore((s) => s.todoFor);
  return path ? <TodoBody path={path} /> : null;
}

function TodoBody({ path }: { path: string }) {
  const close = () => useStore.setState({ todoFor: null });
  const [data, setData] = useState<FolderNotes>(() => loadNotes()[path] ?? { items: [], note: "" });
  const [text, setText] = useState("");
  const box = useRef<HTMLInputElement>(null);
  useEffect(() => box.current?.focus(), []);
  const commit = (next: FolderNotes) => {
    setData(next);
    const all = loadNotes();
    all[path] = next;
    saveNotes(all);
  };
  const add = () => {
    const t = text.trim();
    if (!t) return;
    commit({ ...data, items: [...data.items, { id: Math.random().toString(36).slice(2, 9), text: t, done: false }] });
    setText("");
  };
  const left = data.items.filter((i) => !i.done).length;
  return (
    <div className="modal-backdrop" onMouseDown={close}>
      <div className="dialog todo-dialog" role="dialog" aria-modal="true" aria-label="To-do for this folder" onMouseDown={(e) => e.stopPropagation()} onKeyDown={(e) => { if (e.key === "Escape") close(); e.stopPropagation(); }}>
        <div className="tools-head"><h3>To-do: {baseName(path) || path}</h3><span className="grow" /><button className="icon-button" aria-label="Close" onClick={close}><Icon name="close" size={14} /></button></div>
        <p className="muted">{left ? `${left} still to do` : data.items.length ? "All done." : "Nothing here yet."} Kept on this computer for {path}.</p>
        <div className="todo-add">
          <input ref={box} className="text-input" value={text} placeholder="Add something to do and press Enter" aria-label="New item" onChange={(e) => setText(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") add(); }} />
          <button className="primary" onClick={add} disabled={!text.trim()}>Add</button>
        </div>
        <div className="todo-list" role="list">
          {data.items.map((i) => (
            <div key={i.id} className="todo-item" role="listitem" data-done={i.done || undefined}>
              <input type="checkbox" checked={i.done} aria-label={i.text} onChange={() => commit({ ...data, items: data.items.map((x) => (x.id === i.id ? { ...x, done: !x.done } : x)) })} />
              <span className="todo-text">{i.text}</span>
              <button className="icon-button" aria-label={`Remove ${i.text}`} onClick={() => commit({ ...data, items: data.items.filter((x) => x.id !== i.id) })}><Icon name="close" size={12} /></button>
            </div>
          ))}
        </div>
        {data.items.some((i) => i.done) && <button className="secondary" onClick={() => commit({ ...data, items: data.items.filter((i) => !i.done) })}>Clear the finished ones</button>}
        <label className="field"><span className="muted">Notes</span><textarea className="text-input todo-note" rows={4} value={data.note} onChange={(e) => commit({ ...data, note: e.target.value })} placeholder="Anything to remember about this folder" /></label>
      </div>
    </div>
  );
}
