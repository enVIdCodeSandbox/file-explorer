// "Choose details": pick which columns the Details view shows and in what order, like the same
// window in File Explorer. The list is the same on every kind of device; columns that need the
// file itself on this computer's drives are greyed out for phones, servers and archives.

import { useEffect, useMemo, useState } from "react";
import { adv } from "../settings/advanced";
import { activeLocation, useStore } from "../store";
import { COLUMNS, COLUMN_GROUPS, DEFAULT_SET, canFill, columnDef, hideColumn, moveColumn, normalizeSet, showColumn, widthOf, type ColumnSet } from "../util/columns";
import { isArchiveLoc, isRemote } from "../util/path";

export function ChooseDetails() {
  const open = useStore((s) => s.chooseDetailsOpen);
  const close = () => useStore.setState({ chooseDetailsOpen: false });
  const [draft, setDraft] = useState<ColumnSet>(DEFAULT_SET);
  const [picked, setPicked] = useState("name");
  const [query, setQuery] = useState("");
  const [everywhere, setEverywhere] = useState(false);
  const loc = open ? activeLocation() : null;
  const where = { remote: !!loc && (isRemote(loc) || isArchiveLoc(loc)) };

  useEffect(() => {
    if (!open) return;
    const s = useStore.getState();
    const perFolder = adv<boolean>(s.settings, "files.perFolderView");
    const here = loc ? (perFolder ? s.folderViews[loc]?.columns : undefined) ?? s.columnSet : s.columnSet;
    setDraft(here);
    setPicked("name");
    setQuery("");
    setEverywhere(!perFolder);
  }, [open]); // eslint-disable-line react-hooks/exhaustive-deps

  const perFolder = useStore((s) => adv<boolean>(s.settings, "files.perFolderView"));
  const q = query.trim().toLowerCase();
  const groups = useMemo(
    () =>
      COLUMN_GROUPS.map((g) => ({
        name: g,
        items: COLUMNS.filter((d) => d.group === g && (!q || d.label.toLowerCase().includes(q))),
      })).filter((g) => g.items.length),
    [q],
  );
  if (!open) return null;

  const shown = draft.order;
  const sel = columnDef(picked);
  const isShown = shown.includes(picked);
  const apply = () => {
    useStore.getState().setColumns(normalizeSet(draft), everywhere || !perFolder ? "all" : "here");
    close();
  };
  const toggle = (id: string) => {
    if (id === "name") return;
    setDraft(shown.includes(id) ? hideColumn(draft, id) : showColumn(draft, id));
    setPicked(id);
  };

  return (
    <div className="modal-backdrop" onMouseDown={close}>
      <div
        className="dialog choose-details"
        role="dialog"
        aria-modal="true"
        aria-label="Choose details"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") close();
          e.stopPropagation();
        }}
      >
        <h3>Choose Details</h3>
        <p className="muted">Select the details you want to display for the items in this folder.</p>
        <input className="text-input" placeholder="Search the list" aria-label="Search details" value={query} onChange={(e) => setQuery(e.target.value)} />
        <div className="choose-body">
          <div className="choose-list" role="group" aria-label="Details">
            {groups.map((g) => (
              <div key={g.name}>
                <div className="choose-group">{g.name}</div>
                {g.items.map((d) => {
                  const can = canFill(d, where);
                  return (
                    <label key={d.id} className="choose-row" data-picked={picked === d.id || undefined} data-unavailable={!can || undefined} title={!can ? "Not available for phones, servers and archives" : d.description}>
                      <input type="checkbox" checked={shown.includes(d.id)} disabled={d.id === "name"} onChange={() => toggle(d.id)} />
                      <span onClick={() => setPicked(d.id)}>{d.label}</span>
                    </label>
                  );
                })}
              </div>
            ))}
            {groups.length === 0 && <p className="muted">Nothing matches.</p>}
          </div>
          <div className="choose-buttons">
            <button disabled={!isShown || picked === "name" || shown.indexOf(picked) <= 1} onClick={() => setDraft(moveColumn(draft, picked, -1))}>Move Up</button>
            <button disabled={!isShown || picked === "name" || shown.indexOf(picked) >= shown.length - 1} onClick={() => setDraft(moveColumn(draft, picked, 1))}>Move Down</button>
            <button disabled={isShown || !sel} onClick={() => toggle(picked)}>Show</button>
            <button disabled={!isShown || picked === "name"} onClick={() => toggle(picked)}>Hide</button>
          </div>
        </div>
        {sel && !canFill(sel, where) && <p className="muted">"{sel.label}" is read from the file itself, so it stays blank for phones, servers and archives.</p>}
        <label className="choose-width">
          <span>Width of selected column (in pixels):</span>
          <input
            type="number"
            min={sel?.min ?? 50}
            max={1200}
            disabled={!isShown}
            value={isShown ? widthOf(draft, picked) : ""}
            onChange={(e) => {
              const w = Number(e.target.value);
              if (Number.isFinite(w) && w > 0) setDraft({ ...draft, widths: { ...draft.widths, [picked]: Math.round(w) } });
            }}
          />
        </label>
        {perFolder && (
          <label className="choose-all">
            <input type="checkbox" checked={everywhere} onChange={(e) => setEverywhere(e.target.checked)} />
            <span>Use these columns for all folders</span>
          </label>
        )}
        <div className="dialog-buttons">
          <button onClick={() => setDraft(DEFAULT_SET)}>Reset</button>
          <span className="grow" />
          <button className="primary" onClick={apply}>OK</button>
          <button onClick={close}>Cancel</button>
        </div>
      </div>
    </div>
  );
}
