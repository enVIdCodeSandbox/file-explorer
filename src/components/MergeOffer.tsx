// A whole window was held over this one: it asks which of that window's tabs to take, and whether they become tabs
// here or split panes. Nothing moves until a button is pressed; Cancel brings the other window back.

import { useEffect, useState } from "react";
import { api } from "../api";
import { splitPaths, paneCount, useSplit, plainFolder } from "../split/state";
import { tabTitle, useStore } from "../store";
import { MAX_PANES, GHOST, leaves, presetsFor } from "../util/splitLayout";
import { fitsPane, planMerge, useMerge, type MergeMode } from "../util/mergeWindows";
import { FileIcon } from "./Icons";
import { Mini } from "./Split";

export function MergeOffer() {
  const hover = useMerge((s) => s.hover);
  const offer = useMerge((s) => s.offer);
  const allowed = useSplit((s) => s.allowed);
  const [ticked, setTicked] = useState<string[]>([]);
  const [mode, setMode] = useState<MergeMode>("tabs");
  const [layout, setLayout] = useState("");
  useEffect(() => {
    if (!offer) return;
    setTicked(offer.tabs.map((t) => t.id));
    setMode("tabs");
    setLayout("");
  }, [offer]);
  return (
    <>
      {hover && <div className="merge-outline" aria-hidden="true" />}
      {offer && <Dialog offer={offer} allowed={allowed} ticked={ticked} setTicked={setTicked} mode={mode} setMode={setMode} layout={layout} setLayout={setLayout} />}
    </>
  );
}

function Dialog(p: {
  offer: NonNullable<ReturnType<typeof useMerge.getState>["offer"]>;
  allowed: boolean;
  ticked: string[];
  setTicked: (f: (c: string[]) => string[]) => void;
  mode: MergeMode;
  setMode: (m: MergeMode) => void;
  layout: string;
  setLayout: (l: string) => void;
}) {
  const { offer } = p;
  useStore((s) => s.settings);
  const picked = offer.tabs.filter((t) => p.ticked.includes(t.id));
  const room = MAX_PANES - paneCount();
  const canPane = p.allowed && room > 0 && picked.some((t) => fitsPane(t.location));
  const mode: MergeMode = canPane ? p.mode : "tabs";
  const plan = planMerge(picked, mode, room);
  const total = paneCount() + plan.panes.length;
  const options = mode === "panes" ? presetsFor(total) : [];
  const chosen = options.some((o) => o.id === p.layout) ? p.layout : options[0]?.id;
  const others = leaves(useSplit.getState().tree).filter((s) => s !== 0);
  const close = () => useMerge.setState({ offer: null, hover: false });
  const cancel = () => {
    close();
    void api.mergeCancel(offer.source).catch(() => {});
  };
  const go = () => {
    if (!picked.length) return;
    const s = useStore.getState();
    if (plan.tabs.length) s.insertTabs(plan.tabs.map((t) => t.location));
    if (plan.panes.length) splitPaths(plan.panes.map((t) => plainFolder(t.location)), chosen);
    close();
    void api.mergeDone(offer.source, picked.map((t) => t.id)).catch((e) => s.showToast(String(e)));
  };
  const toggle = (id: string) => p.setTicked((c) => (c.includes(id) ? c.filter((x) => x !== id) : [...c, id]));
  const all = picked.length === offer.tabs.length;
  const what = picked.length === 1 ? "1 tab" : `${picked.length} tabs`;
  return (
    <div className="modal-backdrop" onMouseDown={cancel}>
      <div className="dialog tab-chooser" role="dialog" aria-modal="true" aria-label="Take tabs from the other window" onMouseDown={(e) => e.stopPropagation()} onKeyDown={(e) => { if (e.key === "Escape") cancel(); e.stopPropagation(); }}>
        <h3>Take tabs from the other window</h3>
        <p className="dialog-message">Choose which of its tabs to bring here. The ones you leave stay in that window.</p>
        <label className="chooser-row chooser-all">
          <input type="checkbox" checked={all} onChange={() => p.setTicked(() => (all ? [] : offer.tabs.map((t) => t.id)))} />
          <span className="chooser-name">All {offer.tabs.length} tabs</span>
        </label>
        <div className="chooser-list" role="group" aria-label="Tabs of the other window">
          {offer.tabs.map((t) => (
            <label key={t.id} className="chooser-row">
              <input type="checkbox" checked={p.ticked.includes(t.id)} onChange={() => toggle(t.id)} />
              <FileIcon kind="folder" name={tabTitle(t.location).toLowerCase()} size={16} />
              <span className="chooser-name">{tabTitle(t.location)}</span>
            </label>
          ))}
        </div>
        <div className="chooser-modes" role="radiogroup" aria-label="How to bring them">
          <label>
            <input type="radio" name="merge-mode" checked={mode === "tabs"} onChange={() => p.setMode("tabs")} /> As tabs in this window
          </label>
          <label title={canPane ? undefined : "Split view is not available here, or there is no room for another pane."}>
            <input type="radio" name="merge-mode" checked={mode === "panes"} disabled={!canPane} onChange={() => p.setMode("panes")} /> As split panes
          </label>
        </div>
        {mode === "panes" && plan.tabs.length > 0 && <p className="dialog-message">{plan.tabs.length === 1 ? "1 tab does" : `${plan.tabs.length} tabs do`} not fit a pane (a place that is not a folder here, or no room left) and {plan.tabs.length === 1 ? "is" : "are"} added as {plan.tabs.length === 1 ? "a tab" : "tabs"}.</p>}
        {options.length > 0 && (
          <div className="chooser-layouts" role="radiogroup" aria-label="Layout">
            {options.map((o) => (
              <button key={o.id} type="button" role="radio" aria-checked={chosen === o.id} className="split-preset" data-active={chosen === o.id || undefined} title={o.label} onClick={() => p.setLayout(o.id)}>
                <Mini tree={o.build([0, ...others, ...Array(plan.panes.length).fill(GHOST)])} />
              </button>
            ))}
          </div>
        )}
        <div className="dialog-buttons">
          <button onClick={cancel}>Cancel</button>
          <button className="primary" disabled={!picked.length} onClick={go} autoFocus>
            {mode === "panes" ? `Split ${what}` : `Add ${what}`}
          </button>
        </div>
      </div>
    </div>
  );
}
