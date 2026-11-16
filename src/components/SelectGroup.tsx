// Select group / Unselect group (Total Commander's numpad + and −): select by name pattern, size, date and contents.

import { useEffect, useRef, useState } from "react";
import type { FileEntry } from "../api";
import { emptyGroup, groupMatches, parseSize, type GroupOpts } from "../util/selectGroup";
import { runTool } from "../tools/run";

const lastUsed: { opts: GroupOpts | null; history: string[] } = { opts: null, history: [] };

export function SelectGroup({ mode, entries, folder, onApply, onClose }: { mode: "select" | "deselect"; entries: FileEntry[]; folder: string; onApply: (names: string[]) => void; onClose: () => void }) {
  const [pattern, setPattern] = useState(lastUsed.opts?.pattern ?? "*");
  const [regex, setRegex] = useState(lastUsed.opts?.regex ?? false);
  const [cs, setCs] = useState(lastUsed.opts?.caseSensitive ?? false);
  const [folders, setFolders] = useState(lastUsed.opts?.includeFolders ?? false);
  const [min, setMin] = useState("");
  const [max, setMax] = useState("");
  const [newer, setNewer] = useState("");
  const [older, setOlder] = useState("");
  const [contains, setContains] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState("");
  const cancel = useRef<(() => void) | null>(null);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => input.current?.select(), []);

  const date = (s: string, endOfDay: boolean) => {
    if (!s) return null;
    const t = new Date(`${s}T${endOfDay ? "23:59:59" : "00:00:00"}`).getTime();
    return Number.isNaN(t) ? null : t;
  };
  const opts = (): GroupOpts => ({ pattern, regex, caseSensitive: cs, includeFolders: folders, minSize: parseSize(min), maxSize: parseSize(max), newerThan: date(newer, false), olderThan: date(older, true), contains: contains.trim() });
  const preview = groupMatches(entries, opts());
  const count = preview?.length ?? 0;

  const apply = async () => {
    const o = opts();
    let hits = groupMatches(entries, o);
    if (!hits) {
      setNote("The pattern is not a valid regular expression.");
      return;
    }
    if (o.contains) {
      setBusy(true);
      setNote("Looking inside the files…");
      const sep = folder.includes("\\") ? "\\" : "/";
      const roots = hits.filter((e) => !e.isDir).map((e) => folder.replace(/[\\/]$/, "") + sep + e.name);
      const found = new Set<string>();
      try {
        const run = runTool(
          { kind: "grep", roots, opts: { pattern: o.contains, regex: false, caseSensitive: o.caseSensitive, wholeWord: false, masks: "*", includeArchives: false, includeHidden: true, includeBinary: true, maxFileSize: 512 << 20, maxHits: 1_000_000 } },
          { hits: (h) => h.forEach((x) => found.add(x.path.slice(x.path.lastIndexOf(sep) + 1))) },
        );
        cancel.current = run.cancel;
        await run.result;
      } catch {
        /* cancelled or partial: use what was found */
      }
      cancel.current = null;
      hits = hits.filter((e) => found.has(e.name));
    }
    lastUsed.opts = o;
    onApply(hits.map((e) => e.name));
    onClose();
  };

  const title = mode === "select" ? "Select group" : "Unselect group";
  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div
        className="dialog select-group"
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            cancel.current?.();
            onClose();
          } else if (e.key === "Enter" && !busy) void apply();
          e.stopPropagation();
        }}
      >
        <h3>{title}</h3>
        <label className="field">
          <span>Names</span>
          <input ref={input} className="text-input" value={pattern} onChange={(e) => setPattern(e.target.value)} list="select-group-history" placeholder={regex ? "regular expression" : "*.jpg; *.png | thumb*"} aria-label="Names" />
          <datalist id="select-group-history">{lastUsed.history.map((h) => <option key={h} value={h} />)}</datalist>
        </label>
        <p className="muted">{regex ? "The whole name has to match." : "Wildcards * and ?. Separate several with ; and put names to leave out after a |."}</p>
        <div className="select-group-row">
          <label><input type="checkbox" checked={regex} onChange={(e) => setRegex(e.target.checked)} /> Regular expression</label>
          <label><input type="checkbox" checked={cs} onChange={(e) => setCs(e.target.checked)} /> Match case</label>
          <label><input type="checkbox" checked={folders} onChange={(e) => setFolders(e.target.checked)} /> Include folders</label>
        </div>
        <div className="select-group-grid">
          <label className="field"><span>Size at least</span><input className="text-input" value={min} onChange={(e) => setMin(e.target.value)} placeholder="10 KB" /></label>
          <label className="field"><span>Size at most</span><input className="text-input" value={max} onChange={(e) => setMax(e.target.value)} placeholder="2 MB" /></label>
          <label className="field"><span>Changed on or after</span><input className="text-input" type="date" value={newer} onChange={(e) => setNewer(e.target.value)} /></label>
          <label className="field"><span>Changed on or before</span><input className="text-input" type="date" value={older} onChange={(e) => setOlder(e.target.value)} /></label>
        </div>
        <label className="field">
          <span>Containing the text</span>
          <input className="text-input" value={contains} onChange={(e) => setContains(e.target.value)} placeholder="(leave empty to ignore the contents)" />
        </label>
        <p className="muted">{note || (preview ? `${count} of ${entries.length} items in this folder match.` : "The pattern is not a valid regular expression.")}</p>
        <div className="dialog-buttons">
          <button onClick={() => { setPattern("*"); setRegex(false); setCs(false); setFolders(false); setMin(""); setMax(""); setNewer(""); setOlder(""); setContains(""); }}>Reset</button>
          <span className="grow" />
          <button className="primary" disabled={busy || !preview} onClick={() => { if (pattern && !lastUsed.history.includes(pattern)) lastUsed.history = [pattern, ...lastUsed.history].slice(0, 20); void apply(); }}>{mode === "select" ? "Select" : "Unselect"}</button>
          <button onClick={() => { cancel.current?.(); onClose(); }}>Cancel</button>
        </div>
      </div>
    </div>
  );
}

export { emptyGroup };
