// File tools in one window (like Total Commander's Files and Commands menus): compare files and folders,
// find duplicates, split and combine, encode and decode, convert pictures, search inside files.

import { useEffect, useMemo, useRef, useState } from "react";
import { type ConvertResult, type DirCompare, type DirDiff, type DupGroup, type FileCompare, type GrepHit, type GrepSummary } from "../api";
import { activeLocation, deletePaths, useStore } from "../store";
import { runTool, wasCancelled } from "../tools/run";
import { joinPath } from "../util/userCommands";
import { archiveUrl } from "../util/path";
import { parseSize } from "../util/selectGroup";
import { Icon } from "./Icons";

export type ToolTab = "compareFiles" | "compareDirs" | "duplicates" | "split" | "encode" | "convert" | "grep";
const TABS: [ToolTab, string][] = [
  ["compareFiles", "Compare files"],
  ["compareDirs", "Compare folders"],
  ["duplicates", "Duplicates"],
  ["split", "Split / combine"],
  ["encode", "Encode / decode"],
  ["convert", "Convert pictures"],
  ["grep", "Search in files"],
];

const sepOf = (p: string) => (p.includes("\\") ? "\\" : "/");
const baseName = (p: string) => p.slice(p.search(/[^\\/]*$/));
const dirName = (p: string) => {
  const i = p.search(/[\\/][^\\/]*$/);
  if (i < 0) return p;
  const d = p.slice(0, i);
  return d === "" ? p.slice(0, 1) : /^[A-Za-z]:$/.test(d) ? d + p[i] : d;
};
const fmtSize = (n: number) => (n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} KB` : n < 1073741824 ? `${(n / 1048576).toFixed(1)} MB` : `${(n / 1073741824).toFixed(2)} GB`);
const fmtDate = (ms: number) => (ms ? new Date(ms).toLocaleString() : "");

export function Tools() {
  const req = useStore((s) => s.tools);
  return req ? <ToolsBody key={req.serial} /> : null;
}

function ToolsBody() {
  const req = useStore((s) => s.tools)!;
  const [tab, setTab] = useState<ToolTab>(req.tab);
  const close = () => useStore.setState({ tools: null });
  const folder = req.folder;
  const [running, setRunning] = useState<{ cancel: () => void; text: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => root.current?.focus(), []);

  // Every tool runs through here: one at a time, with a way to stop it.
  async function go<T>(spec: Parameters<typeof runTool>[0], handlers: Parameters<typeof runTool>[1], done: (r: T) => void) {
    setError(null);
    const run = runTool<T>(spec, {
      ...handlers,
      progress: (d, t, note) => {
        setRunning((r) => (r ? { ...r, text: t ? `${d.toLocaleString()} of ${t.toLocaleString()}  ${note}` : note } : r));
        handlers?.progress?.(d, t, note);
      },
    });
    setRunning({ cancel: run.cancel, text: "Working…" });
    try {
      done(await run.result);
    } catch (e) {
      if (!wasCancelled(e)) setError(String(e).replace(/^Error: /, ""));
    } finally {
      setRunning(null);
    }
  }

  const ctx = { folder, paths: req.paths, go, busy: !!running, setError };
  return (
    <div className="modal-backdrop" onMouseDown={close}>
      <div ref={root} className="dialog tools-dialog" role="dialog" aria-modal="true" aria-label="File tools" tabIndex={-1} onMouseDown={(e) => e.stopPropagation()} onKeyDown={(e) => { if (e.key === "Escape") { running?.cancel(); close(); } e.stopPropagation(); }}>
        <div className="tools-head">
          <h3>File tools</h3>
          <span className="grow" />
          <button className="icon-button" aria-label="Close" onClick={() => { running?.cancel(); close(); }}><Icon name="close" size={14} /></button>
        </div>
        <div className="tools-tabs" role="tablist">
          {TABS.map(([id, label]) => <button key={id} role="tab" aria-selected={tab === id} data-on={tab === id || undefined} onClick={() => setTab(id)}>{label}</button>)}
        </div>
        <div className="tools-body">
          {tab === "compareFiles" && <CompareFiles {...ctx} />}
          {tab === "compareDirs" && <CompareDirs {...ctx} />}
          {tab === "duplicates" && <Duplicates {...ctx} />}
          {tab === "split" && <SplitJoin {...ctx} />}
          {tab === "encode" && <EncodeDecode {...ctx} />}
          {tab === "convert" && <Convert {...ctx} />}
          {tab === "grep" && <Grep {...ctx} />}
        </div>
        <div className="tools-foot">
          {running ? (<><div className="loading-bar tools-progress" /><span className="muted tools-note">{running.text}</span><span className="grow" /><button onClick={running.cancel}>Stop</button></>) : error ? (<span className="tools-error" role="alert">{error}</span>) : null}
        </div>
      </div>
    </div>
  );
}

interface Ctx {
  folder: string;
  paths: string[];
  go: <T>(spec: Parameters<typeof runTool>[0], handlers: Parameters<typeof runTool>[1], done: (r: T) => void) => Promise<void>;
  busy: boolean;
  setError: (e: string | null) => void;
}

function Field({ label, value, onChange, placeholder, wide }: { label: string; value: string; onChange: (v: string) => void; placeholder?: string; wide?: boolean }) {
  return <label className="field" data-wide={wide || undefined}><span>{label}</span><input className="text-input" value={value} placeholder={placeholder} onChange={(e) => onChange(e.target.value)} spellCheck={false} /></label>;
}
function Check({ label, value, onChange }: { label: string; value: boolean; onChange: (v: boolean) => void }) {
  return <label className="tools-check"><input type="checkbox" checked={value} onChange={(e) => onChange(e.target.checked)} /> {label}</label>;
}

// ---- Compare files -----------------------------------------------------------------------------

function CompareFiles({ folder, paths, go, busy }: Ctx) {
  const files = paths.filter((p) => !/[\\/]$/.test(p));
  const [a, setA] = useState(files[0] ?? "");
  const [b, setB] = useState(files[1] ?? "");
  const [ws, setWs] = useState(false);
  const [ci, setCi] = useState(false);
  const [out, setOut] = useState<FileCompare | null>(null);
  const [onlyDiff, setOnlyDiff] = useState(false);
  const run = () => go<FileCompare>({ kind: "compareFiles", a, b, opts: { ignoreWhitespace: ws, ignoreCase: ci } }, {}, setOut);
  useEffect(() => { if (a && b) run(); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const rows = out?.result === "text" ? (onlyDiff ? out.rows.filter((r) => r.kind !== "same") : out.rows) : [];
  return (
    <>
      <div className="tools-grid">
        <Field label="First file" value={a} onChange={setA} placeholder={joinPath(folder, "a.txt")} wide />
        <Field label="Second file" value={b} onChange={setB} wide />
      </div>
      <div className="tools-row">
        <Check label="Ignore white space" value={ws} onChange={setWs} />
        <Check label="Ignore case" value={ci} onChange={setCi} />
        <Check label="Show only the differences" value={onlyDiff} onChange={setOnlyDiff} />
        <span className="grow" />
        <button className="primary" disabled={busy || !a || !b} onClick={run}>Compare</button>
      </div>
      {out?.result === "identical" && <p className="tools-result">The files are identical ({fmtSize(out.size)}).</p>}
      {out?.result === "binary" && <p className="tools-result">The files differ ({fmtSize(out.aSize)} and {fmtSize(out.bSize)}){out.firstDiff !== null ? `, first at byte ${out.firstDiff.toLocaleString()} (0x${out.firstDiff.toString(16).toUpperCase()})` : ""}. They are not text, so no lines are shown.</p>}
      {out?.result === "text" && (
        <>
          <p className="tools-result">{out.changed + out.added + out.removed === 0 ? "The files have the same text." : `${out.changed} changed, ${out.added} added, ${out.removed} removed lines.`}{out.truncated ? " Only the first part is shown." : ""}</p>
          <div className="diff-table" role="table" aria-label="Differences">
            {rows.map((r, i) => (
              <div key={i} className="diff-row" data-kind={r.kind} role="row">
                <span className="diff-no">{r.aNo ?? ""}</span><span className="diff-text">{r.a}</span>
                <span className="diff-no">{r.bNo ?? ""}</span><span className="diff-text">{r.b}</span>
              </div>
            ))}
          </div>
        </>
      )}
    </>
  );
}

// ---- Compare and synchronize folders --------------------------------------------------------------

const STATUS: Record<DirDiff["status"], string> = { leftOnly: "Only on the left", rightOnly: "Only on the right", leftNewer: "Left is newer", rightNewer: "Right is newer", different: "Different (same date)", typeMismatch: "File on one side, folder on the other" };

function CompareDirs({ folder, paths, go, busy, setError }: Ctx) {
  // Two things selected (folders or archives): those; one: it against the folder being viewed.
  const picked = paths.filter((p) => p !== folder);
  const [left, setLeft] = useState(picked.length >= 2 ? picked[0] : folder);
  const [right, setRight] = useState(picked.length >= 2 ? picked[1] : (picked[0] ?? ""));
  const [recursive, setRecursive] = useState(true);
  const [content, setContent] = useState(false);
  const [hidden, setHidden] = useState(false);
  const [out, setOut] = useState<DirCompare | null>(null);
  const [ticked, setTicked] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState<DirDiff["status"] | "all">("all");
  const run = () => go<DirCompare>({ kind: "compareDirs", left, right, opts: { recursive, byContent: content, includeHidden: hidden } }, {}, (r) => {
    setOut(r);
    // Ready to synchronize: what is missing on a side or older there is ticked.
    setTicked(new Set(r.entries.filter((e) => e.status !== "different" && e.status !== "typeMismatch").map((e) => e.rel)));
  });
  const shown = (out?.entries ?? []).filter((e) => filter === "all" || e.status === filter);

  // Copy the ticked items that are missing or older on the other side. Folders are only copied when they are
  // missing; those that exist on both sides are walked by the comparison itself.
  const sync = async (dir: "right" | "left" | "both") => {
    if (!out) return;
    try {
      const batches = new Map<string, string[]>();
      // The folders really compared: an archive on a side was unpacked to a temporary folder. Copying out of it
      // uses the unpacked files; copying into it goes in as "add to the archive".
      const roots = { left: out.leftRoot ?? left, right: out.rightRoot ?? right };
      const archives = { left: out.leftArchive ?? null, right: out.rightArchive ?? null };
      for (const e of out.entries) {
        if (!ticked.has(e.rel)) continue;
        const toRight = e.status === "leftOnly" || e.status === "leftNewer";
        const toLeft = e.status === "rightOnly" || e.status === "rightNewer";
        if ((toRight && dir !== "left") || (toLeft && dir !== "right")) {
          const [from, to] = toRight ? (["left", "right"] as const) : (["right", "left"] as const);
          const src = roots[from];
          const s = sepOf(src);
          const rel = e.rel.replace(/[\\/]/g, s);
          const relDir = rel.includes(s) ? rel.slice(0, rel.lastIndexOf(s)) : "";
          const destFolder = archives[to] ? archiveUrl(archives[to]!, relDir.replace(/[\\/]/g, "/")) : roots[to] + (relDir ? sepOf(roots[to]) + relDir.replace(/[\\/]/g, sepOf(roots[to])) : "");
          const list = batches.get(destFolder) ?? [];
          list.push(src.replace(/[\\/]$/, "") + s + rel);
          batches.set(destFolder, list);
        }
      }
      if (!batches.size) {
        useStore.getState().showToast("Nothing ticked needs copying in that direction.");
        return;
      }
      for (const [dest, items] of batches) await useStore.getState().startTransfer(items, dest, "copy");
      useStore.getState().showToast("Copying started. Compare again when it is done.");
    } catch (e) {
      setError(String(e));
    }
  };
  const toggle = (rel: string) => setTicked((t) => { const n = new Set(t); n.has(rel) ? n.delete(rel) : n.add(rel); return n; });
  return (
    <>
      <div className="tools-grid">
        <Field label="Left folder or archive (zip, 7z…)" value={left} onChange={setLeft} wide />
        <Field label="Right folder or archive" value={right} onChange={setRight} wide />
      </div>
      <div className="tools-row">
        <Check label="Include subfolders" value={recursive} onChange={setRecursive} />
        <Check label="Compare contents (not only size and date)" value={content} onChange={setContent} />
        <Check label="Include hidden items" value={hidden} onChange={setHidden} />
        <span className="grow" />
        <button className="primary" disabled={busy || !left || !right} onClick={run}>Compare</button>
      </div>
      {out && (
        <>
          <div className="tools-row">
            <span className="tools-result">{out.entries.length === 0 ? `The folders are the same (${out.same.toLocaleString()} items).` : `${out.entries.length.toLocaleString()} differences, ${out.same.toLocaleString()} items the same.${out.truncated ? " Only the first part is listed." : ""}`}</span>
            <span className="grow" />
            <select aria-label="Show" value={filter} onChange={(e) => setFilter(e.target.value as typeof filter)}>
              <option value="all">Show all differences</option>
              {Object.entries(STATUS).map(([k, v]) => <option key={k} value={k}>{v}</option>)}
            </select>
          </div>
          <div className="tools-list" role="table" aria-label="Differences between the folders">
            {shown.map((e) => (
              <label key={e.rel} className="tools-item" role="row">
                <input type="checkbox" checked={ticked.has(e.rel)} onChange={() => toggle(e.rel)} />
                <span className="tools-item-name" title={e.rel}>{e.rel}</span>
                <span className="muted tools-item-meta">{STATUS[e.status]}</span>
                <span className="muted tools-item-meta">{e.left ? `${e.left.isDir ? "folder" : fmtSize(e.left.size)} · ${fmtDate(e.left.modified)}` : ""}</span>
                <span className="muted tools-item-meta">{e.right ? `${e.right.isDir ? "folder" : fmtSize(e.right.size)} · ${fmtDate(e.right.modified)}` : ""}</span>
              </label>
            ))}
          </div>
          <div className="tools-row">
            <span className="muted">Synchronize the ticked items by copying the newer or missing ones:</span>
            <span className="grow" />
            <button onClick={() => void sync("right")}>Left → right</button>
            <button onClick={() => void sync("left")}>Right → left</button>
            <button onClick={() => void sync("both")}>Both ways</button>
          </div>
        </>
      )}
    </>
  );
}

// ---- Duplicates ------------------------------------------------------------------------------------

function Duplicates({ folder, paths, go, busy, setError }: Ctx) {
  const folders = paths.length ? paths : [folder];
  const [roots, setRoots] = useState(folders.join("; "));
  const [by, setBy] = useState<"content" | "nameSize" | "size">("content");
  const [min, setMin] = useState("1 KB");
  const [hidden, setHidden] = useState(false);
  const [groups, setGroups] = useState<DupGroup[] | null>(null);
  const [ticked, setTicked] = useState<Set<string>>(new Set());
  const run = () => go<DupGroup[]>({ kind: "duplicates", roots: roots.split(";").map((r) => r.trim()).filter(Boolean), opts: { by, minSize: parseSize(min) ?? 0, includeHidden: hidden } }, {}, (g) => { setGroups(g); setTicked(new Set()); });
  const wasted = useMemo(() => (groups ?? []).reduce((n, g) => n + g.size * (g.files.length - 1), 0), [groups]);
  // Tick all copies but one in every group.
  const keep = (which: "newest" | "oldest" | "first") => {
    const t = new Set<string>();
    for (const g of groups ?? []) {
      const sorted = [...g.files].sort((a, b) => (which === "newest" ? b.modified - a.modified : which === "oldest" ? a.modified - b.modified : 0));
      sorted.slice(1).forEach((f) => t.add(f.path));
    }
    setTicked(t);
  };
  const remove = async () => {
    try {
      if (await deletePaths([...ticked])) setGroups((gs) => (gs ?? []).map((g) => ({ ...g, files: g.files.filter((f) => !ticked.has(f.path)) })).filter((g) => g.files.length > 1)), setTicked(new Set());
    } catch (e) {
      setError(String(e));
    }
  };
  return (
    <>
      <div className="tools-grid"><Field label="Search in (separate several folders with ;)" value={roots} onChange={setRoots} wide /></div>
      <div className="tools-row">
        <label className="field-inline">Same <select value={by} onChange={(e) => setBy(e.target.value as typeof by)}><option value="content">content</option><option value="nameSize">name and size</option><option value="size">size</option></select></label>
        <label className="field-inline">Files of at least <input className="text-input tools-short" value={min} onChange={(e) => setMin(e.target.value)} /></label>
        <Check label="Include hidden items" value={hidden} onChange={setHidden} />
        <span className="grow" />
        <button className="primary" disabled={busy || !roots.trim()} onClick={run}>Find duplicates</button>
      </div>
      {groups && (
        <>
          <p className="tools-result">{groups.length === 0 ? "No duplicates found." : `${groups.length.toLocaleString()} groups of duplicates, ${fmtSize(wasted)} could be freed.`}</p>
          <div className="tools-list" role="tree" aria-label="Duplicates">
            {groups.map((g, i) => (
              <div key={i} className="tools-group">
                <div className="tools-group-head muted">{fmtSize(g.size)} each, {g.files.length} copies</div>
                {g.files.map((f) => (
                  <label key={f.path} className="tools-item">
                    <input type="checkbox" checked={ticked.has(f.path)} onChange={() => setTicked((t) => { const n = new Set(t); n.has(f.path) ? n.delete(f.path) : n.add(f.path); return n; })} />
                    <span className="tools-item-name" title={f.path}>{f.path}</span>
                    <span className="muted tools-item-meta">{fmtDate(f.modified)}</span>
                  </label>
                ))}
              </div>
            ))}
          </div>
          {groups.length > 0 && (
            <div className="tools-row">
              <span className="muted">Tick all but the</span>
              <button onClick={() => keep("newest")}>newest</button>
              <button onClick={() => keep("oldest")}>oldest</button>
              <button onClick={() => keep("first")}>first listed</button>
              <span className="grow" />
              <button className="danger" disabled={!ticked.size} onClick={() => void remove()}>Delete {ticked.size || ""} ticked…</button>
            </div>
          )}
        </>
      )}
    </>
  );
}

// ---- Split / combine -----------------------------------------------------------------------------------

const PART_SIZES: [string, string][] = [["1.44 MB (floppy)", "1457664"], ["10 MB", "10 MB"], ["100 MB", "100 MB"], ["650 MB (CD)", "650 MB"], ["700 MB (CD)", "700 MB"], ["1 GB", "1 GB"], ["4 GB (FAT32 limit)", "4095 MB"]];

function SplitJoin({ folder, paths, go, busy }: Ctx) {
  const first = paths[0] ?? "";
  const looksPart = /\.\d{3}$/.test(first);
  const [mode, setMode] = useState<"split" | "join">(looksPart ? "join" : "split");
  const [src, setSrc] = useState(first);
  const [size, setSize] = useState("100 MB");
  const [dest, setDest] = useState(first ? dirName(first) : folder);
  const [msg, setMsg] = useState("");
  const run = () => {
    setMsg("");
    if (mode === "split") void go<string[]>({ kind: "split", src, destDir: dest, partSize: parseSize(size) ?? 0 }, {}, (r) => setMsg(`Done: ${Array.isArray(r) ? r.length : "?"} parts written to ${dest}.`));
    else void go<{ path?: string; verified?: boolean | null } | string>({ kind: "join", first: src, destDir: dest }, {}, (r) => {
      const o = typeof r === "string" ? { path: r } : r;
      setMsg(`Done: ${o.path ?? dest}${o.verified === true ? ". The checksum matches." : o.verified === false ? ". The checksum does NOT match: a part is damaged or missing." : "."}`);
    });
  };
  return (
    <>
      <div className="tools-row">
        <Check label="Split a file into parts" value={mode === "split"} onChange={() => setMode("split")} />
        <Check label="Combine the parts of a file" value={mode === "join"} onChange={() => setMode("join")} />
      </div>
      <div className="tools-grid">
        <Field label={mode === "split" ? "File to split" : "First part (name.001)"} value={src} onChange={setSrc} wide />
        <Field label="Write to the folder" value={dest} onChange={setDest} wide />
      </div>
      {mode === "split" && (
        <div className="tools-row">
          <label className="field-inline">Part size <input className="text-input tools-short" value={size} onChange={(e) => setSize(e.target.value)} /></label>
          <select aria-label="Common sizes" value="" onChange={(e) => e.target.value && setSize(e.target.value)}><option value="">Common sizes…</option>{PART_SIZES.map(([l, v]) => <option key={l} value={v}>{l}</option>)}</select>
          <span className="muted">Writes name.001, name.002… and name.crc to check them when combining.</span>
        </div>
      )}
      <div className="tools-row"><span className="grow" /><button className="primary" disabled={busy || !src || !dest} onClick={run}>{mode === "split" ? "Split" : "Combine"}</button></div>
      {msg && <p className="tools-result">{msg}</p>}
    </>
  );
}

// ---- UUE / XXE / MIME ----------------------------------------------------------------------------------------

function EncodeDecode({ folder, paths, go, busy }: Ctx) {
  const first = paths[0] ?? "";
  const [mode, setMode] = useState<"encode" | "decode">(/\.(uue|xxe|b64|eml|txt)$/i.test(first) ? "decode" : "encode");
  const [src, setSrc] = useState(first);
  const [kind, setKind] = useState<"uue" | "xxe" | "base64">("uue");
  const [dest, setDest] = useState(first ? dirName(first) : folder);
  const [msg, setMsg] = useState("");
  const run = () => {
    setMsg("");
    if (mode === "encode") void go<string>({ kind: "encode", src, destDir: dest, encoding: kind }, {}, (r) => setMsg(`Written: ${r}`));
    else void go<string[]>({ kind: "decode", src, destDir: dest }, {}, (r) => setMsg(r.length ? `Decoded ${r.length} file${r.length === 1 ? "" : "s"}:\n${r.join("\n")}` : "Nothing that looks like UUE, XXE or MIME was found in that file."));
  };
  return (
    <>
      <div className="tools-row">
        <Check label="Encode a file" value={mode === "encode"} onChange={() => setMode("encode")} />
        <Check label="Decode a file (UUE, XXE, MIME/base64)" value={mode === "decode"} onChange={() => setMode("decode")} />
      </div>
      <div className="tools-grid">
        <Field label={mode === "encode" ? "File to encode" : "File to decode"} value={src} onChange={setSrc} wide />
        <Field label="Write to the folder" value={dest} onChange={setDest} wide />
      </div>
      {mode === "encode" && (
        <div className="tools-row">
          <label className="field-inline">Format <select value={kind} onChange={(e) => setKind(e.target.value as typeof kind)}><option value="uue">UUE</option><option value="xxe">XXE</option><option value="base64">MIME (base64)</option></select></label>
        </div>
      )}
      <div className="tools-row"><span className="grow" /><button className="primary" disabled={busy || !src || !dest} onClick={run}>{mode === "encode" ? "Encode" : "Decode"}</button></div>
      {msg && <pre className="tools-result tools-pre">{msg}</pre>}
    </>
  );
}

// ---- Convert pictures ---------------------------------------------------------------------------------------------------

function Convert({ paths, go, busy }: Ctx) {
  const [files, setFiles] = useState(paths.join("; "));
  const [format, setFormat] = useState("jpeg");
  const [quality, setQuality] = useState(85);
  const [resize, setResize] = useState<"none" | "percent" | "fit">("none");
  const [percent, setPercent] = useState(50);
  const [maxW, setMaxW] = useState("1920");
  const [maxH, setMaxH] = useState("1080");
  const [suffix, setSuffix] = useState("");
  const [outDir, setOutDir] = useState("");
  const [overwrite, setOverwrite] = useState(false);
  const [res, setRes] = useState<ConvertResult[] | null>(null);
  const run = () => go<ConvertResult[]>({ kind: "convert", paths: files.split(";").map((f) => f.trim()).filter(Boolean), opts: { format, quality, percent: resize === "percent" ? percent : null, maxWidth: resize === "fit" ? Number(maxW) || null : null, maxHeight: resize === "fit" ? Number(maxH) || null : null, outDir: outDir.trim() || null, suffix, overwrite } }, {}, setRes);
  const failed = res?.filter((r) => r.error).length ?? 0;
  return (
    <>
      <div className="tools-grid"><Field label="Pictures (separate with ;)" value={files} onChange={setFiles} wide /></div>
      <div className="tools-row">
        <label className="field-inline">Convert to <select value={format} onChange={(e) => setFormat(e.target.value)}><option value="jpeg">JPEG</option><option value="png">PNG</option><option value="webp">WebP</option><option value="bmp">BMP</option><option value="gif">GIF</option></select></label>
        {(format === "jpeg" || format === "webp") && <label className="field-inline">Quality <input type="range" min={10} max={100} value={quality} onChange={(e) => setQuality(Number(e.target.value))} /> {quality}</label>}
      </div>
      <div className="tools-row">
        <label className="field-inline">Size <select value={resize} onChange={(e) => setResize(e.target.value as typeof resize)}><option value="none">Keep as is</option><option value="percent">Scale by a percentage</option><option value="fit">Fit inside a width and height</option></select></label>
        {resize === "percent" && <label className="field-inline"><input className="text-input tools-short" type="number" min={1} max={400} value={percent} onChange={(e) => setPercent(Number(e.target.value))} /> %</label>}
        {resize === "fit" && <><input className="text-input tools-short" aria-label="Largest width" value={maxW} onChange={(e) => setMaxW(e.target.value)} /> × <input className="text-input tools-short" aria-label="Largest height" value={maxH} onChange={(e) => setMaxH(e.target.value)} /> pixels</>}
      </div>
      <div className="tools-grid">
        <Field label="Add to the name" value={suffix} onChange={setSuffix} placeholder="_small (empty = same name, new extension)" />
        <Field label="Write to the folder" value={outDir} onChange={setOutDir} placeholder="(next to the original)" />
      </div>
      <div className="tools-row"><Check label="Replace files that already exist" value={overwrite} onChange={setOverwrite} /><span className="grow" /><button className="primary" disabled={busy || !files.trim()} onClick={run}>Convert</button></div>
      {res && (
        <>
          <p className="tools-result">{res.length - failed} of {res.length} converted{failed ? `, ${failed} could not be` : ""}.</p>
          <div className="tools-list">
            {res.map((r) => <div key={r.path} className="tools-item"><span className="tools-item-name" title={r.path}>{baseName(r.path)}</span><span className="muted tools-item-meta">{r.error ?? `→ ${baseName(r.out ?? "")}`}</span></div>)}
          </div>
        </>
      )}
    </>
  );
}

// ---- Search in files ---------------------------------------------------------------------------------------------------------

function Grep({ folder, paths, go, busy }: Ctx) {
  const [roots, setRoots] = useState((paths.length ? paths : [folder]).join("; "));
  const [text, setText] = useState("");
  const [regex, setRegex] = useState(false);
  const [cs, setCs] = useState(false);
  const [word, setWord] = useState(false);
  const [masks, setMasks] = useState("*");
  const [archives, setArchives] = useState(true);
  const [hidden, setHidden] = useState(false);
  const [binary, setBinary] = useState(false);
  const [hits, setHits] = useState<GrepHit[]>([]);
  const [sum, setSum] = useState<GrepSummary | null>(null);
  const run = () => {
    setHits([]);
    setSum(null);
    void go<GrepSummary>({ kind: "grep", roots: roots.split(";").map((r) => r.trim()).filter(Boolean), opts: { pattern: text, regex, caseSensitive: cs, wholeWord: word, masks, includeArchives: archives, includeHidden: hidden, includeBinary: binary, maxFileSize: 256 << 20, maxHits: 20000 } }, { hits: (h) => setHits((x) => (x.length > 20000 ? x : [...x, ...h])) }, setSum);
  };
  const open = (h: GrepHit) => {
    if (h.entry) useStore.getState().navigate(h.path);
    else useStore.getState().openViewer(h.path, { siblings: [h.path] });
  };
  return (
    <>
      <div className="tools-grid">
        <Field label="Search in (several places separated with ;, for example C:\ ; D:\)" value={roots} onChange={setRoots} wide />
        <Field label="Find the text" value={text} onChange={setText} wide />
        <Field label="In files named" value={masks} onChange={setMasks} placeholder="*.txt; *.md" />
      </div>
      <div className="tools-row">
        <Check label="Regular expression" value={regex} onChange={setRegex} />
        <Check label="Match case" value={cs} onChange={setCs} />
        <Check label="Whole words" value={word} onChange={setWord} />
        <Check label="Inside zip, docx, xlsx, odt… files" value={archives} onChange={setArchives} />
        <Check label="Hidden items" value={hidden} onChange={setHidden} />
        <Check label="Binary files" value={binary} onChange={setBinary} />
        <span className="grow" />
        <button className="primary" disabled={busy || !text || !roots.trim()} onClick={run}>Search</button>
      </div>
      {(hits.length > 0 || sum) && <p className="tools-result">{hits.length.toLocaleString()} matches{sum ? ` in ${sum.files.toLocaleString()} files searched${sum.truncated ? " (stopped at the limit)" : ""}.` : " so far…"}</p>}
      <div className="tools-list" role="list" aria-label="Matches">
        {hits.slice(0, 5000).map((h, i) => (
          <div key={i} className="tools-item tools-hit" role="listitem" onDoubleClick={() => open(h)} title="Double-click to open">
            <span className="tools-item-name">{h.path}{h.entry ? ` ▸ ${h.entry}` : ""}</span>
            <span className="muted tools-item-meta">line {h.line}</span>
            <code className="tools-hit-text">{h.text.slice(0, 300)}</code>
            <button className="icon-button" aria-label="Open" onClick={() => open(h)}>Open</button>
          </div>
        ))}
      </div>
    </>
  );
}

/** Called from menus: open the tools on a tab, for the folder being viewed and the items selected in it. */
export function openTools(tab: ToolTab, paths?: string[]) {
  const s = useStore.getState();
  const folder = activeLocation() ?? "";
  useStore.setState({ tools: { tab, folder, paths: paths ?? s.selection, serial: Date.now() } });
}

/** The "File tools" menu: the same entries in the command bar menu and the right-click menu. */
export function fileToolsItems(selection: string[]): import("./ContextMenu").MenuItem[] {
  const two = selection.length === 2;
  const item = (id: ToolTab, label: string, shortcut?: string) => ({ id: `tools-${id}`, label, shortcut, run: () => openTools(id, selection) });
  return [
    item("compareFiles", two ? "Compare the two files…" : "Compare files…"),
    item("compareDirs", "Compare / synchronize folders…"),
    item("duplicates", "Find duplicate files…"),
    item("split", "Split or combine files…"),
    item("encode", "Encode or decode (UUE, XXE, MIME)…"),
    item("convert", "Convert pictures…"),
    item("grep", "Search in files…", "Alt+F7"),
  ];
}
