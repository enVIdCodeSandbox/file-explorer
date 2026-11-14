// Smart Clear: find what can probably go on a drive (or the whole device),
// say how much space that gives back, and clear what the person ticks.
// The rules for what is found are in crates/fast-core/src/smartclear.rs; the
// choosing logic (filters, "clear this much") is in src/util/smartClear.ts.

import { useEffect, useMemo, useRef, useState } from "react";
import { api, on, type ClearItem, type ClearKind, type ClearProgress, type ClearReport } from "../api";
import { ask, useStore } from "../store";
import { formatCount, formatSize } from "../util/format";
import { clearLocation, parentPath } from "../util/path";
import { KINDS, NO_FILTER, idleText, initialSelection, isFiltered, matching, pathsToDelete, percentOf, pickPercent, totalBytes, type ClearFilter } from "../util/smartClear";
import { Icon } from "./Icons";

const AGES = [
  { days: 0, label: "Any age" },
  { days: 30, label: "Not used for a month" },
  { days: 90, label: "Not used for 3 months" },
  { days: 180, label: "Not used for 6 months" },
  { days: 365, label: "Not used for a year" },
  { days: 730, label: "Not used for 2 years" },
];
const SIZES = [
  { bytes: 0, label: "Any size" },
  { bytes: 1024 ** 2, label: "1 MB or more" },
  { bytes: 10 * 1024 ** 2, label: "10 MB or more" },
  { bytes: 100 * 1024 ** 2, label: "100 MB or more" },
  { bytes: 1024 ** 3, label: "1 GB or more" },
];
/** Rows shown per kind before "Show more". */
const PAGE = 100;

export function SmartClearView({ drive }: { drive: string | null }) {
  const { navigate, showToast } = useStore.getState();
  const drives = useStore((s) => s.drives);
  const [report, setReport] = useState<ClearReport | null>(null);
  const [progress, setProgress] = useState<ClearProgress | null>(null);
  const [scanning, setScanning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState<ClearFilter>(NO_FILTER);
  const [open, setOpen] = useState<Set<ClearKind>>(new Set());
  const [shown, setShown] = useState<Partial<Record<ClearKind, number>>>({});
  const [duplicates, setDuplicates] = useState(true);
  const [nonce, setNonce] = useState(0);
  /** The delete job started from here; when it ends, the page looks again. */
  const clearing = useRef<number | null>(null);
  const [busy, setBusy] = useState(false);
  const now = useMemo(() => Math.floor(Date.now() / 1000), [report]);

  const clearable = drives.filter((d) => d.online && !d.readOnly && d.media !== "network");
  const target = drive ? drives.find((d) => d.mount === drive) : null;

  useEffect(() => {
    let live = true;
    setScanning(true);
    setError(null);
    setProgress(null);
    setReport(null);
    api
      .smartClearScan(drive, { oldDays: 180, largeMb: 200, duplicates })
      .then((r) => {
        if (!live) return;
        setReport(r);
        setSelected(initialSelection(r.items));
        setShown({});
      })
      .catch((e) => live && setError(String(e)))
      .finally(() => live && setScanning(false));
    return () => {
      live = false;
      void api.smartClearCancel().catch(() => {});
    };
  }, [drive, duplicates, nonce]);

  useEffect(() => {
    const subs = [
      on("smart-clear-progress", (p) => setProgress(p)),
      on("delete-progress", (p) => {
        if (p.id !== clearing.current || (p.phase !== "done" && p.phase !== "cancelled" && p.phase !== "failed")) return;
        clearing.current = null;
        setBusy(false);
        if (p.errorCount > 0) showToast(`${formatCount(p.errorCount)} item${p.errorCount === 1 ? "" : "s"} could not be removed (in use, or not yours to delete).`);
        useStore.getState().refreshDrives();
        setNonce((n) => n + 1);
      }),
    ];
    return () => subs.forEach((s) => void s.then((off) => off()));
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const items = report?.items ?? [];
  const eligible = useMemo(() => matching(items, filter, now), [items, filter, now]);
  const chosen = useMemo(() => eligible.filter((i) => selected.has(i.path)), [eligible, selected]);
  const chosenBytes = totalBytes(chosen);
  const foundBytes = totalBytes(eligible);
  const percent = percentOf(eligible, selected);
  const byKind = useMemo(() => {
    const map = new Map<ClearKind, ClearItem[]>();
    for (const i of eligible) map.set(i.kind, [...(map.get(i.kind) ?? []), i]);
    return map;
  }, [eligible]);

  const setMany = (list: ClearItem[], ticked: boolean) =>
    setSelected((old) => {
      const next = new Set(old);
      for (const i of list) ticked ? next.add(i.path) : next.delete(i.path);
      return next;
    });

  /** The slider: tick this share of what the filters let through, safest first. */
  const setPercent = (value: number) => {
    const picked = pickPercent(eligible, value);
    setSelected((old) => {
      const next = new Set(old);
      for (const i of eligible) picked.has(i.path) ? next.add(i.path) : next.delete(i.path);
      return next;
    });
  };

  const clear = async () => {
    if (!chosen.length || busy) return;
    const needLook = chosen.filter((i) => !KINDS.find((k) => k.kind === i.kind)?.safe);
    const r = await ask({
      title: `Clear ${formatSize(chosenBytes)}?`,
      message:
        `${formatCount(chosen.length)} item${chosen.length === 1 ? "" : "s"} will be deleted for good. They do not go to the Trash, because that would free no space.` +
        (needLook.length
          ? ` ${formatCount(needLook.length)} of them ${needLook.length === 1 ? "is a file" : "are files"} of your own (downloads, large files, copies or project folders), not just leftovers.`
          : ""),
      buttons: [
        { id: "ok", label: `Delete and free ${formatSize(chosenBytes)}`, primary: true, danger: true },
        { id: "cancel", label: "Cancel" },
      ],
    });
    if (r?.button !== "ok") return;
    try {
      setBusy(true);
      clearing.current = await api.deleteStart(pathsToDelete(chosen), true);
    } catch (e) {
      setBusy(false);
      showToast(String(e));
    }
  };

  const freeAfter = target ? target.available + chosenBytes : null;

  return (
    <div className="page smart-clear">
      <div className="section-head sc-head">
        <h2>Smart Clear</h2>
        <div className="sc-scope">
          <select aria-label="Where to look" value={drive ?? ""} onChange={(e) => navigate(clearLocation(e.target.value || null), { replace: true })}>
            <option value="">The whole device</option>
            {clearable.map((d) => (
              <option key={d.mount} value={d.mount}>
                {d.name ? `${d.name} (${d.mount})` : d.mount}
              </option>
            ))}
          </select>
          <button className="secondary" disabled={scanning || busy} onClick={() => setNonce((n) => n + 1)}>
            Look again
          </button>
        </div>
      </div>

      {error && <div className="empty-state" data-error>{error}</div>}

      {scanning && (
        <div className="sc-scanning" role="status">
          <div className="sc-spinner" aria-hidden="true" />
          <div>
            <strong>Looking for what can go…</strong>
            <p className="muted">
              {progress ? `${formatCount(progress.scanned)} files looked at · ${formatSize(progress.bytes)} found so far · ${progress.step}` : "Starting"}
            </p>
          </div>
          <button className="secondary" onClick={() => void api.smartClearCancel()}>
            Stop
          </button>
        </div>
      )}

      {report && !scanning && (
        <>
          <div className="sc-summary">
            <div className="sc-figure">
              <span className="sc-amount">{formatSize(chosenBytes)}</span>
              <span className="muted">
                will be freed, of {formatSize(foundBytes)} found
                {isFiltered(filter) ? " that match the filters" : ""}
                {report.cancelled ? " (the search was stopped early)" : ""}
              </span>
              {target && freeAfter != null && (
                <span className="muted">
                  {target.name || target.mount}: {formatSize(target.available)} free now, {formatSize(Math.min(freeAfter, target.total))} after
                </span>
              )}
            </div>
            <button className="primary sc-go" disabled={!chosen.length || busy} onClick={() => void clear()}>
              {busy ? "Clearing…" : chosen.length ? `Clear ${formatSize(chosenBytes)}…` : "Nothing ticked"}
            </button>
          </div>

          {items.length === 0 ? (
            <div className="empty-state">Nothing to clear was found{drive ? " on this drive" : ""}.</div>
          ) : (
            <>
              <div className="sc-controls">
                <label className="sc-slider">
                  <span>
                    How much to clear: <strong>{percent}%</strong>
                  </span>
                  <input type="range" min={0} max={100} step={5} value={percent} aria-label="Share of what was found to clear" onChange={(e) => setPercent(Number(e.target.value))} />
                  <span className="muted">Starts with what is safest to remove. Your own files come last.</span>
                </label>
                <div className="sc-filters">
                  <select aria-label="Age" value={filter.olderDays} onChange={(e) => setFilter({ ...filter, olderDays: Number(e.target.value) })}>
                    {AGES.map((a) => (
                      <option key={a.days} value={a.days}>{a.label}</option>
                    ))}
                  </select>
                  <select aria-label="Size" value={filter.minBytes} onChange={(e) => setFilter({ ...filter, minBytes: Number(e.target.value) })}>
                    {SIZES.map((s) => (
                      <option key={s.bytes} value={s.bytes}>{s.label}</option>
                    ))}
                  </select>
                  <input value={filter.text} placeholder="Name or folder contains…" aria-label="Name or folder contains" onChange={(e) => setFilter({ ...filter, text: e.target.value })} />
                  <label className="sc-check">
                    <input type="checkbox" checked={duplicates} onChange={(e) => setDuplicates(e.target.checked)} /> Look for exact copies (slower)
                  </label>
                  {isFiltered(filter) && (
                    <button className="secondary" onClick={() => setFilter(NO_FILTER)}>
                      Clear filters
                    </button>
                  )}
                </div>
              </div>

              <div className="sc-kinds">
                {KINDS.map((k) => {
                  const list = byKind.get(k.kind) ?? [];
                  const total = report.totals.find((t) => t.kind === k.kind);
                  if (!list.length) return null;
                  const ticked = list.filter((i) => selected.has(i.path));
                  const expanded = open.has(k.kind);
                  const limit = shown[k.kind] ?? PAGE;
                  return (
                    <section key={k.kind} className="sc-kind" data-safe={k.safe || undefined}>
                      <div className="sc-kind-head">
                        <input
                          type="checkbox"
                          aria-label={`Clear all ${k.label.toLowerCase()}`}
                          checked={ticked.length === list.length}
                          ref={(el) => {
                            if (el) el.indeterminate = ticked.length > 0 && ticked.length < list.length;
                          }}
                          onChange={(e) => setMany(list, e.target.checked)}
                        />
                        <button
                          className="sc-kind-title"
                          aria-expanded={expanded}
                          onClick={() => setOpen((o) => {
                            const next = new Set(o);
                            expanded ? next.delete(k.kind) : next.add(k.kind);
                            return next;
                          })}
                        >
                          <span className="sc-chevron" data-open={expanded || undefined}><Icon name="chevron" size={12} /></span>
                          <span className="sc-kind-text">
                            <strong>{k.label}</strong>
                            {!k.safe && <span className="sc-tag">Look first</span>}
                            <span className="muted">{k.about}</span>
                          </span>
                        </button>
                        <span className="sc-kind-size">
                          <strong>{formatSize(totalBytes(ticked))}</strong>
                          <span className="muted">
                            of {formatSize(totalBytes(list))} · {formatCount(list.length)} item{list.length === 1 ? "" : "s"}
                          </span>
                        </span>
                      </div>
                      {expanded && (
                        <div className="sc-items">
                          {total && total.listed < total.count && !isFiltered(filter) && (
                            <p className="muted sc-note">
                              Showing the {formatCount(total.listed)} largest of {formatCount(total.count)} ({formatSize(total.bytes)} in all). Clear these and look again for the rest.
                            </p>
                          )}
                          {list.slice(0, limit).map((i) => (
                            <label key={i.path} className="sc-item" title={i.path}>
                              <input type="checkbox" checked={selected.has(i.path)} onChange={(e) => setMany([i], e.target.checked)} />
                              <span className="sc-item-name">
                                <span>{i.name}</span>
                                <span className="muted">{i.kind === "duplicates" ? i.note : parentPath(i.path) ?? ""}</span>
                              </span>
                              <span className="muted sc-item-age">{idleText(i.used, now)}</span>
                              <span className="sc-item-size">{formatSize(i.size)}</span>
                              <button
                                className="icon-button"
                                title="Show in its folder"
                                aria-label={`Show ${i.name} in its folder`}
                                onClick={(e) => {
                                  e.preventDefault();
                                  const parent = parentPath(i.path);
                                  if (!parent) return;
                                  useStore.getState().newTab(parent);
                                  useStore.setState({ pendingSelect: i.path });
                                }}
                              >
                                <Icon name="folderOpen" size={14} />
                              </button>
                            </label>
                          ))}
                          {list.length > limit && (
                            <button className="secondary sc-more" onClick={() => setShown({ ...shown, [k.kind]: limit + PAGE * 5 })}>
                              Show more ({formatCount(list.length - limit)} left)
                            </button>
                          )}
                        </div>
                      )}
                    </section>
                  );
                })}
                {eligible.length === 0 && <div className="empty-state">Nothing matches the filters.</div>}
              </div>
              <p className="muted sc-foot">
                Looked at {formatCount(report.scanned)} files{report.lookedIn.length ? ` in ${report.lookedIn.join(", ")}` : ""}. System folders, other people's files, and folders locked with a passcode are never touched.
              </p>
            </>
          )}
        </>
      )}
    </div>
  );
}
