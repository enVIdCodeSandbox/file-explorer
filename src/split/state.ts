// Split view state: the layout of the panes, the tabs each extra pane has, which pane has the focus, and the dragging
// of tabs onto the panes. The normal file list (pane 0) keeps using the tabs of the window; the extra panes (1 to 7)
// each have tabs of their own, which are only folders.

import { create } from "zustand";
import { useStore } from "../store";
import { isAppLocation, isRemote, isArchiveLoc } from "../util/path";
import {
  GHOST,
  MAX_PANES,
  evenOut,
  insertPane,
  leaves,
  parseTree,
  presetsFor,
  removePane,
  setShares as setTreeShares,
  zoneAt,
  type Node,
  type Preset,
  type Rect,
  type Side,
  type Zone,
} from "../util/splitLayout";

export interface PTab {
  id: string;
  path: string;
  back: string[];
  fwd: string[];
}
export interface PaneState {
  tabs: PTab[];
  active: string;
}

/** What a dragged tab is over right now. */
export interface Hover {
  zone?: Zone;
  preset?: string;
}

interface SplitState {
  /** The layout: pane 0 is the normal file list. */
  tree: Node;
  panes: Record<number, PaneState>;
  /** The panes, most recently used first. */
  history: number[];
  /** Whether the current place can be split (not a server, phone or archive folder). */
  allowed: boolean;
  /** A tab is being dragged: what it is over, what it is called, and the pane it would empty (if it is that pane's only tab). */
  drag: { hover: Hover | null; label: string; empties: number | null } | null;
}

const KEY = "fast.split.v3";
const uid = () => Math.random().toString(36).slice(2, 9);

function load(): Pick<SplitState, "tree" | "panes"> {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? "null");
    const tree = parseTree(raw?.tree);
    const panes: Record<number, PaneState> = {};
    for (const slot of leaves(tree)) {
      const p = raw?.panes?.[slot];
      if (slot > 0 && p && Array.isArray(p.tabs) && p.tabs.length) {
        const tabs: PTab[] = p.tabs.filter((t: PTab) => t && typeof t.path === "string").map((t: PTab) => ({ id: String(t.id || uid()), path: t.path, back: Array.isArray(t.back) ? t.back : [], fwd: Array.isArray(t.fwd) ? t.fwd : [] }));
        if (tabs.length) panes[slot] = { tabs, active: tabs.some((t) => t.id === p.active) ? p.active : tabs[0].id };
      }
    }
    return { tree, panes };
  } catch {
    return { tree: 0, panes: {} };
  }
}

export const useSplit = create<SplitState>(() => ({ ...load(), history: [0], allowed: true, drag: null }));

useSplit.subscribe((s, prev) => {
  if (s.tree === prev.tree && s.panes === prev.panes) return;
  try {
    localStorage.setItem(KEY, JSON.stringify({ tree: s.tree, panes: s.panes }));
  } catch {
    /* not saved */
  }
});

const set = useSplit.setState;
const get = useSplit.getState;

// ---- Small helpers --------------------------------------------------------------------------------------

/** A folder the extra panes can show: the place itself, or the home folder for places they cannot show. */
export function plainFolder(location: string | undefined): string {
  const home = useStore.getState().info?.home ?? "";
  if (!location || isAppLocation(location) || isRemote(location) || isArchiveLoc(location)) return home;
  return location;
}

/** The folder of the normal file list. */
export function mainFolder(): string {
  const s = useStore.getState();
  return plainFolder(s.tabs.find((t) => t.id === s.activeTab)?.location);
}

export const activeTabOf = (slot: number): PTab | undefined => {
  const p = get().panes[slot];
  return p ? (p.tabs.find((t) => t.id === p.active) ?? p.tabs[0]) : undefined;
};

/** How many panes there are. */
export const paneCount = (tree: Node = get().tree) => leaves(tree).length;

/** The lowest pane number not in use (1 to 7), or null if all are. */
function freeSlot(tree: Node): number | null {
  const used = new Set(leaves(tree));
  for (let i = 1; i < MAX_PANES; i++) if (!used.has(i)) return i;
  return null;
}

/** The panes of a layout other than the normal file list, in the order they are drawn. */
const othersOf = (tree: Node) => leaves(tree).filter((s) => s !== 0);

function withPane(panes: Record<number, PaneState>, slot: number, path: string): Record<number, PaneState> {
  const tab: PTab = { id: uid(), path, back: [], fwd: [] };
  return { ...panes, [slot]: { tabs: [tab], active: tab.id } };
}

// ---- Focus ----------------------------------------------------------------------------------------------

export const focusedPane = () => {
  const { history, tree } = get();
  const open = new Set(leaves(tree));
  return history.find((h) => open.has(h)) ?? 0;
};

export function focusPane(slot: number) {
  set((s) => (s.history[0] === slot ? s : { history: [slot, ...s.history.filter((x) => x !== slot)] }));
}

/** The pane that had the focus before `slot` (the normal file list if none), with the folder it shows. */
export function otherPane(slot: number, folder: string): { slot: number; path: string } {
  const { history, tree } = get();
  const open = new Set(leaves(tree));
  for (const h of [...history, ...leaves(tree)]) {
    if (h === slot || !open.has(h)) continue;
    const path = h === 0 ? folder : activeTabOf(h)?.path;
    if (path) return { slot: h, path };
  }
  return { slot: 0, path: folder };
}

/** Move the focus to the next (1) or previous (-1) pane. */
export function cyclePane(from: number, step: 1 | -1) {
  const slots = leaves(get().tree);
  const next = slots[(slots.indexOf(from) + step + slots.length) % slots.length];
  const el = document.querySelector<HTMLElement>(next === 0 ? '[data-split-pane="0"] .file-scroller, [data-split-pane="0"] .miller' : `[data-split-pane="${next}"]`);
  el?.focus();
  focusPane(next);
}

// ---- Panes and their tabs -------------------------------------------------------------------------------

/** Make sure every pane in the layout has a tab (a new one shows `folder`), and forget the panes that are gone. */
export function ensurePanes(folder: string) {
  const { tree, panes } = get();
  const slots = othersOf(tree);
  let next = panes;
  for (const s of slots) if (!next[s]) next = withPane(next, s, folder);
  const extra = Object.keys(next).map(Number).filter((k) => !slots.includes(k));
  if (extra.length) {
    next = { ...next };
    for (const k of extra) delete next[k];
  }
  if (next !== panes) set({ panes: next });
}

function patchPane(slot: number, fn: (p: PaneState) => PaneState) {
  set((s) => (s.panes[slot] ? { panes: { ...s.panes, [slot]: fn(s.panes[slot]) } } : s));
}

const patchTab = (slot: number, id: string, fn: (t: PTab) => PTab) => patchPane(slot, (p) => ({ ...p, tabs: p.tabs.map((t) => (t.id === id ? fn(t) : t)) }));

/** Show another folder in the tab that is open in a pane. */
export function navigatePane(slot: number, path: string) {
  const tab = activeTabOf(slot);
  if (!tab || tab.path === path) return;
  patchTab(slot, tab.id, (t) => ({ ...t, path, back: [...t.back, t.path].slice(-100), fwd: [] }));
}

export function stepPane(slot: number, step: -1 | 1) {
  const tab = activeTabOf(slot);
  if (!tab) return;
  if (step < 0 && tab.back.length) patchTab(slot, tab.id, (t) => ({ ...t, path: t.back[t.back.length - 1], back: t.back.slice(0, -1), fwd: [t.path, ...t.fwd] }));
  if (step > 0 && tab.fwd.length) patchTab(slot, tab.id, (t) => ({ ...t, path: t.fwd[0], fwd: t.fwd.slice(1), back: [...t.back, t.path] }));
}

export function openPaneTab(slot: number, path?: string, index?: number) {
  const p = get().panes[slot];
  if (!p) return;
  const tab: PTab = { id: uid(), path: path ?? activeTabOf(slot)?.path ?? mainFolder(), back: [], fwd: [] };
  const tabs = [...p.tabs];
  tabs.splice(Math.min(index ?? tabs.length, tabs.length), 0, tab);
  patchPane(slot, () => ({ tabs, active: tab.id }));
}

export function activatePaneTab(slot: number, id: string) {
  patchPane(slot, (p) => (p.tabs.some((t) => t.id === id) ? { ...p, active: id } : p));
}

export function closePaneTab(slot: number, id: string) {
  const p = get().panes[slot];
  if (!p) return;
  if (p.tabs.length <= 1) return closePane(slot);
  const i = p.tabs.findIndex((t) => t.id === id);
  const tabs = p.tabs.filter((t) => t.id !== id);
  patchPane(slot, () => ({ tabs, active: p.active === id ? tabs[Math.min(i, tabs.length - 1)].id : p.active }));
}

export function closeOtherPaneTabs(slot: number, id: string) {
  patchPane(slot, (p) => ({ tabs: p.tabs.filter((t) => t.id === id), active: id }));
}

export function cyclePaneTab(slot: number, step: 1 | -1) {
  patchPane(slot, (p) => {
    const i = p.tabs.findIndex((t) => t.id === p.active);
    return { ...p, active: p.tabs[(i + step + p.tabs.length) % p.tabs.length].id };
  });
}

export function reorderPaneTab(slot: number, id: string, index: number) {
  patchPane(slot, (p) => {
    const tab = p.tabs.find((t) => t.id === id);
    if (!tab) return p;
    const rest = p.tabs.filter((t) => t.id !== id);
    rest.splice(Math.max(0, Math.min(index, rest.length)), 0, tab);
    return { ...p, tabs: rest };
  });
}

/** Close a pane and everything in it; the others take its room. */
export function closePane(slot: number) {
  if (slot === 0) return;
  set((s) => {
    const panes = { ...s.panes };
    delete panes[slot];
    return { tree: removePane(s.tree, slot), panes, history: s.history.filter((h) => h !== slot) };
  });
}

/** Back to one pane. */
export function closeAllPanes() {
  set({ tree: 0, panes: {}, history: [0] });
}

export function setBranchShares(path: string, shares: number[]) {
  set((s) => ({ tree: setTreeShares(s.tree, path, shares) }));
}

export function evenOutPanes() {
  set((s) => ({ tree: evenOut(s.tree) }));
}

// ---- Ready-made layouts ---------------------------------------------------------------------------------

/** The panes in the order a preset takes them: the normal file list first (it gets the large place), then the others. */
const slotsInOrder = (tree: Node) => [0, ...othersOf(tree)];

/** Use a ready-made layout: for as many panes as there are (they are rearranged), or one more (a new pane is added). */
export function applyPreset(preset: Preset) {
  const { tree, panes } = get();
  const n = paneCount(tree);
  if (preset.panes === n) return set({ tree: preset.build(slotsInOrder(tree)) });
  if (preset.panes !== n + 1) return;
  const slot = freeSlot(tree);
  if (slot === null) return;
  set({ tree: preset.build([...slotsInOrder(tree), slot]), panes: withPane(panes, slot, mainFolder()) });
  focusPane(slot);
}

/** The ready-made layouts to offer now: for the panes there are, and for one more. */
export function presetsNow(): { current: Preset[]; more: Preset[]; count: number } {
  const count = paneCount();
  return { count, current: presetsFor(count), more: count < MAX_PANES ? presetsFor(count + 1) : [] };
}

/** Whether a preset has the shape of the layout in use. */
export const isCurrentShape = (preset: Preset) => {
  const t = get().tree;
  return preset.panes === paneCount(t) && shapeOf(preset.build(slotsInOrder(t))) === shapeOf(t);
};
const shapeOf = (n: Node): string => (typeof n === "number" ? "x" : `${n.dir}(${n.kids.map(shapeOf).join(",")})`);

// ---- Moving a tab onto the panes ------------------------------------------------------------------------

export interface Source {
  slot: number;
  tabId: string;
}

function sourcePath(src: Source): string | undefined {
  if (src.slot === 0) return plainFolder(useStore.getState().tabs.find((t) => t.id === src.tabId)?.location);
  return get().panes[src.slot]?.tabs.find((t) => t.id === src.tabId)?.path;
}

/** The pane a tab would empty by leaving it: its only tab, in an extra pane. */
const emptiesOf = (src: Source) => (src.slot > 0 && get().panes[src.slot]?.tabs.length === 1 ? src.slot : null);

/** Take the tab out of where it was. The only tab of the window stays (the pane gets a copy). */
function takeOut(src: Source) {
  if (src.slot === 0) {
    const s = useStore.getState();
    if (s.tabs.length > 1) s.closeTab(src.tabId);
  } else {
    closePaneTab(src.slot, src.tabId);
  }
}

/** Put the tab where the zone says: among the tabs of a pane, or in a new pane beside one. */
export function moveTab(src: Source, zone: Zone): boolean {
  const path = sourcePath(src);
  if (path === undefined) return false;
  if (zone.side === "merge") {
    if (zone.target === src.slot) return false;
    takeOut(src);
    if (zone.target === 0) useStore.getState().newTab(path);
    else {
      const p = get().panes[zone.target];
      if (!p) return false;
      const tab: PTab = { id: uid(), path, back: [], fwd: [] };
      patchPane(zone.target, () => ({ tabs: [...p.tabs, tab], active: tab.id }));
    }
    focusPane(zone.target);
    return true;
  }
  if (zone.target === src.slot && emptiesOf(src) !== null) return false;
  const empties = emptiesOf(src);
  if (empties === null && paneCount() >= MAX_PANES) return false;
  takeOut(src);
  const { tree, panes } = get();
  if (!leaves(tree).includes(zone.target)) return false;
  const slot = freeSlot(tree);
  if (slot === null) return false;
  set({ tree: insertPane(tree, zone.target, zone.side, slot), panes: withPane(panes, slot, path) });
  focusPane(slot);
  return true;
}

/** Put the tab in a new pane, in the places of a ready-made layout for one more pane. */
export function moveTabToPreset(src: Source, presetId: string): boolean {
  const path = sourcePath(src);
  const preset = presetsFor(paneCount() + 1).find((p) => p.id === presetId);
  if (path === undefined || !preset || emptiesOf(src) !== null) return false;
  takeOut(src);
  const { tree, panes } = get();
  const slot = freeSlot(tree);
  if (slot === null) return false;
  set({ tree: preset.build([...slotsInOrder(tree), slot]), panes: withPane(panes, slot, path) });
  focusPane(slot);
  return true;
}

// ---- Dragging a tab over the panes (used by every tab strip) --------------------------------------------

let snapshot: Record<number, Rect> | null = null;

const hostBox = () => document.querySelector<HTMLElement>(".split-host")?.getBoundingClientRect() ?? null;

function takeSnapshot(): Record<number, Rect> {
  const out: Record<number, Rect> = {};
  document.querySelectorAll<HTMLElement>(".split-host [data-leaf]").forEach((el) => {
    const r = el.getBoundingClientRect();
    out[Number(el.dataset.leaf)] = { left: r.left, top: r.top, width: r.width, height: r.height };
  });
  return out;
}

const sameHover = (a: Hover | null, b: Hover | null) => a?.preset === b?.preset && a?.zone?.target === b?.zone?.target && a?.zone?.side === b?.zone?.side;

/**
 * A tab is at (x, y). Returns whether that is over the panes; if so, work out where it would go and let the panes show
 * it (the layout shrinks to make room, with a box where the tab would end up).
 */
export function hoverPanes(x: number, y: number, src: Source, label: string): boolean {
  const s = get();
  const host = hostBox();
  if (!s.allowed || !host) {
    endHover();
    return false;
  }
  if (x < host.left || x > host.right || y < host.top || y > host.bottom) {
    if (s.drag?.hover) set({ drag: { ...s.drag, hover: null } });
    return false;
  }
  snapshot ??= takeSnapshot();
  const empties = emptiesOf(src);
  const full = paneCount() >= MAX_PANES && empties === null;
  const under = document.elementFromPoint(x, y);
  const preset = under?.closest<HTMLElement>("[data-preset]")?.dataset.preset;
  let hover: Hover | null = null;
  if (preset && !full && empties === null) hover = { preset };
  else {
    const strip = under?.closest<HTMLElement>("[data-pane-tabs]");
    if (strip) {
      const target = Number(strip.dataset.paneTabs);
      if (target !== src.slot) hover = { zone: { target, side: "merge" } };
    } else {
      const zone = zoneAt(x, y, snapshot, {
        split: (t) => !full && !(t === src.slot && empties !== null),
        merge: (t) => t !== src.slot && (t === 0 || !!s.panes[t]),
      });
      if (zone) hover = { zone };
    }
  }
  if (!s.drag || s.drag.label !== label || s.drag.empties !== empties || !sameHover(s.drag.hover, hover)) set({ drag: { hover, label, empties } });
  return true;
}

/** Let go over the panes: do what was shown. */
export function dropOnPanes(src: Source): boolean {
  const hover = get().drag?.hover;
  endHover();
  if (!hover) return false;
  if (hover.preset) return moveTabToPreset(src, hover.preset);
  return hover.zone ? moveTab(src, hover.zone) : false;
}

export function endHover() {
  snapshot = null;
  if (get().drag) set({ drag: null });
}

// ---- Keys that act on the pane that has the focus -------------------------------------------------------

/** New tab, close tab and next/previous tab act on the extra pane that has the focus; false if it is the main one. */
export function paneCommand(command: string): boolean {
  const s = get();
  const slot = focusedPane();
  if (!s.allowed || slot === 0 || !s.panes[slot]) return false;
  const tab = activeTabOf(slot);
  switch (command) {
    case "newTab":
      openPaneTab(slot);
      return true;
    case "closeTab":
      if (tab) closePaneTab(slot, tab.id);
      return true;
    case "nextTab":
      cyclePaneTab(slot, 1);
      return true;
    case "prevTab":
      cyclePaneTab(slot, -1);
      return true;
  }
  return false;
}

export { GHOST };
export type { Side };
