// Global UI state. Components subscribe to the slices they need, so a
// transfer progress tick does not re-render the file list.
import { isMobileOs, openOutside } from "./util/native";
import { create } from "zustand";
import {
  api,
  type AppInfo,
  type Budget,
  type Conflict,
  type ConflictInfo,
  type DeleteProgress,
  type Device,
  type Drive,
  type IndexStatus,
  type Job,
  type JobFinished,
  type Launch,
  type Pin,
  type Progress,
  type Recent,
  type SavedServer,
  type Session,
  type Settings,
  type Tab,
  type TabGroup,
  type TransferOp,
  type TransferSpec,
} from "./api";
import { METHOD_NAME, jobGroup, noteMethod } from "./util/methods";
import { DEFAULT_SET, normalizeSet, setFromLegacyWidths, type ColumnSet } from "./util/columns";
import { loadViews, saveViews, withView, withoutColumns, type FolderViews } from "./folderViews";
import { describeItems, itemFacts, transferNotice, type Notice, type NoticeKind } from "./util/notices";
import { adv } from "./settings/advanced";
import { syncDetailsPane } from "./util/layout";
import { orderRenames, type RenameInput, type RenamePlan } from "./util/batchRename";
import { formatDate, formatDuration, formatSize, steadyFraction } from "./util/format";
import { HOME, THIS_PC, splitArchive, baseName, insideArchive, isInside, isAppLocation, isArchiveFile, isArchiveLoc, isRemote, joinPath, parentPath, parseLocation } from "./util/path";
import { moveGroupBlock } from "./util/tabs";
import { setAddressPrivacy, setDeviceNames } from "./util/path";
import { snapshotOf } from "./settings/presets";
import { isFiltering } from "./util/filter";

/** The columns shown by default: what was chosen last, or the widths saved before columns could be chosen. */
function loadColumnSet(): ColumnSet {
  try {
    const raw = localStorage.getItem("fast.columnSet");
    if (raw) return normalizeSet(JSON.parse(raw));
    const old = localStorage.getItem("fast.columns");
    if (old) return setFromLegacyWidths(JSON.parse(old));
  } catch {
    /* fall through to the defaults */
  }
  return DEFAULT_SET;
}
function saveColumnSet(set: ColumnSet) {
  try {
    localStorage.setItem("fast.columnSet", JSON.stringify(set));
  } catch {
    /* kept until the app closes */
  }
}

const uid = () => Math.random().toString(36).slice(2, 10);

export const GROUP_COLORS = ["#4cc2ff", "#6ccb5f", "#ffb347", "#ff7b72", "#c38fff", "#f778ba", "#9aa0a6"];

/** A column's id (see util/columns.ts): the folder can be ordered by any column it shows. */
export type SortKey = string;

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
  // The same questions the other way round: what must NOT match.
  /** Case-insensitive text the name must not contain. */
  notText: string;
  /** Categories to leave out. */
  notKinds: string[];
  /** Extensions to leave out. */
  notExtensions: string[];
  /** Leave out files modified in this range ("any" leaves out nothing). */
  notModified: string;
  /** Leave out files of this size class ("any" leaves out nothing). */
  notSize: string;
}

export const NO_FILTER: FilterState = {
  text: "", kinds: [], extensions: [], modified: "any", size: "any", subfolders: false,
  notText: "", notKinds: [], notExtensions: [], notModified: "any", notSize: "any",
};

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
  /** Several items renamed at once, by full path before and after. */
  | { kind: "renameMany"; pairs: { from: string; to: string }[] }
  | { kind: "create"; path: string }
  | { kind: "move"; pairs: { from: string; to: string }[] }
  | { kind: "copy"; created: string[] }
  /** Items moved to the Trash, by the path they had. */
  | { kind: "trash"; paths: string[] };

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

/** Asks for the Connect to server dialog, optionally with fields filled in. */
/** A request to choose a local file, answered with its path or null when cancelled. */
export interface PickRequest {
  title: string;
  /** A file (default) or a folder. */
  kind?: "file" | "folder";
  done: (path: string | null) => void;
}

export interface ConnectRequest {
  /** A server address to log in to (the saved login was lost, say). */
  url?: string;
  /** Fields from a saved server. */
  saved?: SavedServer;
  /** Something to tell the person first. */
  message?: string;
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
  /** Earlier searches, newest first. */
  searches: string[];
  drives: Drive[];
  /** Phones, cameras and other devices that are not drives. */
  devices: Device[];
  /** Wired, wireless or unknown: decides thumbnails on network shares in automatic mode. */
  link: "wired" | "wireless" | "unknown";
  drivesFresh: boolean;
  transfers: Record<number, Progress>;
  /** What each transfer was asked to do, kept for "retry as administrator". */
  specs: Record<number, TransferSpec>;
  clipboard: Clipboard | null;
  /** Bumped per folder path whenever its contents change on disk. */
  dirVersion: Record<string, number>;
  /** The folder being read right now, while some of its items are not in yet. */
  loadingDir: import("./util/progressive").LoadProgress | null;
  /** Pastes waiting for a delete they would collide with. */
  queuedJobs: Record<number, { title: string; detail: string }>;
  unqueue: (id: number) => void;
  /** Paths selected in the active tab (drives the details pane). */
  selection: string[];
  deletes: Record<number, DeleteProgress>;
  /** Speed history per transfer id, for the graphs. */
  series: Record<number, Sample[]>;
  /** The method the latest job of each kind used (by the kind's name), for Settings > Advanced > Transfers. */
  lastMethods: Record<string, { method: string; what: string }>;
  /** Highest progress shown so far per job, so a bar never goes backwards. */
  shown: Record<number, number>;
  /** The transfers window is expanded (otherwise just the small cards). */
  transfersOpen: boolean;
  /** Which panels are open as their own windows right now. */
  detached: { settings: boolean; transfers: boolean };
  /** Where the in-window Settings panel sits (an offset from the center) so it comes back where it was left. */
  settingsPos: { x: number; y: number };
  /** True in a window made by dragging a tab out of the main one. */
  isSecondary: boolean;
  /** Open Settings: in its own window if it is detached, otherwise in this one. */
  openSettings: () => void;
  /** Put tabs back into this window, at the place they were taken from. */
  insertTabs: (locations: string[], index?: number) => void;
  sort: { key: SortKey; asc: boolean };
  filter: FilterState;
  /** The columns shown where a folder has none of its own. */
  columnSet: ColumnSet;
  /** Sort order, filter and columns remembered for each folder. */
  folderViews: FolderViews;
  chooseDetailsOpen: boolean;
  /** A view picked for the open folder only (when views are not shared between folders). */
  folderView: number | null;
  /** Desktop, Documents, Downloads… for "Send to". */
  places: { label: string; path: string }[];
  /** Folders opened most often, for the sidebar. */
  frequent: string[];
  /** Paths shown in the app's own Properties window. */
  properties: string[] | null;
  /** The Rename dialog for several items: the folder, what is selected and what else is in it. */
  batchRename: { dir: string; items: RenameInput[]; siblings: string[] } | null;
  /** Server addresses kept in the sidebar (no passwords). */
  servers: SavedServer[];
  /** The "Connect to server" dialog, with what to fill in. */
  connect: ConnectRequest | null;
  /** While set, the window is choosing a file for a form (see util/pickFile.ts): opening a file picks it. */
  picker: PickRequest | null;
  openConnect: (request?: ConnectRequest) => void;
  closeConnect: () => void;
  setServers: (servers: SavedServer[]) => void;
  /** Files the "Share a download link" dialog is open for. */
  shareFor: string[] | null;
  undo: UndoAction[];
  /** What Undo took back, for Redo. Any new action empties it. */
  redo: UndoAction[];
  /** Kinds and extensions in the folder being viewed, for the filter menu. */
  facets: { kinds: [string, number][]; extensions: [string, number][] };
  /** A path to select once the listing has loaded (after New folder, say). */
  pendingSelect: string | null;
  /** More items to highlight together with `pendingSelect`. */
  pendingSelectMore: string[];
  /** Show folders another app (or the command line) asked for, highlighting items in them. */
  showLaunches: (launches: Launch[]) => void;
  /** The window layout is being edited on screen. */
  layoutEdit: boolean;
  /** The popped-out panel that is open, if any. */
  popout: "tabs" | "address" | "commands" | "sidebar" | "details" | null;
  /** A command for the folder view to carry out (select all, rename…). */
  command: { name: FolderCommand; tick: number } | null;
  runCommand: (name: FolderCommand) => void;
  /** A file shown in the app's own viewer (phones without an "open with" sheet), or null. */
  viewing: string | null;
  /** Phone layout: the open folder is in "select" mode (ticks on every item, actions along the bottom). */
  selecting: boolean;
  settingsOpen: boolean;
  /** The Tasks window (the button next to Share), and a task it should open already filled in. */
  tasksOpen: boolean;
  tasksPrefill: import("./tasks/model").Task | null;
  /** Search text Settings should open with. */
  settingsQuery: string;
  openTasks: (prefill?: import("./tasks/model").Task | null) => void;
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
  /** Tabs closed in this window, newest last, with where each one was. */
  closedTabs: { tab: Tab; index: number }[];
  /** Bring back the tab closed last (Ctrl+Shift+T), with its history. */
  reopenTab: () => void;
  moveTab: (id: string, index: number) => void;
  moveGroup: (groupId: string, index: number) => void;
  /** Move a tab into its own new window (at a screen position). */
  detachTab: (id: string, x?: number, y?: number) => Promise<void>;
  closeOthers: (id: string) => void;
  duplicateTab: (id: string) => void;
  activate: (id: string) => void;
  groupTab: (tabId: string, groupId: string | null) => void;
  newGroupFromTab: (tabId: string) => void;
  updateGroup: (id: string, patch: Partial<TabGroup>) => void;
  updateSettings: (mutate: (s: Settings) => void) => void;
  /** With "Save changes automatically" off: changes are applied here but not yet saved. */
  /** How far the next search reaches, once the person chose (shortcut or the search box); null = the setting decides. */
  searchMode: import("./util/path").SearchMode | null;
  setSearchMode: (m: import("./util/path").SearchMode | null) => void;
  unsaved: boolean;
  savedSettings: Settings | null;
  saveSettings: () => void;
  discardSettings: () => void;
  togglePin: (path: string, isFile?: boolean) => void;
  /** Pin at a position (moving it if already pinned). */
  pinAt: (path: string, index: number, isFile?: boolean) => void;
  /** Move a pin to a new position (counted before the move). */
  movePin: (from: number, to: number) => void;
  addRecent: (path: string, isDir: boolean) => void;
  /** The "Open with" chooser: the file, and why it is being asked (empty when the person asked for it). */
  openWith: { path: string; reason: string } | null;
  /** The folder or drive the "Lock with passcode" dialog is open for. */
  lockDialog: string | null;
  /** A tab another window is dragging over this one: the slot it would take. */
  incomingTab: { index: number; w: number } | null;
  openLockDialog: (path: string) => void;
  chooseApp: (path: string, reason?: string) => void;
  closeChooser: () => void;
  clearRecent: () => void;
  addSearch: (text: string) => void;
  removeSearch: (text: string) => void;
  clearSearches: () => void;
  refreshDrives: () => void;
  refreshLink: () => void;
  onDrivesChanged: (p: { drives: Drive[]; devices: Device[]; added: string[]; removed: string[] }) => void;
  setClipboard: (c: Clipboard | null) => void;
  /** Paste what is on the clipboard (from this app or any other) into a folder. */
  pasteInto: (dest: string) => Promise<void>;
  setSelection: (paths: string[]) => void;
  bumpDirs: (dirs: string[]) => void;
  onProgress: (p: Progress) => void;
  onDeleteProgress: (p: DeleteProgress) => void;
  /** Cancel requested, by job id and when (so a stuck cancel can offer "Force stop"). */
  cancelling: Record<number, number>;
  requestCancel: (kind: "transfer" | "delete", id: number) => void;
  /** Abandon a job right now, without waiting for it to respond. */
  forceStop: (kind: "transfer" | "delete", id: number) => void;
  dismissTransfer: (id: number) => void;
  dismissDelete: (id: number) => void;
  setSort: (sort: { key: SortKey; asc: boolean }) => void;
  /** Change the columns: for the open folder (or every folder when views are not kept per folder), or for all folders. */
  setColumns: (set: ColumnSet, scope: "here" | "all") => void;
  setFilter: (patch: Partial<FilterState> | null) => void;
  setViewLevel: (level: number) => void;
  refreshFrequent: () => void;
  pushUndo: (a: UndoAction) => void;
  startTransfer: (sources: string[], destDir: string, op: TransferOp) => Promise<void>;
  showToast: (message: string) => void;
  notify: (kind: NoticeKind, title: string, detail?: string, more?: { facts?: [string, string][]; names?: string[] }) => void;
  dismissNotice: (id: number, forget?: boolean) => void;
  clearNotices: () => void;
}

/** Set while a tab leaves for another window: that is a move, not a close. */
let skipRemembering = false;

/** Keep a closed tab so Ctrl+Shift+T can bring it back. */
function rememberClosed(tab: Tab, index: number) {
  if (skipRemembering) return;
  const keep = adv<number>(useStore.getState().settings, "tabs.reopenKeep");
  if (keep <= 0) return;
  // A tab still on a start page with no history says nothing worth bringing back.
  if (tab.history.length === 1 && isAppLocation(tab.location) && tab.location === HOME) return;
  useStore.setState({ closedTabs: [...useStore.getState().closedTabs, { tab, index }].slice(-keep) });
}

function makeTab(location: string, groupId: string | null = null): Tab {
  return { id: uid(), location, groupId, history: [location], historyIndex: 0 };
}

let saveTimer: ReturnType<typeof setTimeout> | undefined;
let nextNoticeId = 1;
/** Deletes that went to the Trash, by job id, so a finished one can be undone. */
const trashedBy = new Map<number, string[]>();
/** Where each running delete is taking items from, to reload those folders when it ends. */
const deletedFrom = new Map<number, string[]>();
/** What a running delete or copy is about to take away or put down, for ordering later jobs after it. */
interface Footprint {
  removes: string[];
  writes?: { dir: string; names: string[] };
  /** The order jobs were asked for in: a job waits only for those asked for before it. */
  seq?: number;
}
/** Running jobs by key: "d" + id for deletes, "t" + id for copies and moves. */
const inFlight = new Map<string, Footprint>();
/** Jobs waiting for others to end: each wakes when none of the jobs it waits for is left. */
const waiters: { ids: Set<string>; queue: number; wake: () => void }[] = [];
let queueSeq = 0;
/** What each waiting paste is, to turn away an identical one. */
const queuedSignatures = new Map<number, string>();
/** A job that has been asked for but has no id yet (waiting for a question, an access check or its turn). */
let pendingSeq = 0;
function hold(fp: Footprint): string {
  const key = `p${++pendingSeq}`;
  inFlight.set(key, { ...fp, seq: pendingSeq });
  return key;
}
const seqOf = (key: string) => inFlight.get(key)?.seq;
function release(key: string) {
  inFlight.delete(key);
  wake();
}
const ended = (key: string): boolean => {
  if (key[0] === "p") return false; // lives until released
  const st = useStore.getState();
  const p = key[0] === "d" ? st.deletes[Number(key.slice(1))] : st.transfers[Number(key.slice(1))];
  return !!p && (p.phase === "done" || p.phase === "cancelled" || p.phase === "failed");
};
function wake() {
  for (const k of [...inFlight.keys()]) if (ended(k)) inFlight.delete(k);
  const open = new Set(Object.keys(useStore.getState().queuedJobs).map(Number));
  for (let i = waiters.length - 1; i >= 0; i--) {
    const w = waiters[i];
    for (const id of [...w.ids]) if (!inFlight.has(id)) w.ids.delete(id);
    if (w.ids.size === 0 || !open.has(w.queue)) {
      waiters.splice(i, 1);
      w.wake();
    }
  }
}
const fold = (x: string) => (/^[A-Za-z]:|^\\\\/.test(x) ? x.toLowerCase() : x).replace(/[\\/]+$/, "");
/** Running jobs that a copy or move of `sources` into `dest` would run into, in the order they began. */
function jobsInTheWay(sources: string[], dest: string): string[] {
  const d = fold(dest);
  const names = new Set(sources.map((s) => baseName(s).toLowerCase()));
  const out: string[] = [];
  for (const [key, fp] of inFlight) {
    const removed = fp.removes.some((raw) => {
      const p = fold(raw);
      if (isInside(d, p)) return true; // the destination is being removed, or is inside what is
      const up = parentPath(p);
      if (up !== null && fold(up) === d && names.has(baseName(p).toLowerCase())) return true; // would land where it was
      return sources.some((s) => isInside(fold(s), p) || isInside(p, fold(s))); // copying from what is being removed
    });
    // Another copy putting the same names in the same folder, or making the folder this goes into.
    const written =
      !!fp.writes &&
      ((fold(fp.writes.dir) === d && fp.writes.names.some((n) => names.has(n.toLowerCase()))) ||
        fp.writes.names.some((n) => isInside(d, fold(`${fp.writes!.dir}${fp.writes!.dir.includes("\\") ? "\\" : "/"}${n}`))));
    if (removed || written) out.push(key);
  }
  return out;
}
/** Jobs let go with "Force stop": whatever they report later is ignored. */
const forcedJobs = new Set<number>();

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
  searches: [],
  openWith: null,
  lockDialog: null,
  incomingTab: null,
  openLockDialog: (path) => set({ lockDialog: path }),
  drives: [],
  devices: [],
  link: "unknown",
  drivesFresh: false,
  transfers: {},
  specs: {},
  cancelling: {},
  clipboard: null,
  dirVersion: {},
  loadingDir: null,
  queuedJobs: {},
  unqueue: (id) => {
    const q = { ...get().queuedJobs };
    delete q[id];
    set({ queuedJobs: q });
    wake();
  },
  selection: [],
  deletes: {},
  series: {},
  lastMethods: {},
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
  columnSet: loadColumnSet(),
  folderViews: loadViews(),
  chooseDetailsOpen: false,
  folderView: null,
  frequent: [],
  places: [],
  properties: null,
  batchRename: null,
  servers: [],
  connect: null,
  picker: null,
  openConnect: (request = {}) => set({ connect: request }),
  closeConnect: () => set({ connect: null }),
  setServers: (servers) => set({ servers }),
  shareFor: null,
  undo: [],
  redo: [],
  facets: { kinds: [], extensions: [] },
  pendingSelect: null,
  pendingSelectMore: [],
  showLaunches: (launches) => {
    for (const l of launches) {
      const s = get();
      if (isMobileOs(s.info?.os)) {
        // A phone has one folder on screen; a file handed over is shown in the app's own viewer.
        if (l.folder) s.navigate(l.folder);
        set({ pendingSelect: l.select[0] ?? null, pendingSelectMore: l.select.slice(1), viewing: l.select[0] ?? null });
        continue;
      }
      const open = s.tabs.find((t) => t.location === l.folder);
      if (open) s.activate(open.id);
      else {
        s.newTab(l.folder);
        // At the tab limit the folder takes over the current tab instead.
        const now = get();
        if (now.tabs.find((t) => t.id === now.activeTab)?.location !== l.folder) now.navigate(l.folder);
      }
      set(l.select.length ? { pendingSelect: l.select[0], pendingSelectMore: l.select.slice(1) } : { pendingSelect: null, pendingSelectMore: [] });
    }
  },
  layoutEdit: false,
  popout: null,
  selecting: false,
  viewing: null,
  command: null,
  runCommand: (name) => set({ command: { name, tick: (get().command?.tick ?? 0) + 1 } }),
  settingsOpen: false,
  tasksOpen: false,
  tasksPrefill: null,
  settingsQuery: "",
  openTasks: (prefill) => set({ tasksOpen: true, tasksPrefill: prefill ?? null }),
  dialog: null,
  toast: null,
  notices: [],
  noticesOpen: false,
  unreadNotices: 0,

  init: async () => {
    const [settings, session, info] = await Promise.all([api.getSettings(), api.getSession(), api.appInfo()]);
    const isView = typeof (window as unknown as { __VIEW__?: string }).__VIEW__ === "string";
    const isExtra = typeof (window as unknown as { __OPEN__?: string }).__OPEN__ === "string";
    // Folders the app was started with (a folder on the command line, or another app's "Show in folder").
    const launches = isView || isExtra ? [] : await api.takeLaunch().catch(() => [] as Launch[]);
    const start = settings.general.startPage === "thisPc" ? THIS_PC : HOME;
    // A window made by dragging a tab out starts with just that tab and does
    // not touch the saved session of the main window.
    const opened = (window as unknown as { __OPEN__?: string }).__OPEN__;
    detachedView = (window as unknown as { __VIEW__?: string }).__VIEW__ === "transfers";
    secondary = typeof opened === "string" || detachedView;
    let tabs = secondary ? [makeTab(opened!)] : settings.general.restoreSession ? session.tabs : [];
    // Started for particular folders without tabs to restore: show just those.
    if (!tabs.length) tabs = [makeTab(launches[0]?.folder ?? start)];
    const activeTab = tabs.some((t) => t.id === session.activeTab) ? session.activeTab! : tabs[0].id;
    // The filter from last time, when filters are kept.
    let savedFilter: FilterState | null = null;
    try {
      const raw = adv<boolean>(settings, "files.keepFilter") ? localStorage.getItem("fast.filter") : null;
      const parsed = raw ? (JSON.parse(raw) as Partial<FilterState>) : null;
      if (parsed && typeof parsed === "object") savedFilter = { ...NO_FILTER, ...parsed, subfolders: false };
    } catch {
      savedFilter = null;
    }
    set({
      ...(savedFilter ? { filter: savedFilter } : {}),
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
      searches: session.searches ?? [],
      sort: adv<boolean>(settings, "files.rememberSort")
        ? { key: adv<string>(settings, "files.sortKey") as SortKey, asc: adv<boolean>(settings, "files.sortAsc") }
        : get().sort,
      // The list from the previous run paints immediately; live data follows.
      drives: session.drives,
    });
    get().showLaunches(launches);
    get().refreshDrives();
    get().refreshFrequent();
    void api.serversList().then((servers) => set({ servers })).catch(() => {});
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
      filter:
        id !== activeTab || location.startsWith("fast://search")
          ? get().filter
          : adv<boolean>(get().settings, "files.keepFilter")
            ? // "Include subfolders" is a search of the old folder; the rest of the filter carries over.
              get().filter.subfolders
              ? { ...get().filter, subfolders: false }
              : get().filter
            : NO_FILTER,
      folderView: id === activeTab ? null : get().folderView,
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

  closedTabs: [],

  reopenTab: () => {
    const { closedTabs, tabs } = get();
    const last = closedTabs[closedTabs.length - 1];
    if (!last) return get().showToast("There is no closed tab to bring back.");
    const max = adv<number>(get().settings, "tabs.max");
    if (max > 0 && tabs.length >= max) return get().showToast(`The tab limit is ${max}. Change it in Settings > Advanced.`);
    // A tab that was the only one, replaced by a blank start tab, is not worth keeping that blank tab for.
    const only = tabs.length === 1 && tabs[0].history.length === 1 && isAppLocation(tabs[0].location) && last.tab.id !== tabs[0].id && last.index === 0;
    const groupId = last.tab.groupId && get().groups.some((g) => g.id === last.tab.groupId) ? last.tab.groupId : null;
    const tab = { ...last.tab, groupId };
    const base = only ? [] : tabs;
    const at = Math.max(0, Math.min(base.length, last.index));
    set({
      closedTabs: closedTabs.slice(0, -1),
      tabs: [...base.slice(0, at), tab, ...base.slice(at)],
      activeTab: tab.id,
      selection: [],
    });
    persist();
  },

  closeTab: (id) => {
    const { tabs, activeTab, settings } = get();
    const i = tabs.findIndex((t) => t.id === id);
    if (i < 0) return;
    rememberClosed(tabs[i], i);
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

  /** Move a whole tab group, as one block, to `index` among the tabs outside it. */
  moveGroup: (groupId, index) => {
    set({ tabs: moveGroupBlock(get().tabs, groupId, index) });
    persist();
  },

  detachTab: async (id, x, y) => {
    const { tabs } = get();
    const tab = tabs.find((t) => t.id === id);
    if (!tab) return;
    try {
      // Dropped on another window's tab bar: it moves there, like in a browser.
      // A window left with no tabs closes itself.
      if (x !== undefined && y !== undefined && (await api.dropTab(tab.location, x, y, tabs.length < 2))) {
        if (tabs.length > 1) {
          // It moved to another window; it was not closed.
          skipRemembering = true;
          get().closeTab(id);
          skipRemembering = false;
        }
        return;
      }
      if (tabs.length < 2) return;
      await api.openWindow(tab.location, x, y);
      skipRemembering = true;
      get().closeTab(id);
      skipRemembering = false;
    } catch (e) {
      get().showToast(String(e));
    }
  },

  closeOthers: (id) => {
    get().tabs.forEach((t, i) => t.id !== id && rememberClosed(t, i));
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
    syncDetailsPane(current, next);
    // While a profile of your own is active, what you change is saved into it.
    const active = next.profiles?.find((p) => p.id === next.activeProfile && !p.builtIn);
    if (active && JSON.stringify(current.profiles) === JSON.stringify(next.profiles) && current.activeProfile === next.activeProfile) {
      active.snapshot = snapshotOf(next);
    }
    set({ settings: next });
    // Changes show at once either way. They are written to disk right away unless the person
    // turned "Save changes automatically" off; the Save button then writes them.
    const auto = adv<boolean>(next, "ui.autoSave");
    if (auto || adv<boolean>(current, "ui.autoSave") !== auto) {
      set({ unsaved: false, savedSettings: null });
      void api.setSettings(next).catch((e) => get().showToast(String(e)));
    } else {
      set({ unsaved: true, savedSettings: get().savedSettings ?? current });
    }
  },
  searchMode: null,
  setSearchMode: (m) => set({ searchMode: m }),
  unsaved: false,
  savedSettings: null,
  saveSettings: () => {
    const cur = get().settings;
    if (!cur) return;
    void api
      .setSettings(cur)
      .then(() => {
        set({ unsaved: false, savedSettings: null });
        get().showToast("Settings saved.");
      })
      .catch((e) => get().showToast(String(e)));
  },
  discardSettings: () => {
    const back = get().savedSettings;
    if (back) set({ settings: back });
    set({ unsaved: false, savedSettings: null });
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

  pinAt: (path, index, isFile = false) => {
    const pins = [...get().pins];
    const old = pins.findIndex((p) => p.path === path);
    const pin = old >= 0 ? pins.splice(old, 1)[0] : { path, label: baseName(path), builtin: false, isFile };
    pins.splice(Math.max(0, Math.min(old >= 0 && old < index ? index - 1 : index, pins.length)), 0, pin);
    set({ pins });
    persist();
  },

  movePin: (from, to) => {
    const pins = [...get().pins];
    if (from < 0 || from >= pins.length) return;
    const [pin] = pins.splice(from, 1);
    pins.splice(Math.max(0, Math.min(to > from ? to - 1 : to, pins.length)), 0, pin);
    set({ pins });
    persist();
  },

  addRecent: (path, isDir) => {
    const recent = [{ path, isDir, at: Date.now() }, ...get().recent.filter((r) => r.path !== path)].slice(0, adv<number>(get().settings, "files.recentLimit"));
    set({ recent });
    persist();
  },

  chooseApp: (path, reason = "") => set({ openWith: { path, reason } }),
  closeChooser: () => set({ openWith: null }),

  clearRecent: () => {
    set({ recent: [] });
    persist();
  },

  addSearch: (text) => {
    const t = text.trim();
    const limit = adv<number>(get().settings, "search.historyLimit");
    if (!t || limit <= 0) return;
    set({ searches: [t, ...get().searches.filter((x) => x !== t)].slice(0, limit) });
    persist();
  },

  removeSearch: (text) => {
    set({ searches: get().searches.filter((x) => x !== text) });
    persist();
  },

  clearSearches: () => {
    set({ searches: [] });
    persist();
  },

  onDrivesChanged: (p) => {
    set({ drives: p.drives, devices: p.devices, drivesFresh: true });
    if (!get().settings || !adv<boolean>(get().settings, "drives.notify")) return;
    for (const name of p.added) get().notify("info", `${name} connected`);
    for (const name of p.removed) get().notify("info", `${name} removed`);
  },

  refreshDrives: () => {
    void api
      .getDrives()
      .then((drives) => set({ drives, drivesFresh: true }))
      .catch(() => {});
    void api
      .getDevices()
      .then((devices) => set({ devices }))
      .catch(() => {});
  },

  refreshLink: () => {
    void api
      .networkLink()
      .then((link) => {
        if (link !== get().link) set({ link });
      })
      .catch(() => {});
  },

  setClipboard: (clipboard) => {
    set({ clipboard });
    stagedClip = [];
    // Also on the system clipboard, so other programs can paste the same files. Items from
    // inside an archive are unpacked first, so what other programs get are real files.
    if (clipboard?.paths.length) {
      const paths = clipboard.paths;
      if (paths.every(isArchiveLoc)) {
        void api
          .archiveStage(paths)
          .then((staged) => {
            if (get().clipboard !== clipboard) return; // something newer was copied meanwhile
            stagedClip = staged;
            return api.clipboardSetFiles(staged);
          })
          .catch(() => {});
      } else void api.clipboardSetFiles(paths).catch(() => {});
    }
    if (clipboard && clipboard.paths.length) get().notify(clipboard.op === "move" ? "cut" : "copy", `${clipboard.op === "move" ? "Cut" : "Copied"} ${describeItems(clipboard.paths)}`, "Ready to paste", itemFacts(clipboard.op === "move" ? "Cut (moves when pasted)" : "Copy", clipboard.paths, [["Status", "Ready to paste"]]));
  },
  pasteInto: async (dest) => {
    const ours = get().clipboard;
    const clip = await api.clipboardGet().catch(() => ({ paths: [] as string[], cut: false, virtualFiles: false }));
    let system = clip.paths;
    const same = ours && system.length === ours.paths.length && system.every((p) => ours.paths.includes(p) || stagedClip.includes(p));
    if (ours && (same || (system.length === 0 && !clip.virtualFiles))) {
      // Copied or cut here: cut means move.
      await get().startTransfer(ours.paths, dest, ours.op);
      if (ours.op === "move") get().setClipboard(null);
      return;
    }
    if (!system.length && clip.virtualFiles) {
      // Copied out of a zip, a phone or the cloud in another program: the files only exist inside it, so they are saved first.
      get().showToast("Getting the files from the other program…");
      try {
        system = await api.clipboardStageVirtual();
      } catch (e) {
        return get().showToast(`Could not paste: ${String(e).replace(/^Error: /, "")}`);
      }
    }
    if (!system.length) return get().showToast("There are no files to paste. Copy some files first.");
    // Copied in another program (Explorer, Finder, Files, a mail client). If that program cut them, they are moved, as in any file manager.
    const move = clip.cut && !clip.virtualFiles;
    await get().startTransfer(system, dest, move ? "move" : "copy");
    if (move) void api.clipboardClear().catch(() => {});
  },
  setSelection: (selection) => set({ selection }),

  bumpDirs: (dirs) => {
    const dirVersion = { ...get().dirVersion };
    for (const d of dirs) dirVersion[d] = (dirVersion[d] ?? 0) + 1;
    set({ dirVersion });
  },

  onProgress: (p) => {
    if (forcedJobs.has(p.id)) return;
    const prev = get().shown[p.id] ?? 0;
    const fraction = steadyFraction(prev, p);
    const list = get().series[p.id] ?? [];
    let series = get().series;
    if (p.phase === "running" && p.elapsedSecs > (list.at(-1)?.t ?? -1) + 0.2) {
      const next = [...list, { t: p.elapsedSecs, filesPerSec: p.filesPerSec, bytesPerSec: p.bytesPerSec }];
      // Keep the graph light on very long jobs: thin out every other sample.
      series = { ...series, [p.id]: next.length > 900 ? next.filter((_, i) => i % 2 === 0 || i === next.length - 1) : next };
    }
    // A job that was over before a second sample could be taken (an instant move, one small file)
    // still gets a graph: from nothing to its average speed.
    if (p.phase === "done" && (series[p.id]?.length ?? 0) < 2) {
      const t = Math.max(p.elapsedSecs, 0.05);
      series = { ...series, [p.id]: [{ t: 0, filesPerSec: 0, bytesPerSec: 0 }, { t, filesPerSec: p.avgFilesPerSec, bytesPerSec: p.avgBytesPerSec }] };
    }
    set({ transfers: { ...get().transfers, [p.id]: p }, shown: { ...get().shown, [p.id]: fraction }, series });
    if (p.method && p.phase === "running") noteMethod(get, set, "Copy and move", METHOD_NAME[p.method], `to ${p.destDir}`);
    if (p.phase === "done" || p.phase === "cancelled" || p.phase === "failed") {
      inFlight.delete(`t${p.id}`);
      wake();
    }
    // Let files appear in the destination folder while they are being copied in.
    if (p.phase === "running" && !detachedView && Date.now() - (lastDestRefresh[p.id] ?? 0) > 1200) {
      lastDestRefresh[p.id] = Date.now();
      get().bumpDirs([p.destDir]);
    }
    if (p.conflict && !detachedView) void askAboutConflict(p.id, p.conflict, get().dialog == null);
    if (p.phase === "done" && p.errorCount === 0) {
      // Successful jobs leave on their own; failures stay until dismissed.
      let secs = adv<number>(get().settings, "transfers.dismissSecs");
      // A job that was over in a moment has already said so in a notice; do not leave its card lingering.
      if (secs > 0 && p.elapsedSecs < 3) secs = Math.min(secs, 1.5);
      if (secs > 0) setTimeout(() => get().dismissTransfer(p.id), secs * 1000);
      const spec = get().specs[p.id];
      // Reload what the job touched, so the sort and filter apply to the finished result.
      if (adv<boolean>(get().settings, "files.sortAfterJobs")) {
        const touched = [p.destDir, ...(spec?.op === "move" ? spec.sources.map((x) => parentPath(x) ?? "") : [])].filter(Boolean);
        get().bumpDirs(touched);
      }
      // The window that started the job reports it; the transfers window only displays.
      if (detachedView) return;
      if (spec) {
        const t = transferNotice(spec.op === "move" ? "move" : "copy", spec.sources, spec.destDir);
        get().notify(spec.op === "move" ? "move" : "copy", t.title, t.detail, itemFacts(spec.op === "move" ? "Move" : "Copy", spec.sources, [
          ["To", spec.destDir],
          ["Files", String(p.filesDone)],
          ["Elapsed", formatDuration(p.elapsedSecs)],
          ...(p.skipped ? ([["Skipped", String(p.skipped)]] as [string, string][]) : []),
        ]));
      } else get().notify("info", `Finished ${p.filesDone} item${p.filesDone === 1 ? "" : "s"}`);
    }
    if (p.phase === "running" && !prev && adv<boolean>(get().settings, "transfers.openWindow")) set({ transfersOpen: true });
  },

  onDeleteProgress: (p) => {
    if (forcedJobs.has(p.id)) return;
    const ratio = p.bytesTotal > 0 ? p.bytesDone / p.bytesTotal : p.itemsTotal > 0 ? p.itemsDone / p.itemsTotal : 0;
    const shown = steadyFraction(get().shown[p.id] ?? 0, { phase: p.phase === "done" ? "done" : "running", fraction: p.phase === "scanning" ? 0 : (p.stats?.fraction ?? ratio) });
    // The same speed graph a transfer has: items (shown as files) and bytes per second over time.
    let series = get().series;
    const list = series[p.id] ?? [];
    if (p.phase === "running" && p.stats && p.elapsedSecs > (list.at(-1)?.t ?? -1) + 0.2) {
      const next = [...list, { t: p.elapsedSecs, filesPerSec: p.stats.itemsPerSec, bytesPerSec: p.stats.bytesPerSec }];
      series = { ...series, [p.id]: next.length > 900 ? next.filter((_, i) => i % 2 === 0 || i === next.length - 1) : next };
    }
    if (p.phase === "done" && p.stats && (series[p.id]?.length ?? 0) < 2) {
      series = { ...series, [p.id]: [{ t: 0, filesPerSec: 0, bytesPerSec: 0 }, { t: Math.max(p.elapsedSecs, 0.05), filesPerSec: p.stats.avgItemsPerSec, bytesPerSec: p.stats.avgBytesPerSec }] };
    }
    set({ deletes: { ...get().deletes, [p.id]: p }, shown: { ...get().shown, [p.id]: shown }, series });
    if (p.method && p.phase === "running") noteMethod(get, set, jobGroup(p.action, p.permanent), p.method, p.firstName);
    // Copying out of an archive asks the same "this file already exists" question a transfer does.
    if (p.conflict && !detachedView && p.phase === "running") void askAboutConflict(p.id, p.conflict, get().dialog == null);
    if (p.phase === "done" && p.errorCount === 0) setTimeout(() => get().dismissDelete(p.id), 3000);
    if (p.action === "delete" && p.phase === "failed") {
      deletedFrom.delete(p.id);
      inFlight.delete(`d${p.id}`);
      wake();
    }
    if (p.action === "update") {
      if (p.phase === "done") setTimeout(() => get().dismissDelete(p.id), 4000);
      return;
    }
    if (p.action !== "delete") {
      if (p.phase === "done" || p.phase === "cancelled" || p.phase === "failed") {
        if (p.action === "compress" || p.action === "extract") archiveFinished(p);
        else remoteFinished(p);
      }
      return;
    }
    if (p.phase === "done" || p.phase === "cancelled") {
      const what = p.selected === 1 && p.firstName ? `"${p.firstName}"` : `${p.selected} items`;
      const where = p.permanent ? "permanently" : `to the ${get().info?.os === "windows" ? "Recycle Bin" : "Trash"}`;
      const trashed = trashedBy.get(p.id);
      trashedBy.delete(p.id);
      const from = deletedFrom.get(p.id);
      deletedFrom.delete(p.id);
      inFlight.delete(`d${p.id}`);
      wake();
      if (from && adv<boolean>(get().settings, "files.sortAfterJobs")) get().bumpDirs([...new Set(from.map((x) => parentPath(x) ?? "").filter(Boolean))]);
      if (p.phase === "done" && p.errorCount === 0 && trashed) get().pushUndo({ kind: "trash", paths: trashed });
      if (p.phase === "done") get().notify("delete", `Deleted ${what}`, p.errorCount ? `${p.errorCount} could not be deleted` : p.permanent ? "Permanently" : `Moved ${where}`, {
        facts: [
          ["Action", "Delete"],
          ["Items", `${p.itemsDone} of ${p.itemsTotal}`],
          ["Method", p.permanent ? "Permanent delete" : `Moved ${where}`],
          ["Elapsed", formatDuration(p.elapsedSecs)],
          ...(p.errorCount ? ([["Could not delete", String(p.errorCount)]] as [string, string][]) : []),
        ],
        names: trashed ? trashed.map((t) => baseName(t)) : p.firstName ? [p.firstName] : undefined,
      });
      else get().notify("info", `Delete stopped`, `${p.itemsDone} of ${p.itemsTotal} done`);
    }
  },

  requestCancel: (kind, id) => {
    set({ cancelling: { ...get().cancelling, [id]: Date.now() } });
    void (kind === "delete" ? api.deleteControl(id, "cancel") : api.transferControl(id, "cancel")).catch((e) => get().showToast(String(e)));
  },

  forceStop: (kind, id) => {
    forcedJobs.add(id);
    const spec = get().specs[id];
    void (kind === "delete" ? api.deleteControl(id, "cancel") : api.transferForceStop(id)).catch(() => {});
    if (kind === "delete") get().dismissDelete(id);
    else get().dismissTransfer(id);
    const cancelling = { ...get().cancelling };
    delete cancelling[id];
    set({ cancelling });
    if (spec) get().bumpDirs([spec.destDir, ...spec.sources.map((p) => parentOf(p))].filter((d): d is string => !!d));
    get().notify("info", "Stopped", "Anything already copied stays where it is. The drive may take a while to let go.");
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

  setColumns: (next, scope) => {
    const loc = activeLocation();
    const set_ = normalizeSet(next);
    if (scope === "all" || !loc || isAppLocation(loc) || !adv<boolean>(get().settings, "files.perFolderView")) {
      const folderViews = scope === "all" ? withoutColumns(get().folderViews) : get().folderViews;
      set({ columnSet: set_, folderViews });
      saveColumnSet(set_);
      if (scope === "all") saveViews(folderViews);
      return;
    }
    const folderViews = withView(get().folderViews, loc, { columns: set_ });
    set({ folderViews });
    saveViews(folderViews);
  },

  setSort: (sort) => {
    set({ sort });
    // With views kept per folder, the order belongs to this folder alone.
    const loc = activeLocation();
    if (loc && !isAppLocation(loc) && adv<boolean>(get().settings, "files.perFolderView")) {
      const folderViews = withView(get().folderViews, loc, { sort: isDefaultSort(sort, get().settings) ? null : sort });
      set({ folderViews });
      saveViews(folderViews);
      return;
    }
    // The order is kept for every folder and for the next start.
    if (adv<boolean>(get().settings, "files.rememberSort")) {
      const a = get().settings?.advanced;
      if (a?.["files.sortKey"] !== sort.key || a?.["files.sortAsc"] !== sort.asc) {
        get().updateSettings((st) => {
          st.advanced = { ...(st.advanced ?? {}), "files.sortKey": sort.key, "files.sortAsc": sort.asc };
        });
      }
    }
  },

  setFilter: (patch) => {
    const filter = patch ? { ...get().filter, ...patch } : NO_FILTER;
    set({ filter });
    const loc = activeLocation();
    if (loc && !isAppLocation(loc) && adv<boolean>(get().settings, "files.perFolderView")) {
      const folderViews = withView(get().folderViews, loc, { filter: isFiltering(filter) ? { ...filter, subfolders: false } : null });
      set({ folderViews });
      saveViews(folderViews);
      return;
    }
    // Kept for next time, unless "Use the same filter in every folder" is off.
    try {
      if (adv<boolean>(get().settings, "files.keepFilter")) localStorage.setItem("fast.filter", JSON.stringify({ ...filter, subfolders: false }));
      else localStorage.removeItem("fast.filter");
    } catch {
      /* storage can be unavailable; the filter then lasts until the app closes */
    }
  },

  setViewLevel: (level) => {
    const next = Math.max(0, Math.min(VIEW_LEVELS.length - 1, level));
    if (!adv<boolean>(get().settings, "files.rememberView")) {
      // Only for this folder: it is forgotten when another folder is opened.
      if (next !== get().folderView) set({ folderView: next });
      return;
    }
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
    set({ undo: limit > 0 ? [...get().undo.slice(-(limit - 1)), a] : [], redo: [] });
  },

  startTransfer: async (sources, destDir, op) => {
    const s = get();
    if (!sources.length || !s.settings) return;
    // "Ask" is answered file by file while the transfer runs (see askAboutConflict), so
    // that files inside merged folders get asked about too, as in Windows.
    const conflict: Conflict = s.settings.transfer.conflict;
    const spec: TransferSpec = { sources, destDir, op, conflict, skipIdentical: adv<boolean>(s.settings, "transfers.skipIdentical") };
    // A job begun earlier that is still working on these folders goes first, so things happen
    // in the order they were asked for: this waits in line, shown with the other jobs, and
    // starts by itself when that job is over.
    // Pasting again while the same paste is already waiting would only make a second copy.
    const signature = `${op}|${destDir}|${sources.join("\n")}`;
    if ([...queuedSignatures.values()].includes(signature)) {
      s.showToast("That paste is already waiting for the earlier job to finish.");
      return;
    }
    // This job is on record from this moment, so pastes asked for after it wait for it, even
    // while it is still waiting itself or being checked.
    const key = hold({ removes: op === "move" ? sources : [], writes: { dir: destDir, names: sources.map(baseName) } });
    const mySeq = seqOf(key) ?? 0;
    let qid = 0;
    try {
      for (;;) {
        const blockers = jobsInTheWay(sources, destDir).filter((k) => k !== key && (seqOf(k) ?? 0) < mySeq);
        if (!blockers.length) break;
        if (!qid) {
          qid = ++queueSeq;
          queuedSignatures.set(qid, signature);
          const what = blockers.some((k) => k[0] === "d") ? "a delete" : "an earlier job";
          set({
            queuedJobs: { ...get().queuedJobs, [qid]: { title: `${op === "move" ? "Move" : "Copy"} ${describeItems(sources)} to ${baseName(destDir) || destDir}`, detail: `Waiting for ${what} on these folders to finish, then it starts by itself` } },
            transfersOpen: true,
          });
          s.showToast(`Waiting for ${what} on these folders to finish. The paste starts by itself.`);
        }
        await new Promise<void>((resolve) => {
          waiters.push({ ids: new Set(blockers), queue: qid, wake: resolve });
          wake();
        });
        if (!get().queuedJobs[qid]) return; // taken out of the line
      }
      if (qid) {
        queuedSignatures.delete(qid);
        get().unqueue(qid);
        qid = 0;
      }
      if (isArchiveLoc(destDir)) {
        // Into an archive: from disk, from inside it (a move rearranges it) or from another archive.
        if (isRemote(destDir) || sources.some(isRemote)) {
          s.showToast("Copy items from a server to a folder on this computer first.");
          return;
        }
        try {
          await api.archivePasteStart(sources, destDir, op === "move" ? "move" : "copy");
        } catch (e) {
          s.showToast(friendlyRemoteError(e));
        }
        return;
      }
      if (sources.some(isArchiveLoc)) {
        // Out of an archive: unpacked into the folder; a file already there is handled by the
        // "When a file already exists" setting, like any other copy.
        // A move also takes the items out of the archive once they have arrived.
        if (isRemote(destDir) || !sources.every(isArchiveLoc)) {
          s.showToast("Copy items out of an archive into a folder on this computer first.");
          return;
        }
        try {
          await api.archiveCopyStart(sources, destDir, false, op === "move");
        } catch (e) {
          s.showToast(friendlyRemoteError(e));
        }
        return;
      }
      if (isRemote(destDir) || isRemote(sources[0])) {
        // To or from a server: its own job (no administrator route, no undo).
        try {
          remoteJobs.set(await api.remoteCopyStart(sources, destDir, op === "move"), { op });
        } catch (e) {
          s.showToast(String(e));
        }
        return;
      }
      try {
        const job: Job = { kind: "transfer", spec };
        const mode = await guardAccess(job, `${op === "move" ? "Moving" : "Copying"} to ${baseName(destDir)}`);
        if (mode === "cancel") return;
        const id = mode === "elevated" ? await api.elevatedStart(job) : await api.transferStart(spec);
        set({ specs: { ...get().specs, [id]: spec } });
        inFlight.set(`t${id}`, { removes: op === "move" ? sources : [], writes: { dir: destDir, names: sources.map(baseName) }, seq: mySeq });
        wake();
        const landed = sources.map((src) => ({ from: src, to: joinPath(destDir, baseName(src)) }));
        get().pushUndo(op === "move" ? { kind: "move", pairs: landed } : { kind: "copy", created: landed.map((l) => l.to) });
      } catch (e) {
        s.showToast(String(e));
      }
    } finally {
      if (qid) {
        queuedSignatures.delete(qid);
        get().unqueue(qid);
      }
      release(key);
    }
  },

  notify: (kind, title, detail, more) => {
    const settings = get().settings;
    if (!settings || !adv<boolean>(settings, "notify.enabled")) return;
    const id = nextNoticeId++;
    const bell = adv<string>(settings, "notify.mode") === "bell";
    const keep = Math.max(5, adv<number>(settings, "notify.keep"));
    const notice: Notice = { id, kind, title, detail, at: Date.now(), popup: !bell, ...more };
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
      searches: s.searches,
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
    case "clear":
      return "Smart Clear";
    default:
      return baseName(l.path);
  }
}

export function thisPcLabel(os: string | undefined): string {
  return os === "windows" ? "This PC" : os === "macos" ? "This Mac" : "This Computer";
}

const askedConflicts = new Set<string>();
/** When each running job last made its destination folder re-read. */
const lastDestRefresh: Record<number, number> = {};

/** "This file already exists": Replace, Skip, Keep both, each for this file or for all that follow. */
async function askAboutConflict(id: number, c: ConflictInfo, canShow: boolean) {
  const key = `${id}:${c.number}`;
  if (askedConflicts.has(key) || !canShow) return; // asked already, or another dialog is open: the next progress update retries
  askedConflicts.add(key);
  const line = (label: string, size: number, at: number | null) => `${label}: ${formatSize(size)}${at ? `, changed ${formatDate(at * 1000)}` : ""}`;
  const newer = c.srcModified != null && c.dstModified != null ? (c.srcModified > c.dstModified ? " (newer)" : c.srcModified < c.dstModified ? " (older)" : " (same time)") : "";
  const answer = await ask({
    title: `"${c.name}" already exists here`,
    message: `${line("Already here", c.dstSize, c.dstModified)}\n${line("Coming in", c.srcSize, c.srcModified)}${newer}\n\n${c.dst}`,
    buttons: [
      { id: "replace", label: "Replace", primary: true },
      { id: "replaceAll", label: "Replace all" },
      { id: "skip", label: "Skip" },
      { id: "skipAll", label: "Skip all" },
      { id: "keepBoth", label: "Keep both" },
      { id: "cancel", label: "Cancel transfer" },
    ],
  });
  const b = answer?.button ?? "cancel";
  try {
    if (b === "cancel") {
      // A transfer, or a job on the same card (copying out of an archive).
      if (useStore.getState().deletes[id]) await api.deleteControl(id, "cancel");
      else await api.transferControl(id, "cancel");
    }
    else await api.transferResolve(id, b.replace("All", "") as "skip" | "replace" | "keepBoth", b.endsWith("All"));
  } catch {
    /* the transfer ended while the question was open */
  }
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
/** Things the bars around the folder view ask it to do. The last six are used by the phone layout. */
export type FolderCommand = "selectAll" | "selectNone" | "invert" | "rename" | "select" | "newFolder" | "newFile" | "paste" | "refresh" | "menu";

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
/**
 * Open a file with the system's default program. When there is none, ask which
 * program to use, the way the person chose in Settings: this app's own chooser,
 * the system's (Windows), or just a message.
 */
export async function openFile(path: string): Promise<void> {
  // Choosing a file for a form: opening it picks it instead.
  const picking = useStore.getState().picker;
  if (picking) return picking.done(path);
  const { showToast, chooseApp, info } = useStore.getState();
  if (isMobileOs(info?.os)) {
    // A phone opens files through its own "open with" sheet; the app only hands the file over.
    const said = openOutside(path);
    if (said === "") return;
    if (said) return showToast(said);
    return void useStore.setState({ viewing: path });
  }
  try {
    const used = await api.openItem(path);
    if (used) showToast(`No app was set for this file, so it was opened with ${used}. Right-click and choose Open with to pick another.`);
    return;
  } catch (e) {
    const text = String(e);
    if (!text.startsWith("no-handler:")) return showToast(text);
    const reason = text.slice("no-handler:".length);
    const how = adv<string>(useStore.getState().settings, "files.noAppAction");
    if (how === "message") return showToast(`${reason} Right-click the file and choose Open with.`);
    if (how === "system") {
      try {
        await api.openSystemChooser(path);
        return;
      } catch {
        // This system has no chooser of its own: use ours.
      }
    }
    chooseApp(path, reason);
  }
}

export async function deletePaths(paths: string[], forcePermanent = false): Promise<boolean> {
  const s = useStore.getState();
  if (!paths.length || !s.settings) return false;
  if (paths.some((p) => insideArchive(p))) {
    const what = paths.length === 1 ? `"${baseName(paths[0])}"` : `${paths.length} items`;
    const r = await ask({
      title: `Delete ${what} from the archive?`,
      message: "Items deleted from an archive do not go to the Trash. This cannot be undone.",
      buttons: [
        { id: "ok", label: "Delete", primary: true, danger: true },
        { id: "cancel", label: "Cancel" },
      ],
    });
    if (!r || r.button !== "ok") return false;
    try {
      await api.archiveDelete(paths);
      s.notify("delete", `Deleted ${what} from the archive`);
      refreshArchiveTabs();
      return true;
    } catch (e) {
      s.showToast(String(e));
      return false;
    }
  }
  if (isRemote(paths[0])) return deleteOnServer(paths);
  // A phone has no recycle bin the app can use, so deleting there is always for good (and always asks).
  const permanent = forcePermanent || !s.settings.general.deleteToTrash || isMobileOs(s.info?.os);
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
  // From now on a paste into these folders waits for this delete, even while the access check
  // below is still running and the delete has no id yet.
  const held = hold({ removes: paths });
  try {
    const job: Job = { kind: "delete", paths, permanent };
    const mode = await guardAccess(job, "Deleting these items");
    if (mode === "cancel") return false;
    if (mode === "elevated") {
      const id = await api.elevatedStart(job);
      deletedFrom.set(id, paths);
      inFlight.set(`d${id}`, { removes: paths, seq: seqOf(held) });
    } else {
      const id = await api.deleteStart(paths, permanent);
      if (!permanent) trashedBy.set(id, paths);
      deletedFrom.set(id, paths);
      inFlight.set(`d${id}`, { removes: paths, seq: seqOf(held) });
    }
    return true;
  } catch (e) {
    s.showToast(String(e));
    return false;
  } finally {
    release(held);
  }
}

export async function renamePath(path: string, newName: string): Promise<string | null> {
  const { showToast, pushUndo } = useStore.getState();
  if (insideArchive(path)) {
    try {
      const made = await api.archiveRename(path, newName);
      useStore.getState().notify("rename", `Renamed "${baseName(path)}"`, `to "${baseName(made)}"`, { facts: [["Action", "Rename"], ["From", baseName(path)], ["To", baseName(made)], ["Location", parentPath(made) ?? ""]] });
      refreshArchiveTabs();
      return made;
    } catch (e) {
      showToast(String(e));
      return null;
    }
  }
  if (isRemote(path)) {
    try {
      const made = await api.remoteRename(path, newName);
      useStore.getState().notify("rename", `Renamed "${baseName(path)}"`, `to "${baseName(made)}"`, { facts: [["Action", "Rename"], ["From", baseName(path)], ["To", baseName(made)], ["Location", parentPath(made) ?? ""]] });
      refreshRemoteTabs();
      return made;
    } catch (e) {
      showToast(friendlyRemoteError(e));
      return null;
    }
  }
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
      useStore.getState().notify("rename", `Renamed "${baseName(path)}"`, `to "${baseName(made)}"`, { facts: [["Action", "Rename"], ["From", baseName(path)], ["To", baseName(made)], ["Location", parentPath(made) ?? ""]] });
    }
    return made;
  } catch (e) {
    showToast(String(e));
    return null;
  }
}

export async function createPath(dir: string, name: string, folder: boolean): Promise<string | null> {
  const { showToast, pushUndo } = useStore.getState();
  if (insideArchive(dir)) {
    try {
      const made = await api.archiveCreate(dir, name, folder);
      useStore.getState().notify("create", `Created ${folder ? "folder" : "file"} "${baseName(made)}"`, undefined, { facts: [["Action", "Create"], ["Name", baseName(made)], ["Location", parentPath(made) ?? ""]] });
      refreshArchiveTabs();
      return made;
    } catch (e) {
      showToast(String(e));
      return null;
    }
  }
  if (isRemote(dir)) {
    if (!folder) {
      showToast("New files can't be made on a server. Make a folder, or copy a file into it.");
      return null;
    }
    try {
      const made = await api.remoteMkdir(dir, name);
      useStore.getState().notify("create", `Created folder "${baseName(made)}"`, undefined, { facts: [["Action", "Create"], ["Name", baseName(made)], ["Location", parentPath(made) ?? ""]] });
      refreshRemoteTabs();
      return made;
    } catch (e) {
      showToast(friendlyRemoteError(e));
      return null;
    }
  }
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
      useStore.getState().notify("create", `Created ${folder ? "folder" : "file"} "${baseName(made)}"`, undefined, { facts: [["Action", "Create"], ["Name", baseName(made)], ["Location", parentPath(made) ?? ""]] });
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

const ACTION_NAME = { rename: "rename", renameMany: "rename", create: "create", copy: "copy", move: "move", trash: "delete" } as const;

/** Do the opposite of an action (`undo`), or do it again (`redo`). Throws when it cannot. */
async function reverse(action: UndoAction, direction: "undo" | "redo"): Promise<void> {
  const undoing = direction === "undo";
  switch (action.kind) {
    case "rename":
      await (undoing ? api.renameItem(action.to, baseName(action.from)) : api.renameItem(action.from, baseName(action.to)));
      break;
    case "renameMany": {
      const to = (p: { from: string; to: string }) => (undoing ? p : { from: p.to, to: p.from });
      const pairs = action.pairs.map(to);
      await applyRenames(parentOf(pairs[0].from), pairs.map((p) => ({ from: baseName(p.from), to: baseName(p.to), changed: true, problem: null })), false);
      break;
    }
    case "create":
      await (undoing ? api.trashPaths([action.path]) : restoreFromTrash([action.path]));
      break;
    case "copy":
      await (undoing ? api.trashPaths(action.created) : restoreFromTrash(action.created));
      break;
    case "trash":
      await (undoing ? restoreFromTrash(action.paths) : api.trashPaths(action.paths));
      break;
    case "move":
      for (const p of action.pairs) {
        const [source, back] = undoing ? [p.to, p.from] : [p.from, p.to];
        await api.transferStart({ sources: [source], destDir: parentOf(back), op: "move", conflict: "keepBoth" });
      }
      break;
  }
}

/**
 * Carry out planned renames in `dir` (see `orderRenames` for why there can be
 * several rounds). Returns what was renamed, by full path. Items that fail are
 * counted in a toast; the rest still go ahead.
 */
export async function applyRenames(dir: string, plans: RenamePlan[], remember = true): Promise<{ from: string; to: string }[]> {
  const s = useStore.getState();
  const done = new Map<string, string>();
  let failed = 0;
  let firstError = "";
  const tag = String(Date.now());
  const rounds = orderRenames(plans, tag);
  const finalName = new Map(plans.map((p) => [p.from, p.to]));
  for (const round of rounds) {
    for (const step of round) {
      try {
        await api.renameItem(joinPath(dir, step.from), step.to);
        // Track by the original name: a temporary name later becomes the final one.
        const original = step.from.startsWith(`.fast-rename-${tag}-`) ? [...done.entries()].find(([, v]) => v === step.from)?.[0] : step.from;
        if (original) done.set(original, step.to);
      } catch (e) {
        failed++;
        firstError ||= String(e instanceof Error ? e.message : e);
      }
    }
  }
  const pairs = [...done.entries()].filter(([from, to]) => finalName.get(from) === to).map(([from, to]) => ({ from: joinPath(dir, from), to: joinPath(dir, to) }));
  s.bumpDirs([dir]);
  if (failed) s.showToast(`${failed} item${failed === 1 ? "" : "s"} could not be renamed. ${firstError}`);
  if (remember && pairs.length) {
    s.pushUndo({ kind: "renameMany", pairs });
    s.notify("rename", `Renamed ${describeCount(pairs.length)}`);
  }
  if (!remember && failed) throw new Error(firstError || "Some items could not be renamed.");
  return pairs;
}

/** Whether the name says it is an archive this app can open and unpack. */
export const isArchiveName = isArchiveFile;



/** Archive jobs that are running, by job id, with whether the person asked to extract "here". */
const archiveJobs = new Map<number, { here: boolean; /** Called with the folder that was unpacked, once it is done. */ then?: (folder: string) => void }>();

/** Put the items into a zip next to them. Progress and Cancel are on the transfers card. */
export async function compressPaths(paths: string[]): Promise<void> {
  if (!paths.length) return;
  try {
    archiveJobs.set(await api.archiveStart("compress", paths), { here: false });
  } catch (e) {
    useStore.getState().showToast(String(e instanceof Error ? e.message : e));
  }
}

/** Unpack an archive into a folder named after it, or straight into the folder it is in. */
export async function extractArchive(path: string, here: boolean): Promise<void> {
  try {
    archiveJobs.set(await api.archiveStart("extract", [path], here), { here });
  } catch (e) {
    useStore.getState().showToast(String(e instanceof Error ? e.message : e));
  }
}

/**
 * A terminal needs a real folder, and a folder inside an archive is not one. So the archive is unpacked
 * first (into a folder next to it, or straight into the folder it is in) and the terminal opens in the
 * unpacked copy of the folder that was being looked at.
 */
export async function extractAndOpenTerminal(location: string, mode: "here" | "admin", intoParent = false): Promise<void> {
  const arc = splitArchive(location);
  if (!arc) return;
  try {
    const id = await api.archiveStart("extract", [arc.archive], intoParent);
    useStore.getState().showToast("Unpacking first, the terminal opens when it is done.");
    archiveJobs.set(id, {
      here: intoParent,
      then: (folder) => {
        const dir = arc.inner ? arc.inner.split("/").reduce((d, part) => joinPath(d, part), folder) : folder;
        void api.openTerminal(dir, mode).catch(() => api.openTerminal(folder, mode).catch((e) => useStore.getState().showToast(String(e))));
      },
    });
  } catch (e) {
    useStore.getState().showToast(String(e instanceof Error ? e.message : e));
  }
}

/** A compress or extract job ended: say so, make it undoable and select what it made. */
function archiveFinished(p: DeleteProgress) {
  const s = useStore.getState();
  const job = archiveJobs.get(p.id);
  archiveJobs.delete(p.id);
  const name = p.selected === 1 && p.firstName ? `"${p.firstName}"` : `${p.selected} items`;
  if (p.phase === "cancelled") return s.notify("info", `${p.action === "compress" ? "Compressing" : "Extracting"} stopped`, "Nothing was kept");
  if (p.phase === "failed") return s.notify("error", `Could not ${p.action} ${name}`, p.errors[0]?.message ?? "");
  if (!p.result) return;
  const files = `${p.itemsDone} file${p.itemsDone === 1 ? "" : "s"}`;
  if (p.action === "compress") s.notify("create", `Created "${baseName(p.result)}"`, files);
  else s.notify("create", `Extracted ${name}`, files);
  if (p.action === "compress" || !job?.here) s.pushUndo({ kind: "create", path: p.result });
  s.setSelection([p.result]);
  job?.then?.(p.result);
}

/** Server jobs that are running, by job id. */
/** Where the items copied from inside an archive were unpacked for the system clipboard. */
let stagedClip: string[] = [];
const remoteJobs = new Map<number, { op: "copy" | "move" }>();

/** Tell every open server folder to read its listing again. */
/** Show the changed contents in every tab that is looking inside an archive. */
export function refreshArchiveTabs(): void {
  const s = useStore.getState();
  const open = s.tabs.map((t) => t.location).filter(isArchiveLoc);
  if (open.length) s.bumpDirs([...new Set(open)]);
}

export function refreshRemoteTabs(): void {
  const s = useStore.getState();
  const open = s.tabs.map((t) => t.location).filter(isRemote);
  if (open.length) s.bumpDirs([...new Set(open)]);
}

/** Server errors in plain words; the login prompt is handled where it is shown. */
export function friendlyRemoteError(e: unknown): string {
  const text = String(e instanceof Error ? e.message : e);
  if (text.startsWith("needs-login:")) return "Log in to this server again first.";
  if (text.startsWith("untrusted-host:") || text.startsWith("changed-host:")) return "This server's identity has not been confirmed.";
  return text;
}

async function deleteOnServer(paths: string[]): Promise<boolean> {
  const s = useStore.getState();
  const what = paths.length === 1 ? `"${baseName(paths[0])}"` : `${paths.length} items`;
  const device = /^mtp:\/\//i.test(paths[0]);
  const r = await ask({
    title: `Delete ${what} from ${device ? "the device" : "the server"}?`,
    message: device ? "Phones and cameras have no Recycle Bin. This cannot be undone." : "Servers have no Recycle Bin or Trash. This cannot be undone.",
    buttons: [
      { id: "ok", label: "Delete", primary: true, danger: true },
      { id: "cancel", label: "Cancel" },
    ],
  });
  if (!r || r.button !== "ok") return false;
  try {
    await api.remoteRemoveStart(paths);
    return true;
  } catch (e) {
    s.showToast(friendlyRemoteError(e));
    return false;
  }
}

/** An upload, download or delete on a server ended. */
function remoteFinished(p: DeleteProgress) {
  const s = useStore.getState();
  const job = remoteJobs.get(p.id);
  remoteJobs.delete(p.id);
  refreshRemoteTabs();
  refreshArchiveTabs();
  const name = p.selected === 1 && p.firstName ? `"${p.firstName}"` : `${p.selected} items`;
  const files = `${p.itemsDone} file${p.itemsDone === 1 ? "" : "s"}`;
  if (p.phase === "cancelled") return s.notify("info", "Stopped", "What was already copied was kept");
  if (p.phase === "failed") {
    const message = p.errors[0]?.message ?? "";
    if (message.startsWith("needs-login:")) {
      const url = message.slice("needs-login:".length);
      s.openConnect({ url, message: "Log in to this server again." });
      return;
    }
    if (p.action === "unpack") return s.notify("error", `Could not copy ${name} out of the archive`, message);
    if (p.action === "pack") return s.notify("error", `Could not change the archive with ${name}`, message);
    const verb = p.action === "remote-delete" ? "delete" : p.action === "open" ? "open" : p.action === "upload" ? "upload" : "download";
    return s.notify("error", `Could not ${verb} ${name}`, friendlyRemoteError(message));
  }
  if (p.action === "open") {
    if (p.result) void openFile(p.result);
    return;
  }
  if (p.action === "remote-delete") return s.notify("delete", `Deleted ${name}`, p.errorCount ? `${p.errorCount} could not be deleted` : `${p.itemsDone} item${p.itemsDone === 1 ? "" : "s"}`);
  if (p.action === "unpack") {
    const left = p.skipped ? `${p.skipped} already there, left as they were` : "";
    return s.notify("copy", `Copied out of the archive: ${name}`, p.errors[0]?.message ?? [files, left].filter(Boolean).join(", "));
  }
  if (p.action === "pack") return s.notify("copy", `Added to the archive: ${name}`, p.errors[0]?.message ?? files);
  const moved = job?.op === "move";
  const verb = p.action === "upload" ? (moved ? "Moved to the server" : "Uploaded") : moved ? "Moved from the server" : "Downloaded";
  s.notify(moved ? "move" : "copy", `${verb}: ${name}`, p.errorCount ? `${p.errorCount} could not be copied` : files);
}

const describeCount = (n: number) => (n === 1 ? "1 item" : `${n} items`);

/** Put items back from the Trash; complains when none of them are there any more. */
async function restoreFromTrash(paths: string[]): Promise<void> {
  const found = await api.restoreFromTrash(paths);
  if (found === 0) throw new Error("Those items are no longer in the Trash.");
}

async function runStep(from: "undo" | "redo"): Promise<void> {
  const s = useStore.getState();
  const stack = s[from];
  const action = stack.at(-1);
  if (!action) return;
  const other = from === "undo" ? "redo" : "undo";
  set_({ [from]: stack.slice(0, -1) });
  try {
    await reverse(action, from);
    set_({ [other]: [...useStore.getState()[other], action] });
    s.notify("undo", `${from === "undo" ? "Undid" : "Redid"} ${ACTION_NAME[action.kind]}`);
  } catch (e) {
    s.showToast(String(e instanceof Error ? e.message : e));
  }
}

const set_ = (patch: Partial<Pick<State, "undo" | "redo">>) => useStore.setState(patch);

/** Reverse the last rename, create, move, copy or delete to the Trash. */
export const undoLast = () => runStep("undo");
/** Do again what Undo took back. */
export const redoLast = () => runStep("redo");

function parentOf(path: string): string {
  return parentPath(path) ?? path;
}

// Keep the address-hiding helper in step with the settings and saved servers.
{
  let last: unknown[] = [];
  useStore.subscribe((s) => {
    const next = [s.settings?.appearance.hideNetworkAddress, s.servers, s.devices];
    if (next[0] === last[0] && next[1] === last[1] && next[2] === last[2]) return;
    last = next;
    setAddressPrivacy(!!s.settings?.appearance.hideNetworkAddress, s.servers);
    setDeviceNames(s.devices);
  });
}

// Changing "Sorted by" or "Sort ascending" in Settings (or applying a profile)
// takes effect at once in the open folder.
useStore.subscribe((s, prev) => {
  if (s.settings?.advanced === prev.settings?.advanced || !s.settings) return;
  if (adv<boolean>(s.settings, "files.perFolderView")) return;
  if (!adv<boolean>(s.settings, "files.rememberSort")) return;
  const key = adv<string>(s.settings, "files.sortKey") as SortKey;
  const asc = adv<boolean>(s.settings, "files.sortAsc");
  if (s.sort.key !== key || s.sort.asc !== asc) useStore.setState({ sort: { key, asc } });
});

// ---- Views kept for each folder ---------------------------------------------------
// Whenever the open folder changes (opening, going back, another tab, the app starting), its own
// sort order and filter are put in place; a folder with none starts as the defaults say.

export const activeLocation = (): string | null => {
  const s = useStore.getState();
  return s.tabs.find((t) => t.id === s.activeTab)?.location ?? null;
};

const defaultSortOf = (settings: Settings | null) => ({ key: adv<string>(settings, "files.sortKey") as SortKey, asc: adv<boolean>(settings, "files.sortAsc") });
const isDefaultSort = (sort: { key: SortKey; asc: boolean }, settings: Settings | null) => {
  const d = defaultSortOf(settings);
  return sort.key === d.key && sort.asc === d.asc;
};

let appliedFor: string | null = null;
let appliedPerFolder = false;
function applyFolderView() {
  const s = useStore.getState();
  const loc = activeLocation();
  if (!s.ready || !s.settings || !loc) return;
  const per = adv<boolean>(s.settings, "files.perFolderView");
  if (loc === appliedFor && per === appliedPerFolder) return;
  appliedFor = loc;
  appliedPerFolder = per;
  if (!per || isAppLocation(loc)) return;
  const v = s.folderViews[loc];
  const sort = v?.sort ?? defaultSortOf(s.settings);
  const filter = v?.filter ?? NO_FILTER;
  if (sort.key !== s.sort.key || sort.asc !== s.sort.asc || filter !== s.filter) useStore.setState({ sort, filter });
}
useStore.subscribe(applyFolderView);
