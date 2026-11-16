// The file viewer, in the spirit of Total Commander's Lister: text, hex and binary views of a file of
// any size (only the part on screen is read), the encoding of your choice, search, go to offset, and
// pictures, players, PDF and HTML pages. "Quick look" is the same viewer opened with the space bar.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import { useStore } from "../store";
import { isArchiveLoc, isRemote } from "../util/path";
import {
  ENCODINGS, MEDIA_LIMIT, binaryRow, hexRow, lineStarts, lineText, looksLikeText, mimeOf, needleFor, offsetWidth, parseOffset, resolveEnc, viewKindOf, unitOf,
  type Enc,
} from "../util/viewerCore";
import { Icon } from "./Icons";

type Mode = "text" | "hex" | "bin" | "media";

interface Win {
  start: number;
  data: Uint8Array;
}

const BACK = 64 * 1024;
const AHEAD = 128 * 1024;
const MARGIN = 8 * 1024;

const saved = (() => {
  try {
    return JSON.parse(localStorage.getItem("fast.viewer") ?? "{}") as { font?: number; wrap?: boolean; enc?: Enc; perRow?: number };
  } catch {
    return {};
  }
})();

export function Viewer() {
  const req = useStore((s) => s.viewer);
  return req ? <ViewerBody key={req.serial} /> : null;
}

function ViewerBody() {
  const req = useStore((s) => s.viewer)!;
  const close = () => useStore.setState({ viewer: null });
  const [index, setIndex] = useState(Math.max(0, req.siblings.indexOf(req.path)));
  const shown = req.siblings[index] ?? req.path;

  const [real, setReal] = useState<string | null>(null);
  const [size, setSize] = useState(0);
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [mode, setMode] = useState<Mode>("text");
  const [enc, setEnc] = useState<Enc>(saved.enc ?? "auto");
  const [font, setFont] = useState(saved.font ?? 13);
  const [wrap, setWrap] = useState(saved.wrap ?? false);
  const [perRow, setPerRow] = useState(saved.perRow ?? 16);
  const [pos, setPos] = useState(0);
  const [win, setWin] = useState<Win | null>(null);
  const [auto, setAuto] = useState<{ enc: Exclude<Enc, "auto">; bom: number }>({ enc: "utf-8", bom: 0 });
  const [query, setQuery] = useState("");
  const [caseSens, setCaseSens] = useState(false);
  const [match, setMatch] = useState<{ at: number; len: number } | null>(null);
  const [status, setStatus] = useState("");
  const [picked, setPicked] = useState<number | null>(null);
  const [media, setMedia] = useState<{ url: string | null; text: string | null; progress: number }>({ url: null, text: null, progress: 0 });
  const [fit, setFit] = useState(true);
  const root = useRef<HTMLDivElement>(null);
  const body = useRef<HTMLDivElement>(null);
  const [rows, setRows] = useState(30);
  const findId = useRef(1);

  const kind = useMemo(() => viewKindOf(name), [name]);
  const mediaOk = kind !== "other" && size > 0 && size <= (MEDIA_LIMIT[kind] ?? 0);
  const eff = enc === "auto" ? auto.enc : enc;

  useEffect(() => {
    try {
      localStorage.setItem("fast.viewer", JSON.stringify({ font, wrap, enc, perRow }));
    } catch {
      /* private mode */
    }
  }, [font, wrap, enc, perRow]);

  // The file: a path on this computer (an item inside an archive is unpacked to a temporary place first).
  useEffect(() => {
    let live = true;
    setReal(null);
    setError(null);
    setWin(null);
    setPos(0);
    setMatch(null);
    setPicked(null);
    setStatus("");
    (async () => {
      try {
        if (isRemote(shown)) throw new Error("Download the file from the server first, then view it.");
        const path = isArchiveLoc(shown) ? (await api.archiveStage([shown]))[0] : shown;
        if (!path) throw new Error("The file could not be read.");
        const info = await api.viewInfo(path);
        const sample = await api.viewRead(path, 0, 8192);
        if (!live) return;
        const k = viewKindOf(info.name);
        const okMedia = k !== "other" && info.size > 0 && info.size <= (MEDIA_LIMIT[k] ?? 0);
        setName(info.name);
        setSize(info.size);
        setAuto(resolveEnc(sample, "auto", info.size > sample.length));
        setMode(okMedia ? "media" : looksLikeText(sample) ? "text" : "hex");
        setReal(path);
      } catch (e) {
        if (live) setError(String(e).replace(/^Error: /, ""));
      }
    })();
    return () => {
      live = false;
    };
  }, [shown]);

  // How many rows fit.
  useEffect(() => {
    const el = body.current;
    if (!el) return;
    const measure = () => setRows(Math.max(5, Math.floor(el.clientHeight / (font * 1.45)) + 1));
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [font, mode, real]);

  // Keep a window of the file around the position being looked at.
  useEffect(() => {
    if (!real || mode === "media") return;
    const covers = win && pos >= win.start + (win.start > 0 ? MARGIN : 0) && pos + (rows * perRow * 2 + 4096) <= win.start + win.data.length + (win.start + win.data.length >= size ? 1e12 : -MARGIN);
    if (covers) return;
    let live = true;
    const start = Math.max(0, pos - BACK) & ~1;
    api
      .viewRead(real, start, BACK + AHEAD)
      .then((data) => live && setWin({ start, data }))
      .catch((e) => live && setError(String(e)));
    return () => {
      live = false;
    };
  }, [real, mode, pos, win, rows, perRow, size]);

  // Lines of the window, as byte offsets in the file. The first one is left out when it begins in the middle of a line.
  const lines = useMemo(() => {
    if (!win || mode !== "text") return [] as number[];
    const rel = lineStarts(win.data.subarray(auto.bom && win.start === 0 ? auto.bom : 0), eff);
    const shift = auto.bom && win.start === 0 ? auto.bom : 0;
    const abs = rel.map((r) => win.start + shift + r);
    if (win.start > 0 && abs.length > 1) abs.shift();
    else if (win.start > 0) abs.length = 0;
    return abs;
  }, [win, mode, eff, auto.bom]);

  // A position that is not the start of a line (from the scroll bar, a search or "go to") moves to the one it is in.
  useEffect(() => {
    // Only once the part of the file around the position has been read (else the lines are from somewhere else).
    const here = !!win && pos >= win.start && pos < win.start + win.data.length;
    if (mode === "text" && lines.length && here) {
      let lo = 0;
      let hi = lines.length - 1;
      while (lo < hi) {
        const mid = (lo + hi + 1) >> 1;
        if (lines[mid] <= pos) lo = mid;
        else hi = mid - 1;
      }
      const snapped = Math.max(lines[lo], 0);
      if (snapped !== pos && Math.abs(snapped - pos) < 1e9) setPos(snapped);
    } else if ((mode === "hex" || mode === "bin") && pos % perRow !== 0) setPos(pos - (pos % perRow));
  }, [lines, mode, pos, perRow, win]);

  // A picture, player or page: read the whole file.
  useEffect(() => {
    if (mode !== "media" || !real) return;
    let live = true;
    let url: string | null = null;
    setMedia({ url: null, text: null, progress: 0 });
    (async () => {
      try {
        const parts: Uint8Array[] = [];
        for (let at = 0; at < size; at += 4 << 20) {
          const part = await api.viewRead(real, at, 4 << 20);
          if (!live) return;
          parts.push(part);
          setMedia((m) => ({ ...m, progress: Math.min(1, (at + part.length) / size) }));
        }
        const blob = new Blob(parts as BlobPart[], { type: mimeOf(name) });
        if (kind === "html") {
          setMedia({ url: null, text: await blob.text(), progress: 1 });
        } else {
          url = URL.createObjectURL(blob);
          setMedia({ url, text: null, progress: 1 });
        }
      } catch (e) {
        if (live) setError(String(e));
      }
    })();
    return () => {
      live = false;
      if (url) URL.revokeObjectURL(url);
    };
  }, [mode, real, size, name, kind]);

  // ---- Moving about
  const clamp = (n: number) => Math.max(0, Math.min(Math.max(0, size - 1), n));
  const scrollRows = useCallback(
    (n: number) => {
      if (mode === "text") {
        const i = lines.findIndex((l) => l >= pos);
        const at = Math.max(0, Math.min(lines.length - 1, (i < 0 ? 0 : i) + n));
        if (lines.length === 0) return;
        const next = lines[at];
        // Past the end of what is read: the line before the end stays at the top.
        if (n > 0 && next === pos && win && win.start + win.data.length >= size) return;
        setPos(next);
      } else setPos((p) => clamp(p + n * perRow));
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [mode, lines, pos, perRow, win, size],
  );

  const gotoOffset = () => {
    const raw = window.prompt("Go to offset (decimal, 0x… or …h):", "0x");
    const n = raw === null ? null : parseOffset(raw);
    if (n !== null) setPos(clamp(n));
  };

  const find = async (backward: boolean) => {
    if (!real || !query) return;
    const needle = needleFor(query, eff);
    if (!needle) {
      setStatus(`"${query}" cannot be written in ${ENCODINGS.find(([k]) => k === eff)?.[1] ?? eff}.`);
      return;
    }
    setStatus("Searching…");
    const from = match ? (backward ? match.at : match.at + unitOf(eff)) : pos;
    try {
      const at = await api.viewFind(findId.current++, real, needle, backward ? from : from, backward, !caseSens);
      if (at === null) setStatus("Not found.");
      else {
        setMatch({ at, len: needle.length });
        setPos(at);
        setStatus("");
      }
    } catch (e) {
      setStatus(String(e));
    }
  };

  const step = (by: number) => {
    const next = index + by;
    if (next >= 0 && next < req.siblings.length) setIndex(next);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const typing = (e.target as HTMLElement).tagName === "INPUT" || (e.target as HTMLElement).tagName === "SELECT";
    const mod = e.ctrlKey || e.metaKey;
    if (e.key === "Escape") {
      if (typing) (e.target as HTMLElement).blur();
      else close();
    } else if (e.key === "F3" && !mod) void find(e.shiftKey);
    else if (mod && e.key.toLowerCase() === "f") root.current?.querySelector<HTMLInputElement>("input.viewer-find")?.focus();
    else if (mod && e.key.toLowerCase() === "g") gotoOffset();
    else if (typing) return;
    else if (e.key === " " && req.quick) close();
    else if (mode !== "media" && e.key === "ArrowDown") scrollRows(1);
    else if (mode !== "media" && e.key === "ArrowUp") scrollRows(-1);
    else if (mode !== "media" && e.key === "PageDown") scrollRows(rows - 1);
    else if (mode !== "media" && e.key === "PageUp") scrollRows(-(rows - 1));
    else if (mode !== "media" && e.key === "Home") setPos(0);
    else if (mode !== "media" && e.key === "End") setPos(clamp(size - Math.min(size, rows * perRow)));
    else if (e.key === "ArrowRight" && req.quick) step(1);
    else if (e.key === "ArrowLeft" && req.quick) step(-1);
    else if (e.key.toLowerCase() === "n" && !mod) step(1);
    else if (e.key.toLowerCase() === "p" && !mod) step(-1);
    else if (e.key.toLowerCase() === "t") setMode("text");
    else if (e.key.toLowerCase() === "h") setMode("hex");
    else if (e.key.toLowerCase() === "b") setMode("bin");
    else if (e.key.toLowerCase() === "i" && mediaOk) setMode("media");
    else if (e.key === "+" || e.key === "=") setFont((f) => Math.min(32, f + 1));
    else if (e.key === "-") setFont((f) => Math.max(8, f - 1));
    else return;
    e.preventDefault();
    e.stopPropagation();
  };
  useEffect(() => root.current?.focus(), [real]);

  // ---- What to draw
  const re = useMemo(() => {
    if (!query) return null;
    try {
      return new RegExp(query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), caseSens ? "g" : "gi");
    } catch {
      return null;
    }
  }, [query, caseSens]);

  const mark = (text: string) => {
    if (!re) return text;
    const out: React.ReactNode[] = [];
    let last = 0;
    for (const m of text.matchAll(re)) {
      if (m.index === undefined || m[0] === "") continue;
      out.push(text.slice(last, m.index), <mark key={m.index}>{m[0]}</mark>);
      last = m.index + m[0].length;
    }
    return out.length ? [...out, text.slice(last)] : text;
  };

  const textView = () => {
    if (!win) return null;
    const first = Math.max(0, lines.findIndex((l) => l >= pos));
    const out: React.ReactNode[] = [];
    for (let i = first; i < Math.min(lines.length, first + rows + 2); i++) {
      const from = lines[i] - win.start;
      const to = (i + 1 < lines.length ? lines[i + 1] : win.start + win.data.length) - win.start;
      const t = lineText(win.data, from, to, eff).slice(0, 20000);
      out.push(<div className="viewer-line" key={lines[i]}>{t ? mark(t) : "​"}</div>);
    }
    return out;
  };

  const hexView = () => {
    if (!win) return null;
    const width = offsetWidth(size);
    const out: React.ReactNode[] = [];
    const top = pos - win.start;
    for (let r = 0; r < rows; r++) {
      const from = top + r * perRow;
      if (from < 0 || from >= win.data.length || win.start + from >= size) break;
      if (mode === "bin") {
        const row = binaryRow(win.data, from, Math.max(4, perRow / 2), win.start, width);
        out.push(<div className="viewer-hexrow" key={from}><span className="viewer-off">{row.offset}</span> <span>{row.bits.join(" ")}</span></div>);
      } else {
        const row = hexRow(win.data, from, perRow, win.start, eff, width);
        out.push(
          <div className="viewer-hexrow" key={from}>
            <span className="viewer-off">{row.offset}</span>{" "}
            {row.bytes.map((b, i) => {
              const abs = win.start + from + i;
              const hit = match && abs >= match.at && abs < match.at + match.len;
              return <span key={i} className="viewer-byte" data-hit={hit || undefined} data-picked={picked === abs || undefined} onClick={() => setPicked(abs)}>{b}{i % 8 === 7 && i < row.bytes.length - 1 ? "  " : " "}</span>;
            })}
            {"   ".repeat(Math.max(0, perRow - row.bytes.length))}
            <span className="viewer-ascii">{row.text}</span>
          </div>,
        );
      }
    }
    return out;
  };

  const pickedInfo = picked !== null && win && picked >= win.start && picked < win.start + win.data.length ? win.data[picked - win.start] : null;
  const percent = size ? Math.min(100, Math.round((pos / size) * 100)) : 0;
  const encLabel = ENCODINGS.find(([k]) => k === eff)?.[1] ?? eff;
  const external = () => void api.openItem(real ?? shown).catch((e) => useStore.getState().showToast(String(e)));

  return (
    <div className="viewer-backdrop" onMouseDown={close}>
      <div ref={root} className="viewer" data-quick={req.quick || undefined} role="dialog" aria-modal="true" aria-label={`View ${name}`} tabIndex={-1} onMouseDown={(e) => e.stopPropagation()} onKeyDown={onKeyDown}>
        <div className="viewer-bar">
          <strong className="viewer-name" title={shown}>{name || shown}</strong>
          <span className="muted viewer-size">{formatBytes(size)}{req.siblings.length > 1 ? `  ·  ${index + 1} of ${req.siblings.length}` : ""}</span>
          <span className="grow" />
          <div className="viewer-modes" role="group" aria-label="View as">
            {mediaOk && <button data-on={mode === "media" || undefined} onClick={() => setMode("media")} title="Preview (I)">Preview</button>}
            <button data-on={mode === "text" || undefined} onClick={() => setMode("text")} title="Text (T)">Text</button>
            <button data-on={mode === "hex" || undefined} onClick={() => setMode("hex")} title="Hex (H)">Hex</button>
            {!req.quick && <button data-on={mode === "bin" || undefined} onClick={() => setMode("bin")} title="Binary (B)">Binary</button>}
          </div>
          <button className="icon-button" aria-label="Open with the default app" title="Open with the default app" onClick={external}><Icon name="share" size={14} /></button>
          <button className="icon-button" aria-label="Close" title="Close (Esc)" onClick={close}><Icon name="close" size={14} /></button>
        </div>

        {mode !== "media" && !error && (
          <div className="viewer-tools">
            {mode !== "bin" && (
              <select aria-label="Encoding" value={enc} onChange={(e) => setEnc(e.target.value as Enc)}>
                {ENCODINGS.map(([k, label]) => <option key={k} value={k}>{k === "auto" ? `Automatic (${ENCODINGS.find(([x]) => x === auto.enc)?.[1] ?? auto.enc})` : label}</option>)}
              </select>
            )}
            {mode !== "text" && (
              <select aria-label="Bytes per row" value={perRow} onChange={(e) => setPerRow(Number(e.target.value))}>
                {[8, 16, 24, 32].map((n) => <option key={n} value={n}>{mode === "bin" ? Math.max(4, n / 2) : n} bytes per row</option>)}
              </select>
            )}
            {mode === "text" && <label className="viewer-check"><input type="checkbox" checked={wrap} onChange={(e) => setWrap(e.target.checked)} /> Wrap lines</label>}
            <button className="icon-button" aria-label="Smaller text" title="Smaller text (−)" onClick={() => setFont((f) => Math.max(8, f - 1))}>A−</button>
            <button className="icon-button" aria-label="Larger text" title="Larger text (+)" onClick={() => setFont((f) => Math.min(32, f + 1))}>A+</button>
            <span className="grow" />
            <input className="text-input viewer-find" placeholder="Find (Ctrl+F)" aria-label="Find" value={query} onChange={(e) => { setQuery(e.target.value); setMatch(null); }} onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); e.stopPropagation(); void find(e.shiftKey); } }} />
            <button className="icon-button" aria-label="Match case" title="Match case" data-on={caseSens || undefined} onClick={() => setCaseSens((c) => !c)}>Aa</button>
            <button className="icon-button" aria-label="Find previous" title="Previous (Shift+F3)" onClick={() => void find(true)}>↑</button>
            <button className="icon-button" aria-label="Find next" title="Next (F3)" onClick={() => void find(false)}>↓</button>
            <button className="icon-button" title="Go to offset (Ctrl+G)" onClick={gotoOffset}>Go to…</button>
          </div>
        )}

        <div ref={body} className="viewer-body" data-mode={mode} data-wrap={(mode === "text" && wrap) || undefined} style={{ fontSize: font }} onWheel={(e) => mode !== "media" && scrollRows(Math.sign(e.deltaY) * 3)}>
          {error ? (
            <div className="empty-state"><strong>This file cannot be viewed</strong><span>{error}</span></div>
          ) : !real ? (
            <div className="loading-bar" />
          ) : mode === "media" ? (
            media.progress < 1 && !media.url && media.text === null ? (
              <div className="empty-state"><span>Reading the file… {Math.round(media.progress * 100)}%</span></div>
            ) : kind === "image" && media.url ? (
              <div className="viewer-image" data-fit={fit || undefined} onClick={() => setFit(!fit)} title="Click to switch between fitting the window and full size"><img src={media.url} alt={name} /></div>
            ) : kind === "video" && media.url ? (
              <video className="viewer-player" src={media.url} controls />
            ) : kind === "audio" && media.url ? (
              <div className="viewer-audio"><Icon name="file" size={48} /><audio src={media.url} controls /></div>
            ) : kind === "pdf" && media.url ? (
              <iframe className="viewer-frame" title={name} src={media.url} />
            ) : kind === "html" && media.text !== null ? (
              <iframe className="viewer-frame" title={name} sandbox="" srcDoc={media.text} />
            ) : null
          ) : mode === "text" ? (
            textView()
          ) : (
            hexView()
          )}
        </div>

        {mode !== "media" && !error && real && (
          <div className="viewer-status">
            <input className="viewer-scroll" type="range" min={0} max={10000} value={size ? Math.round((pos / size) * 10000) : 0} aria-label="Position in the file" onChange={(e) => setPos(clamp(Math.floor((Number(e.target.value) / 10000) * size)))} />
            <span>{percent}%</span>
            <span>Offset {pos.toLocaleString()} (0x{pos.toString(16).toUpperCase()})</span>
            {pickedInfo !== null && <span>Byte {picked!.toLocaleString()}: {pickedInfo} = 0x{pickedInfo.toString(16).toUpperCase().padStart(2, "0")} = {pickedInfo.toString(2).padStart(8, "0")}b</span>}
            <span className="grow" />
            {status && <span>{status}</span>}
            {mode !== "bin" && <span>{encLabel}</span>}
          </div>
        )}
      </div>
    </div>
  );
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} bytes`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v < 10 ? 2 : 1)} ${units[i]}`;
}
