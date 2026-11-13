import { useEffect, useMemo, useRef, useState } from "react";
import { applyRenames, useStore } from "../store";
import { DEFAULT_RULES, PROBLEM_TEXT, planRenames, type RenameRules } from "../util/batchRename";

/** The Rename dialog for several items at once, with a live preview. */
export function BatchRename() {
  const request = useStore((s) => s.batchRename);
  const [rules, setRules] = useState<RenameRules>(DEFAULT_RULES);
  const [busy, setBusy] = useState(false);
  const first = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!request) return;
    setRules(DEFAULT_RULES);
    setBusy(false);
    setTimeout(() => first.current?.select(), 0);
  }, [request]);

  const plans = useMemo(() => (request ? planRenames(request.items, rules, request.siblings) : []), [request, rules]);
  if (!request) return null;

  const changed = plans.filter((p) => p.changed);
  const problems = plans.filter((p) => p.problem).length;
  const close = () => useStore.setState({ batchRename: null });
  const set = <K extends keyof RenameRules>(key: K, value: RenameRules[K]) => setRules((r) => ({ ...r, [key]: value }));
  const apply = async () => {
    if (busy || !changed.length || problems) return;
    setBusy(true);
    await applyRenames(request.dir, plans);
    close();
  };

  return (
    <div className="modal-backdrop" onMouseDown={close}>
      <div
        className="dialog batch-rename"
        role="dialog"
        aria-modal="true"
        aria-label="Rename items"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") close();
          if (e.key === "Enter" && !(e.target instanceof HTMLButtonElement)) void apply();
          e.stopPropagation();
        }}
      >
        <h3>Rename {request.items.length} items</h3>

        <label className="batch-field">
          <span>New name</span>
          <input ref={first} className="text-input" value={rules.template} spellCheck={false} onChange={(e) => set("template", e.target.value)} />
        </label>
        <p className="batch-hint muted">
          <code>{"{name}"}</code> the current name, <code>{"{n}"}</code> a number, <code>{"{date}"}</code> the date changed. Extensions stay as they are.
        </p>

        <div className="batch-row">
          <label className="batch-field">
            <span>Find</span>
            <input className="text-input" value={rules.find} spellCheck={false} onChange={(e) => set("find", e.target.value)} />
          </label>
          <label className="batch-field">
            <span>Replace with</span>
            <input className="text-input" value={rules.replace} spellCheck={false} onChange={(e) => set("replace", e.target.value)} />
          </label>
          <label className="batch-check">
            <input type="checkbox" checked={rules.matchCase} onChange={(e) => set("matchCase", e.target.checked)} />
            Match case
          </label>
        </div>

        <div className="batch-row">
          <label className="batch-field narrow">
            <span>Start at</span>
            <input type="number" min={0} value={rules.start} onChange={(e) => set("start", Number(e.target.value) || 0)} />
          </label>
          <label className="batch-field narrow">
            <span>Digits</span>
            <input type="number" min={1} max={12} value={rules.digits} onChange={(e) => set("digits", Number(e.target.value) || 1)} />
          </label>
          <label className="batch-field">
            <span>Letters</span>
            <select value={rules.letters} onChange={(e) => set("letters", e.target.value as RenameRules["letters"])}>
              <option value="keep">Leave as they are</option>
              <option value="lower">all lowercase</option>
              <option value="upper">ALL UPPERCASE</option>
              <option value="title">Each Word Capitalised</option>
            </select>
          </label>
        </div>

        <div className="batch-preview" role="list" aria-label="Preview">
          {plans.map((p) => (
            <div key={p.from} className="batch-line" role="listitem" data-changed={p.changed || undefined} data-problem={p.problem || undefined}>
              <span className="batch-from" title={p.from}>{p.from}</span>
              <span className="batch-arrow" aria-hidden>→</span>
              <span className="batch-to" title={p.problem ? PROBLEM_TEXT[p.problem] : p.to}>{p.to || "(empty)"}</span>
              {p.problem && <span className="batch-problem">{PROBLEM_TEXT[p.problem]}</span>}
            </div>
          ))}
        </div>

        <div className="dialog-buttons">
          <span className="batch-summary muted">{problems ? `${problems} can't be renamed` : changed.length ? `${changed.length} will change` : "Nothing changes yet"}</span>
          <button className="secondary" onClick={close}>Cancel</button>
          <button className="primary" disabled={busy || !changed.length || problems > 0} onClick={() => void apply()}>
            Rename{changed.length ? ` ${changed.length}` : ""}
          </button>
        </div>
      </div>
    </div>
  );
}
