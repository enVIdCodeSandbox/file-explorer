// Global UI state. Components subscribe to the slices they need, so a
// transfer progress tick does not re-render the file list.

import { create } from "zustand";
import {
  api,
  type AppInfo,
  type Budget,
  type Conflict,
  type DeleteProgress,
  type Drive,
  type IndexStatus,
  type Job,
  type JobFinished,
  type Pin,
  type Progress,
  type Recent,
  type Session,
  type Settings,
  type Tab,
  type TabGroup,
  type TransferOp,
  type TransferSpec,
} from "./api";
import { describeItems, transferNotice, type Notice, type NoticeKind } from "./util/notices";
import { adv } from "./settings/advanced";
import { steadyFraction } from "./util/format";
import { HOME, THIS_PC, baseName, isAppLocation, joinPath, parentPath, parseLocation } from "./util/path";

const uid = () => Math.random().toString(36).slice(2, 10);

export const GROUP_COLORS = ["#4cc2ff", "#6ccb5f", "#ffb347", "#ff7b72", "#c38fff", "#f778ba", "#9aa0a6"];

export type SortKey = "name" | "modified" | "created" | "type" | "size" | "extension";

/** Narrowing applied to the folder being viewed (reset when you leave it). */
export interface FilterState {
  /** Case-insensitive text the name must contain. */
  text: string;
  /** Categories to keep (from kindOf, plus "folder"); empty keeps all. */
  kinds: string[];
  /** Extensions to keep (lowercase, no dot); empty keeps all. */
  extensions: string[];
  /** "any", "today", "week", "month", "year" or "older". */
  modified: string;
  /** "any", "empty", "small" (<1 MB), "medium" (<100 MB) or "large". */
  size: string;
  /** Look in subfolders too (switches to a search of this folder). */
  subfolders: boolean;
}

export const NO_FILTER: FilterState = { text: "", kinds: [], extensions: [], modified: "any", size: "any", subfolders: false };

/** One speed sample for the transfer graphs. */
export interface Sample {
  /** Seconds since the job began. */
  t: number;
  filesPerSec: number;
  bytesPerSec: number;
}

/** Something Undo can reverse. */
export type UndoAction =
  | { kind: "rename"; from: string; to: string }
  | { kind: "create"; path: string }
  | { kind: "move"; pairs: { from: string; to: string }[] }
  | { kind: "copy"; created: string[] };

export interface Clipboard {
  paths: string[];
  op: TransferOp;
}

export interface DialogRequest {
  title: string;
  message?: string;
  /** Present for text prompts. */
  input?: string;
  /** An extra drop-down under the text box. */
  select?: { label: string; options: { value: string; label: string }[] };
  buttons: { id: string; label: string; primary?: boolean; danger?: boolean }[];
  resolve: (result: { button: string; input: string; selected: string } | null) => void;
}

interface State {
  ready: boolean;
  info: AppInfo | null;
  settings: Settings | null;
  budget: Budget | null;
  indexStatus: IndexStatus | null;
  tabs: Tab[];
  groups: TabGroup[];
  activeTab: string;
  pins: Pin[];
  recent: Recent[];
  drives: Drive[];
  drivesFresh: boolean;
  transfers: Record<number, Progress>;
  /** What each transfer was asked to do, kept for "retry as administrator". */
  specs: Record<number, TransferSpec>;
  clipboard: Clipboard | null;
  /** Bumped per folder path whenever its contents change on disk. */
  dirVersion: Record<string, number>;
  /** Paths selected in the active tab (drives the details pane). */
  selection: string[];
  deletes: Record<number, DeleteProgress>;
  /** Speed history per transfer id, for the graphs. */
  series: Record<number, Sample[]>;
  /** Highest progress shown so far per job, so a bar never goes backwards. */
  shown: Record<number, number>;
  /** The transfers window is expanded (otherwise just the small cards). */
  transfersOpen: boolean;
  /** Which panels are open as their own windows right now. */
  detached: { settings: boolean; transfers: boolean };
  /** Where the in-window Settings panel sits (an offset from the centre) so it comes back where it was left. */
  settingsPos: { x: number; y: number };
  /** True in a window made by dragging a tab out of the main one. */
  isSecondary: boolean;
  /** Open Settings: in its own window if it is detached, otherwise in this one. */
  openSettings: () => void;
  /** Put tabs back into this window, at the place they were taken from. */
  insertTabs: (locations: string[], index?: number) => void;
  sort: { key: SortKey; asc: boolean };
  filter: FilterState;
  /** Desktop, Documents, Downloads… for "Send to". */
  places: { label: string; path: string }[];
  /** Folders opened most often, for the sidebar. */
  frequent: string[];
  /** Paths shown in the app's own Properties window. */
  properties: string[] | null;
  undo: UndoAction[];
  /** Kinds and extensions in the folder being viewed, for the filter menu. */
  facets: { kinds: [string, number][]; extensions: [string, number][] };
  /** A path to select once the listing has loaded (after New folder, say). */
  pendingSelect: string | null;
  /** The window layout is being edited on screen. */
  layoutEdit: boolean;
  /** The popped-out panel that is open, if any. */
  popout: "tabs" | "address" | "commands" | "sidebar" | "details" | null;
  /** A command for the folder view to carry out (select all, rename…). */
  command: { name: "selectAll" | "selectNone" | "invert" | "rename"; tick: number } | null;
  runCommand: (name: "selectAll" | "selectNone" | "invert" | "rename") => void;
  settingsOpen: boolean;
  dialog: DialogRequest | null;
  toast: string | null;
  notices: Notice[];
  noticesOpen: boolean;
  unreadNotices: number;

  init: () => Promise<void>;
  navigate: (location: string, opts?: { tabId?: string; replace?: boolean }) => void;
  back: () => void;
  forward: () => void;
  newTab: (location?: string, opts?: { background?: boolean; groupId?: string | null }) => void;
  closeTab: (id: string) => void;
  moveTab: (id: string, index: number) => void;
  /** Move a tab into its own new window (at a screen position). */
  detachTab: (id: string, x?: number, y?: number) => Promise<void>;
  closeOthers: (id: string) => void;
  duplicateTab: (id: string) => void;
  activate: (id: string) => void;
  groupTab: (tabId: string, groupId: string | null) => void;
  newGroupFromTab: (tabId: string) => void;
  updateGroup: (id: string, patch: Partial<TabGroup>) => void;
  updateSettings: (mutate: (s: Settings) => void) => void;
  togglePin: (path: string, isFile?: boolean) => void;
  addRecent: (path: string, isDir: boolean) => void;
  clearRecent: () => void;
  refreshDrives: () => void;
  setClipboard: (c: Clipboard | null) => void;
  setSelection: (paths: string[]) => void;
  bumpDirs: (dirs: string[]) => void;
  onProgress: (p: Progress) => void;
  onDeleteProgress: (p: DeleteProgress) => void;
  dismissTransfer: (id: number) => void;
  dismissDelete: (id: number) => void;
  setSort: (sort: { key: SortKey; asc: boolean }) => void;
  setFilter: (patch: Partial<FilterState> | null) => void;
  setViewLevel: (level: number) => void;
  refreshFrequent: () => void;
  pushUndo: (a: UndoAction) => void;
  startTransfer: (sources: string[], destDir: string, op: TransferOp) => Promise<void>;
  showToast: (message: string) => void;
  notify: (kind: NoticeKind, title: string, detail?: string) => void;
  dismissNotice: (id: number, forget?: boolean) => void;
  clearNotices: () => void;
}

function makeTab(location: string, groupId: string | null = null): Tab {
  return { id: uid(), location, groupId, history: [location], historyIndex: 0 };
}

let saveTimer: ReturnType<typeof setTimeout> | undefined;
let nextNoticeId = 1;

/** A short, quiet tone. Silently does nothing where audio is unavailable. */
function beep() {
  try {
    const ctx = new AudioContext();
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.frequency.value = 660;
    gain.gain.value = 0.04;
    osc.connect(gain).connect(ctx.destination);
    osc.start();
    osc.stop(ctx.currentTime + 0.08);
    osc.onended = () => void ctx.close();
  } catch {
    /* no audio */
  }
}

let toastTimer: ReturnType<typeof setTimeout> | undefined;

export const useStore = create<State>((set, get) => ({
  ready: false,
  info: null,
  settings: null,
  budget: null,
  indexStatus: null,
  tabs: [],
  groups: [],
  activeTab: "",
  pins: [],
  recent: [],
  drives: [],
  drivesFresh: false,
  transfers: {},
  specs: {},
  clipboard: null,
  dirVersion: {},
  selection: [],
  deletes: {},
  series: {},
  shown: {},
  transfersOpen: false,
  detached: { settings: false, transfers: false },
  settingsPos: { x: 0, y: 0 },
  isSecondary: false,
  openSettings: () => {
    if (get().detached.settings) void api.openSettingsWindow().catch(() => {});
    else set({ settingsOpen: true });
  },
  insertTabs: (locations, index) => {
    if (!locations.length) return;
    const made = locations.map((l) => makeTab(l));
    const tabs = [...get().tabs];
    tabs.splice(Math.min(index ?? tabs.length, tabs.length), 0, ...made);
    set({ tabs, activeTab: made[0].id, selection: [] });
    persist();
  },
  sort: { key: "name", asc: true },
  filter: NO_FILTER,
  frequent: [],
  places: [],
  properties: null,
  undo: [],
  facets: { kinds: [], extensions: [] },
  pendingSelect: null,
  layoutEdit: false,
  popout: null,
  command: null,
  runCommand: (name) => set({ command: { name, tick: (get().command?.tick ?? 0) + 1 } }),
  settingsOpen: false,
  dialog: null,
  toast: null,
  notices: [],
  noticesOpen: false,
  unreadNotices: 0,

  init: async () => {
    const [settings, session, info] = await Promise.all([api.getSettings(), api.getSession(), api.appInfo()]);
    const start = settings.general.startPage === "thisPc" ? THIS_PC : HOME;
    // A window made by dragging a tab out starts with just that tab and does
    // not touch the saved session of the main window.
    const opened = (window as unknown as { __OPEN__?: string }).__OPEN__;
    detachedView = (window as unknown as { __VIEW__?: string }).__VIEW__ === "transfers";
    secondary = typeof opened === "string" || detachedView;
    originIndex = (window as unknown as { __INDEX__?: number | null }).__INDEX__ ?? undefined;
    let tabs = secondary ? [makeTab(opened!)] : settings.general.restoreSession ? session.tabs : [];
    if (!tabs.length) tabs = [makeTab(start)];
    const activeTab = tabs.some((t) => t.id === session.activeTab) ? session.activeTab! : tabs[0].id;
    set({
      ready: true,
      isSecondary: secondary,
      settings,
      info,
      budget: info.budget,
      indexStatus: info.index,
      tabs,
      groups: !secondary && settings.general.restoreSession ? session.groups : [],
      activeTab,
      pins: session.pins ?? [],
      recent: session.recent,
      // The list from the previous run paints immediately; live data follows.
      drives: session.drives,
    });
    get().refreshDrives();
    get().refreshFrequent();
    void api.knownFolders().then((places) => set({ places })).catch(() => {});
  },

  navigate: (location, opts) => {
    const { tabs, activeTab } = get();
    const id = opts?.tabId ?? activeTab;
    const current = tabs.find((t) => t.id === id);
    if (!current || current.location === location) return;
    const from = current.location;
    set({
      tabs: tabs.map((t) => {
        if (t.id !== id) return t;
        if (opts?.replace) {
          const history = [...t.history];
          history[t.historyIndex] = location;
          return { ...t, location, history };
        }
        const history = [...t.history.slice(0, t.historyIndex + 1), location];
        return { ...t, location, history, historyIndex: history.length - 1 };
      }),
      selection: id === activeTab ? [] : get().selection,
      filter: id === activeTab && !location.startsWith("fast://search") ? NO_FILTER : get().filter,
    });
    if (!isAppLocation(location)) {
      get().addRecent(location, true);
      // Teach the predictor and warm the listings it expects next.
      void api.recordVisit(isAppLocation(from) ? null : from, location).catch(() => {});
    }
    persist();
  },

  back: () => step(-1),
  forward: () => step(1),

  newTab: (location, opts) => {
    const s = get();
    const start = s.settings?.general.startPage === "thisPc" ? THIS_PC : HOME;
    const max = adv<number>(s.settings, "tabs.max");
    if (max > 0 && s.tabs.length >= max) return s.showToast(`The tab limit is ${max}. Change it in Settings > Advanced.`);
    const tab = makeTab(location ?? start, opts?.groupId ?? null);
    const after = adv<string>(s.settings, "tabs.newPosition") === "afterCurrent" ? s.tabs.findIndex((t) => t.id === s.activeTab) : -1;
    const tabs = after >= 0 ? [...s.tabs.slice(0, after + 1), tab, ...s.tabs.slice(after + 1)] : [...s.tabs, tab];
    set({ tabs, ...(opts?.background ? {} : { activeTab: tab.id, selection: [] }) });
    if (location && !isAppLocation(location)) get().addRecent(location, true);
    persist();
  },

  closeTab: (id) => {
    const { tabs, activeTab, settings } = get();
    const i = tabs.findIndex((t) => t.id === id);
    if (i < 0) return;
    let next = tabs.filter((t) => t.id !== id);
    if (!next.length) next = [makeTab(settings?.general.startPage === "thisPc" ? THIS_PC : HOME)];
    set({
      tabs: next,
      activeTab: activeTab === id ? next[Math.min(i, next.length - 1)].id : activeTab,
      selection: activeTab === id ? [] : get().selection,
    });
    pruneGroups();
    persist();
  },

  /** Put a tab at `index` (counted among the tabs without it). */
  moveTab: (id, index) => {
    const { tabs } = get();
    const tab = tabs.find((t) => t.id === id);
    if (!tab) return;
    const rest = tabs.filter((t) => t.id !== id);
    rest.splice(Math.max(0, Math.min(rest.length, index)), 0, tab);
    set({ tabs: rest });
    persist();
  },

  detachTab: async (id, x, y) => {
    const { tabs } = get();
    const tab = tabs.find((t) => t.id === id);
    if (!tab || tabs.length < 2) return;
    try {
      await api.openWindow(tab.location, x, y, tabs.findIndex((t) => t.id === id));
      get().closeTab(id);
    } catch (e) {
      get().showToast(String(e));
    }
  },

  closeOthers: (id) => {
    set({ tabs: get().tabs.filter((t) => t.id === id), activeTab: id });
    pruneGroups();
    persist();
  },

  duplicateTab: (id) => {
    const { tabs } = get();
    const i = tabs.findIndex((t) => t.id === id);
    if (i < 0) return;
    const copy: Tab = { ...tabs[i], id: uid(), history: [...tabs[i].history] };
    set({ tabs: [...tabs.slice(0, i + 1), copy, ...tabs.slice(i + 1)], activeTab: copy.id, selection: [] });
    persist();
  },

  activate: (id) => {
    if (get().activeTab !== id) set({ activeTab: id, selection: [] });
    persist();
  },

  groupTab: (tabId, groupId) => {
    const tabs = get().tabs.map((t) => (t.id === tabId ? { ...t, groupId } : t));
    // Keep members of a group next to each other, like browser tab groups.
    const ordered: Tab[] = [];
    const placed = new Set<string>();
    for (const t of tabs) {
      if (placed.has(t.id)) continue;
      if (t.groupId) {
        for (const m of tabs) if (m.groupId === t.groupId) (ordered.push(m), placed.add(m.id));
      } else {
        ordered.push(t);
        placed.add(t.id);
      }
    }
    set({ tabs: ordered });
    pruneGroups();
    persist();
  },

  newGroupFromTab: (tabId) => {
    const { groups, tabs } = get();
    const tab = tabs.find((t) => t.id === tabId);
    if (!tab) return;
    const group: TabGroup = {
      id: uid(),
      name: tabTitle(tab.location),
      color: GROUP_COLORS[groups.length % GROUP_COLORS.length],
      collapsed: false,
    };
    set({ groups: [...groups, group] });
    get().groupTab(tabId, group.id);
  },

  updateGroup: (id, patch) => {
    set({ groups: get().groups.map((g) => (g.id === id ? { ...g, ...patch } : g)) });
    persist();
  },

  updateSettings: (mutate) => {
    const current = get().settings;
    if (!current) return;
    const next: Settings = structuredClone(current);
    mutate(next);
    set({ settings: next });
    void api.setSettings(next).catch((e) => get().showToast(String(e)));
  },

  togglePin: (path, isFile = false) => {
    const { pins } = get();
    const exists = pins.some((p) => p.path === path);
    set({
      pins: exists
        ? pins.filter((p) => p.path !== path)
        : [...pins, { path, label: baseName(path), builtin: false, isFile }],
    });
    persist();
  },

  addRecent: (path, isDir) => {
    const recent = [{ path, isDir, at: Date.now() }, ...get().recent.filter((r) => r.path !== path)].slice(0, adv<number>(get().settings, "files.recentLimit"));
    set({ recent });
    persist();
  },

  clearRecent: () => {
    set({ recent: [] });
    persist();
  },

  refreshDrives: () => {
    void api
      .getDrives()
      .then((drives) => set({ drives, drivesFresh: true }))
      .catch(() => {});
  },

  setClipboard: (clipboard) => {
    set({ clipboard });
    if (clipboard && clipboard.paths.length) get().notify(clipboard.op === "move" ? "cut" : "copy", `${clipboard.op === "move" ? "Cut" : "Copied"} ${describeItems(clipboard.paths)}`, "Ready to paste");
  },
  setSelection: (selection) => set({ selection }),

  bumpDirs: (dirs) => {
    const dirVersion = { ...get().dirVersion };
    for (const d of dirs) dirVersion[d] = (dirVersion[d] ?? 0) + 1;
    set({ dirVersion });
  },

  onProgress: (p) => {
    const prev = get().shown[p.id] ?? 0;
    const fraction = steadyFraction(prev, p);
    const list = get().series[p.id] ?? [];
    let series = get().series;
    if (p.phase === "running" && p.elapsedSecs > (list.at(-1)?.t ?? -1) + 0.2) {
      const next = [...list, { t: p.elapsedSecs, filesPerSec: p.filesPerSec, bytesPerSec: p.bytesPerSec }];
      // Keep the graph light on very long jobs: thin out every other sample.
      series = { ...series, [p.id]: next.length > 900 ? next.filter((_, i) => i % 2 === 0 || i === next.length - 1) : next };
    }
    set({ transfers: { ...get().transfers, [p.id]: p }, shown: { ...get().shown, [p.id]: fraction }, series });
    if (p.phase === "done" && p.errorCount === 0) {
      // Successful jobs leave on their own; failures stay until dismissed.
      const secs = adv<number>(get().settings, "transfers.dismissSecs");
      if (secs > 0) setTimeout(() => get().dismissTransfer(p.id), secs * 1000);
      const spec = get().specs[p.id];
      // The window that started the job reports it; the transfers window only displays.
      if (detachedView) return;
      if (spec) {
        const t = transferNotice(spec.op === "move" ? "move" : "copy", spec.sources, spec.destDir);
        get().notify(spec.op === "move" ? "move" : "copy", t.title, t.detail);
      } else get().notify("info", `Finished ${p.filesDone} item${p.filesDone === 1 ? "" : "s"}`);
    }
    if (p.phase === "running" && !prev && adv<boolean>(get().settings, "transfers.openWindow")) set({ transfersOpen: true });
  },

  onDeleteProgress: (p) => {
    const ratio = p.itemsTotal > 0 ? p.itemsDone / p.itemsTotal : 0;
    const shown = steadyFraction(get().shown[p.id] ?? 0, { phase: p.phase === "done" ? "done" : "running", fraction: p.phase === "scanning" ? 0 : ratio });
    set({ deletes: { ...get().deletes, [p.id]: p }, shown: { ...get().shown, [p.id]: shown } });
    if (p.phase === "done" && p.errorCount === 0) setTimeout(() => get().dismissDelete(p.id), 3000);
    if (p.phase === "done" || p.phase === "cancelled") {
      const what = p.selected === 1 && p.firstName ? `"${p.firstName}"` : `${p.selected} items`;
      const where = p.permanent ? "permanently" : `to the ${get().info?.os === "windows" ? "Recycle Bin" : "Trash"}`;
      if (p.phase === "done") get().notify("delete", `Deleted ${what}`, p.errorCount ? `${p.errorCount} could not be deleted` : p.permanent ? "Permanently" : `Moved ${where}`);
      else get().notify("info", `Delete stopped`, `${p.itemsDone} of ${p.itemsTotal} done`);
    }
  },

  dismissTransfer: (id) => {
    const transfers = { ...get().transfers };
    delete transfers[id];
    set({ transfers });
  },

  dismissDelete: (id) => {
    const deletes = { ...get().deletes };
    delete deletes[id];
    set({ deletes });
  },

  setSort: (sort) => set({ sort }),

  setFilter: (patch) => set({ filter: patch ? { ...get().filter, ...patch } : NO_FILTER }),

  setViewLevel: (level) => {
    const next = Math.max(0, Math.min(VIEW_LEVELS.length - 1, level));
    if (next === get().settings?.appearance.viewLevel) return;
    get().updateSettings((s) => void (s.appearance.viewLevel = next));
  },

  refreshFrequent: () => {
    void api
      .frequentFolders(Math.min(60, adv<number>(get().settings, "files.frequentCount") + 10))
      .then((frequent) => set({ frequent }))
      .catch(() => {});
  },

  pushUndo: (a) => {
    const limit = adv<number>(get().settings, "files.undoLimit");
    set({ undo: limit > 0 ? [...get().undo.slice(-(limit - 1)), a] : [] });
  },

  startTransfer: async (sources, destDir, op) => {
    const s = get();
    if (!sources.length || !s.settings) return;
    let conflict: Conflict = "keepBoth";
    const policy = s.settings.transfer.conflict;
    const clashes = await api.checkConflicts(sources, destDir);
    if (clashes.length) {
      if (policy === "ask") {
        const answer = await ask({
          title: `${clashes.length} item${clashes.length === 1 ? "" : "s"} already exist${clashes.length === 1 ? "s" : ""} here`,
          message: clashes.slice(0, 6).join("\n") + (clashes.length > 6 ? `\n…and ${clashes.length - 6} more` : ""),
          buttons: [
            { id: "overwrite", label: "Replace" },
            { id: "skip", label: "Skip" },
            { id: "keepBoth", label: "Keep both", primary: true },
            { id: "cancel", label: "Cancel" },
          ],
        });
        if (!answer || answer.button === "cancel") return;
        conflict = answer.button as Conflict;
      } else {
        conflict = policy;
      }
    } else if (policy !== "ask") {
      conflict = policy;
    }
    const spec: TransferSpec = { sources, destDir, op, conflict };
    try {
      const job: Job = { kind: "transfer", spec };
      const mode = await guardAccess(job, `${op === "move" ? "Moving" : "Copying"} to ${baseName(destDir)}`);
      if (mode === "cancel") return;
      const id = mode === "elevated" ? await api.elevatedStart(job) : await api.transferStart(spec);
      set({ specs: { ...get().specs, [id]: spec } });
      const landed = sources.map((src) => ({ from: src, to: joinPath(destDir, baseName(src)) }));
      get().pushUndo(op === "move" ? { kind: "move", pairs: landed } : { kind: "copy", created: landed.map((l) => l.to) });
    } catch (e) {
      s.showToast(String(e));
    }
  },

  notify: (kind, title, detail) => {
    const settings = get().settings;
    if (!settings || !adv<boolean>(settings, "notify.enabled")) return;
    const id = nextNoticeId++;
    const bell = adv<string>(settings, "notify.mode") === "bell";
    const keep = Math.max(5, adv<number>(settings, "notify.keep"));
    const notice: Notice = { id, kind, title, detail, at: Date.now(), popup: !bell };
    set({
      notices: [...get().notices, notice].slice(-keep),
      unreadNotices: get().noticesOpen ? 0 : get().unreadNotices + 1,
    });
    if (adv<boolean>(settings, "notify.sound")) beep();
    if (!bell) setTimeout(() => get().dismissNotice(id), adv<number>(settings, "notify.seconds") * 1000);
  },

  /** Hide a pop-up; with `forget` also remove it from the list. */
  dismissNotice: (id, forget = false) =>
    set({ notices: forget ? get().notices.filter((n) => n.id !== id) : get().notices.map((n) => (n.id === id ? { ...n, popup: false } : n)) }),

  clearNotices: () => set({ notices: [], unreadNotices: 0 }),

  showToast: (toast) => {
    set({ toast });
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => set({ toast: null }), 5000);
  },
}));

function step(delta: number) {
  const { tabs, activeTab } = useStore.getState();
  useStore.setState({
    tabs: tabs.map((t) => {
      if (t.id !== activeTab) return t;
      const i = t.historyIndex + delta;
      if (i < 0 || i >= t.history.length) return t;
      return { ...t, historyIndex: i, location: t.history[i] };
    }),
    selection: [],
  });
  persist();
}

function pruneGroups() {
  const { tabs, groups } = useStore.getState();
  const used = new Set(tabs.map((t) => t.groupId));
  if (groups.some((g) => !used.has(g.id))) useStore.setState({ groups: groups.filter((g) => used.has(g.id)) });
}

/** Save tabs, pins and recents shortly after they change. */
/** True in windows made by dragging a tab out of another. */
let secondary = false;
/** Where in the main window's tab bar a detached tab came from. */
let originIndex: number | undefined;
export const tabOrigin = () => originIndex;
/** True in the separate transfers window, which only shows progress. */
let detachedView = false;

function persist() {
  if (secondary) return;
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    const s = useStore.getState();
    const session: Session = {
      tabs: s.tabs,
      groups: s.groups,
      activeTab: s.activeTab,
      pins: s.pins,
      recent: s.recent,
      drives: [],
    };
    void api.saveSession(session).catch(() => {});
  }, 400);
}

export function tabTitle(location: string): string {
  const os = useStore.getState().info?.os;
  const l = parseLocation(location);
  switch (l.kind) {
    case "home":
      return "Home";
    case "thispc":
      return thisPcLabel(os);
    case "search":
      return `Search: ${l.query}`;
    case "analyze":
      return `Disk usage: ${baseName(l.path)}`;
    default:
      return baseName(l.path);
  }
}

export function thisPcLabel(os: string | undefined): string {
  return os === "windows" ? "This PC" : os === "macos" ? "This Mac" : "This Computer";
}

/** Show a modal with buttons; resolves with the chosen button or null. */
export function ask(req: Omit<DialogRequest, "resolve">): Promise<{ button: string; input: string; selected: string } | null> {
  return new Promise((resolve) => {
    useStore.setState({
      dialog: {
        ...req,
        resolve: (r) => {
          useStore.setState({ dialog: null });
          resolve(r);
        },
      },
    });
  });
}

export async function promptText(title: string, initial: string): Promise<string | null> {
  const r = await ask({
    title,
    input: initial,
    buttons: [
      { id: "ok", label: "OK", primary: true },
      { id: "cancel", label: "Cancel" },
    ],
  });
  return r && r.button === "ok" && r.input.trim() ? r.input.trim() : null;
}

/** Folder views, from the most detail to the biggest icons. */
export const VIEW_LEVELS = ["details", "list", "small", "medium", "large", "xl"] as const;
export type ViewLevel = (typeof VIEW_LEVELS)[number];
/** Thumbnail edge in pixels for each icon level (Windows uses 48/96/256). */
export const ICON_PX: Record<string, number> = { small: 32, medium: 64, large: 112, xl: 200 };

// ---------------------------------------------------------------------------
// Administrator rights
// ---------------------------------------------------------------------------

const finishedJobs = new Map<number, JobFinished>();
const waiting = new Map<number, (f: JobFinished) => void>();

export function onJobFinished(f: JobFinished) {
  const resolve = waiting.get(f.id);
  if (resolve) {
    waiting.delete(f.id);
    resolve(f);
  } else finishedJobs.set(f.id, f);
  const { showToast } = useStore.getState();
  if (f.declined) showToast("Not run: administrator permission was not granted.");
  else if (!f.ok && f.error) showToast(f.error);
}

/** Resolves when an elevated job reports back (used for rename and create). */
export function jobResult(id: number): Promise<JobFinished> {
  const done = finishedJobs.get(id);
  if (done) {
    finishedJobs.delete(id);
    return Promise.resolve(done);
  }
  return new Promise((resolve) => waiting.set(id, resolve));
}

/**
 * Ask the backend whether the current user would be refused. If so, explain
 * and ask first: the operating system's own prompt (UAC, polkit, macOS
 * password) follows only when the person agrees, and nothing runs otherwise.
 * "normal" means run it as usual, "elevated" means start it through the
 * elevated route, "cancel" means do nothing.
 */
export async function guardAccess(job: Job, what: string): Promise<"normal" | "elevated" | "cancel"> {
  const check = await api.checkAccess(job).catch(() => ({ needsElevation: false, becomesPermanent: false }));
  if (!check.needsElevation) return "normal";
  const os = useStore.getState().info?.os;
  const prompt = os === "windows" ? "Windows will ask you to confirm" : os === "macos" ? "macOS will ask for your password" : "You will be asked for your password";
  const r = await ask({
    title: "Administrator permission needed",
    message:
      `${what} needs administrator rights. ${prompt} before anything happens; if you decline, nothing is changed.` +
      (check.becomesPermanent ? "\n\nAs administrator, deleted items cannot go to the Recycle Bin/Trash and will be deleted permanently." : ""),
    buttons: [
      { id: "ok", label: "Continue as administrator", primary: true, danger: check.becomesPermanent },
      { id: "cancel", label: "Cancel" },
    ],
  });
  return r?.button === "ok" ? "elevated" : "cancel";
}

/** Run a job as administrator after the person has agreed; waits for its result. */
export async function runElevated(job: Job): Promise<JobFinished> {
  const id = await api.elevatedStart(job);
  return jobResult(id);
}

/** Retry a transfer that failed for lack of permission. */
export async function retryTransferAsAdmin(spec: TransferSpec): Promise<void> {
  const job: Job = { kind: "transfer", spec: { ...spec, conflict: "skip" } };
  const mode = await guardAccess(job, "Finishing this transfer");
  if (mode === "cancel") return;
  try {
    if (mode === "elevated") await api.elevatedStart(job);
    else await api.transferStart(job.spec);
  } catch (e) {
    useStore.getState().showToast(String(e));
  }
}

// ---------------------------------------------------------------------------
// File operations the UI shares
// ---------------------------------------------------------------------------

/** Delete with the user's confirmation and trash preferences applied. */
export async function deletePaths(paths: string[], forcePermanent = false): Promise<boolean> {
  const s = useStore.getState();
  if (!paths.length || !s.settings) return false;
  const permanent = forcePermanent || !s.settings.general.deleteToTrash;
  const forced = adv<number>(s.settings, "files.confirmDeleteOver");
  if (s.settings.general.confirmDelete || permanent || (forced > 0 && paths.length >= forced)) {
    const what = paths.length === 1 ? `"${baseName(paths[0])}"` : `${paths.length} items`;
    const r = await ask({
      title: permanent ? `Permanently delete ${what}?` : `Move ${what} to the ${s.info?.os === "windows" ? "Recycle Bin" : "Trash"}?`,
      message: permanent ? "This cannot be undone." : undefined,
      buttons: [
        { id: "ok", label: "Delete", primary: true, danger: permanent },
        { id: "cancel", label: "Cancel" },
      ],
    });
    if (!r || r.button !== "ok") return false;
  }
  try {
    const job: Job = { kind: "delete", paths, permanent };
    const mode = await guardAccess(job, "Deleting these items");
    if (mode === "cancel") return false;
    if (mode === "elevated") await api.elevatedStart(job);
    else await api.deleteStart(paths, permanent);
    return true;
  } catch (e) {
    s.showToast(String(e));
    return false;
  }
}

export async function renamePath(path: string, newName: string): Promise<string | null> {
  const { showToast, pushUndo } = useStore.getState();
  try {
    const job: Job = { kind: "rename", path, newName };
    const mode = await guardAccess(job, `Renaming "${baseName(path)}"`);
    if (mode === "cancel") return null;
    let made: string | null;
    if (mode === "elevated") {
      const r = await runElevated(job);
      made = r.ok ? r.path : null;
    } else made = await api.renameItem(path, newName);
    if (made) {
      pushUndo({ kind: "rename", from: path, to: made });
      useStore.getState().notify("rename", `Renamed "${baseName(path)}"`, `to "${baseName(made)}"`);
    }
    return made;
  } catch (e) {
    showToast(String(e));
    return null;
  }
}

export async function createPath(dir: string, name: string, folder: boolean): Promise<string | null> {
  const { showToast, pushUndo } = useStore.getState();
  try {
    const job: Job = { kind: "create", dir, name, folder };
    const mode = await guardAccess(job, `Creating "${name}" here`);
    if (mode === "cancel") return null;
    let made: string | null;
    if (mode === "elevated") {
      const r = await runElevated(job);
      made = r.ok ? r.path : null;
    } else made = await api.createItem(dir, name, folder);
    if (made) {
      pushUndo({ kind: "create", path: made });
      useStore.getState().notify("create", `Created ${folder ? "folder" : "file"} "${baseName(made)}"`);
    }
    return made;
  } catch (e) {
    showToast(String(e));
    return null;
  }
}

/** The OS properties window when there is one; otherwise the app's own. */
export async function showProperties(paths: string[]): Promise<void> {
  if (!paths.length) return;
  const native = await api.showProperties(paths).catch(() => false);
  if (!native) useStore.setState({ properties: paths });
}

/** Reverse the last rename, create, move or copy. Deleting is not undoable. */
export async function undoLast(): Promise<void> {
  const s = useStore.getState();
  const action = s.undo.at(-1);
  if (!action) return;
  useStore.setState({ undo: s.undo.slice(0, -1) });
  s.notify("undo", `Undid ${({ rename: "rename", create: "create", copy: "copy", move: "move" } as const)[action.kind]}`);
  try {
    if (action.kind === "rename") await api.renameItem(action.to, baseName(action.from));
    else if (action.kind === "create") await deletePaths([action.path]);
    else if (action.kind === "copy") await deletePaths(action.created);
    else {
      for (const p of action.pairs) await api.transferStart({ sources: [p.to], destDir: parentOf(p.from), op: "move", conflict: "keepBoth" });
    }
  } catch (e) {
    s.showToast(String(e));
  }
}

function parentOf(path: string): string {
  return parentPath(path) ?? path;
}
