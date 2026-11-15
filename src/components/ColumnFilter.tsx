// The filter popover under a column heading: tick the values to keep. Nothing ticked off means no
// filter. Dates and sizes are offered as ranges (Today, Large…) rather than every distinct value.

import { useEffect, useMemo, useRef, useState } from "react";

export function ColumnFilter({
  label,
  x,
  y,
  facets,
  selected,
  loading,
  onChange,
  onClose,
}: {
  label: string;
  x: number;
  y: number;
  facets: [string, number][];
  /** Values kept; empty means all of them. */
  selected: string[];
  loading: boolean;
  onChange: (selected: string[]) => void;
  onClose: () => void;
}) {
  const box = useRef<HTMLDivElement>(null);
  const [query, setQuery] = useState("");
  const [pos, setPos] = useState({ left: x, top: y });

  useEffect(() => {
    const away = (e: MouseEvent) => !box.current?.contains(e.target as Node) && onClose();
    const key = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    document.addEventListener("mousedown", away, true);
    document.addEventListener("keydown", key, true);
    return () => {
      document.removeEventListener("mousedown", away, true);
      document.removeEventListener("keydown", key, true);
    };
  }, [onClose]);

  // Stay inside the window.
  useEffect(() => {
    const r = box.current?.getBoundingClientRect();
    if (!r) return;
    setPos({ left: Math.max(4, Math.min(x, window.innerWidth - r.width - 4)), top: Math.max(4, Math.min(y, window.innerHeight - r.height - 4)) });
  }, [x, y, facets.length]);

  const all = useMemo(() => facets.map(([v]) => v), [facets]);
  const q = query.trim().toLowerCase();
  const shown = facets.filter(([v]) => !q || v.toLowerCase().includes(q));
  const on = (v: string) => selected.length === 0 || selected.includes(v);
  const toggle = (v: string) => {
    const now = selected.length === 0 ? all : selected;
    const next = now.includes(v) ? now.filter((x) => x !== v) : [...now, v];
    // Keeping nothing would hide everything; keep at least one. Keeping everything is no filter.
    if (next.length === 0) return;
    onChange(next.length === all.length ? [] : next);
  };

  return (
    <div ref={box} className="column-filter" role="dialog" aria-label={`Filter by ${label}`} style={pos} onContextMenu={(e) => e.preventDefault()}>
      <div className="column-filter-title">Filter by {label}</div>
      {facets.length > 8 && <input className="text-input" autoFocus placeholder="Search values" aria-label="Search values" value={query} onChange={(e) => setQuery(e.target.value)} />}
      <div className="column-filter-list" role="group" aria-label="Values">
        {shown.map(([v, n]) => (
          <label key={v} className="column-filter-row">
            <input type="checkbox" checked={on(v)} onChange={() => toggle(v)} />
            <span className="column-filter-value" title={v}>{v}</span>
            <span className="column-filter-count">{n}</span>
          </label>
        ))}
        {shown.length === 0 && <p className="muted">{loading ? "Reading the files…" : "Nothing matches."}</p>}
      </div>
      {loading && shown.length > 0 && <p className="muted column-filter-note">Still reading the files…</p>}
      <div className="column-filter-buttons">
        <button disabled={selected.length === 0} onClick={() => onChange([])}>Clear filter</button>
        <span className="grow" />
        <button className="primary" onClick={onClose}>Done</button>
      </div>
    </div>
  );
}
