// The terminal panel's tabs and panes. A pane shows one shell kept by the terminal host; what the
// panel looks like (open or shut, which tabs, how they are split) is saved here so the next start
// brings it back, and the host (or a transcript saved when the app closed) brings back the text.

import { create } from "zustand";
import { api, type TermInfo, type TermSession } from "../api";
import { isAppLocation, isArchiveLoc, isRemote, isSshLoc, parentPath, remoteLabel, splitRemote } from "../util/path";
import { posixShellQuote, sshNanoCommand } from "../util/remoteFileKind";
import { adv } from "../settings/advanced";
import { useStore } from "../store";
import { moveTo, resolveLayout } from "../util/layout";
import { isMobileOs } from "../util/native";

export interface Pane {
  id: string;
  /** The shell's id in the host; null while it is being started or when starting failed. */
  sid: string | null;
  title: string;
  cwd: string;
  elevated: boolean;
  /** Exit code once the shell has ended. */
  exited: number | null;
  /** What it printed last time (base64), to show above the new shell's output. */
  transcript?: string;
  error?: string;
}

export interface Tab {
  id: string;
  panes: Pane[];
  active: string;
}

interface Saved {
  open: boolean;
  active: string | null;
  tabs: { id: string; active: string; panes: { id: string; sid: string | null; title: string; cwd: string; elevated: boolean }[] }[];
}

const KEY = "fast.terminal";

function loadSaved(): Saved {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? "null") as Saved | null;
    if (raw && Array.isArray(raw.tabs)) return { open: !!raw.open, active: raw.active ?? null, tabs: raw.tabs };
  } catch {
    /* nothing saved, or storage unavailable */
  }
  return { open: false, active: null, tabs: [] };
}

export const uid = () => Math.random().toString(36).slice(2, 8) + Date.now().toString(36).slice(-4);

/** Where a new terminal starts: the folder being viewed, when it is a real local folder or an SSH folder. */
export function startFolder(): string {
  const s = useStore.getState();
  const home = s.info?.home ?? "";
  if (adv<string>(s.settings, "terminal.startIn") === "home") return home;
  const loc = s.tabs.find((t) => t.id === s.activeTab)?.location ?? "";
  if (isSshLoc(loc)) return loc;
  if (!loc || isAppLocation(loc) || isRemote(loc) || isArchiveLoc(loc)) return home;
  return loc;
}

const b64Encoder = new TextEncoder();
const encodeB64 = (text: string): string => {
  const bytes = b64Encoder.encode(text);
  let out = "";
  for (let i = 0; i < bytes.length; i += 0x8000) out += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(out);
};

let sshQueue: Promise<void> = Promise.resolve();

function openOrRunSsh(dirUrl: string, command?: string): Promise<void> {
  if (!isSshLoc(dirUrl)) return Promise.resolve();
  const appState = useStore.getState();
  if (isMobileOs(appState.info?.os)) return Promise.resolve();
  const task = async () => {
    const { getState: get, setState: set } = useTerminal;
    const s = useStore.getState();
    const layout = resolveLayout(s.settings?.appearance.layout);
    if (layout.terminal.dock === "hidden") {
      s.updateSettings((x) => void (x.appearance.layout = moveTo(layout, "terminal", "bottom")));
    }
    if (!get().open) set({ open: true });
    if (!get().ready) await get().init();

    const { origin, path } = splitRemote(dirUrl);
    const originKey = origin.toLowerCase();
    for (const tab of get().tabs) {
      const pane = tab.panes.find(
        (p) => p.exited === null && p.sid && isSshLoc(p.cwd) && splitRemote(p.cwd).origin.toLowerCase() === originKey,
      );
      if (!pane || !pane.sid) continue;
      get().activatePane(tab.id, pane.id);
      if (splitRemote(pane.cwd).path !== path) {
        await api.termInput(pane.sid, encodeB64(` cd ${posixShellQuote(path)}\r`)).catch(() => {});
        get().patchPane(pane.id, { cwd: dirUrl });
        void api.termDo("cwd", pane.sid, dirUrl).catch(() => {});
      }
      if (command) {
        await api.termInput(pane.sid, encodeB64(command)).catch(() => {});
      }
      return;
    }

    const pane = await startPane(dirUrl, false, remoteLabel(dirUrl));
    const tab: Tab = { id: uid(), panes: [pane], active: pane.id };
    set((st) => ({ tabs: [...st.tabs, tab], active: tab.id, open: true }));
    if (command && pane.sid) {
      // Allow the new remote SSH shell to print its initial prompt and apply `pending_cd`.
      await new Promise((r) => setTimeout(r, 350));
      await api.termInput(pane.sid, encodeB64(command)).catch(() => {});
    }
  };
  const next = sshQueue.then(task, task);
  sshQueue = next;
  return next;
}

interface TerminalState {
  open: boolean;
  tabs: Tab[];
  active: string | null;
  ready: boolean;
  info: TermInfo | null;
  error: string | null;
  setOpen: (open: boolean) => void;
  toggle: () => void;
  init: () => Promise<void>;
  newTab: (opts?: { cwd?: string; elevated?: boolean; title?: string | null }) => Promise<void>;
  /** Open the terminal for an `ssh://` folder, reusing and routing an existing SSH shell for the same server. */
  openForSsh: (url: string) => Promise<void>;
  /** Open a remote `ssh://` text file inside `nano` in the active/reused SSH shell for that server. */
  openFileInSsh: (fileUrl: string, forceSudo?: boolean) => Promise<void>;
  split: () => Promise<void>;
  activate: (id: string) => void;
  activatePane: (tabId: string, paneId: string) => void;
  closeTab: (id: string) => void;
  closePane: (tabId: string, paneId: string) => void;
  clearActive: () => void;
  restart: (tabId: string, paneId: string) => Promise<void>;
  patchPane: (paneId: string, patch: Partial<Pane>) => void;
  /** Re-read the host's list of sessions (a shell ended, one was added elsewhere). */
  refresh: () => Promise<void>;
  /** Take a pane out of the panel without ending its shell (it moves to a window of its own); returns the shell's id. */
  release: (tabId: string, paneId: string) => string | null;
  /** Show a shell that is already running (one that was in a window of its own) as a new tab. */
  adopt: (sid: string) => Promise<void>;
}

async function startPane(cwd: string, elevated: boolean, title: string | null = null): Promise<Pane> {
  const pane: Pane = { id: uid(), sid: null, title: title ?? "", cwd, elevated, exited: null };
  try {
    pane.sid = await api.termSpawn(cwd, 80, 24, title, elevated);
  } catch (e) {
    pane.error = String(e);
  }
  return pane;
}

let initFlight: Promise<void> | null = null;

/** Match what the host is running with what the saved tabs say, and fill in whatever is missing. */
async function startUp(): Promise<void> {
  const { getState: get, setState: set } = useTerminal;
  try {
    const [info, live] = await Promise.all([api.termInfo(), api.termList()]);
    const liveBy = new Map(live.map((s) => [s.sid, s]));
    const restore = adv<boolean>(useStore.getState().settings, "terminal.restore");
    const used = new Set<string>();
    const tabs: Tab[] = [];
    for (const t of restore ? get().tabs : []) {
      const panes: Pane[] = [];
      for (const p of t.panes) {
        const s = p.sid ? liveBy.get(p.sid) : undefined;
        if (s) {
          used.add(s.sid);
          panes.push({ ...p, title: s.title || p.title, cwd: s.cwd || p.cwd, elevated: s.elevated, exited: s.alive ? null : s.exit ?? 0 });
        } else {
          // Saved SSH tabs require an active server login first; do not auto-connect before the user signs in.
          if (isSshLoc(p.cwd)) continue;
          // Its shell is gone (the app was closed): show what it printed, then start a new shell in the same folder.
          const transcript = p.sid ? (await api.termTranscript(p.sid, true).catch(() => null)) ?? undefined : undefined;
          const fresh = await startPane(p.cwd, p.elevated && info.os !== "windows", p.title || null);
          panes.push({ ...fresh, transcript });
        }
      }
      if (panes.length) tabs.push({ id: t.id, panes, active: panes.some((p) => p.id === t.active) ? t.active : panes[0].id });
    }
    // Shells that are still running but that no tab shows (popped out, or the tabs were lost): show them.
    for (const s of live.filter((x) => !used.has(x.sid))) {
      const pane: Pane = { id: uid(), sid: s.sid, title: s.title, cwd: s.cwd, elevated: s.elevated, exited: s.alive ? null : s.exit ?? 0 };
      tabs.push({ id: uid(), panes: [pane], active: pane.id });
    }
    set({ info, tabs, active: tabs.some((t) => t.id === get().active) ? get().active : tabs[0]?.id ?? null, ready: true, error: null });
    void api.termSweepTranscripts([]).catch(() => {});
    if (!tabs.length && get().open) await get().newTab();
  } catch (e) {
    set({ error: String(e), ready: true });
  }
}

const saved0 = loadSaved();

export const useTerminal = create<TerminalState>((set, get) => ({
  open: saved0.open,
  tabs: saved0.tabs.map((t) => ({ id: t.id, active: t.active, panes: t.panes.map((p) => ({ ...p, exited: null })) })),
  active: saved0.active,
  ready: false,
  info: null,
  error: null,

  setOpen: (open) => {
    set({ open });
    if (open && !get().ready) void get().init();
  },
  toggle: () => get().setOpen(!get().open),

  init: () => {
    // Opening the panel and the toggle can both ask at once; one start-up serves both.
    if (get().ready) return Promise.resolve();
    initFlight ??= startUp().finally(() => (initFlight = null));
    return initFlight;
  },

  newTab: async (opts = {}) => {
    if (!get().ready) await get().init();
    const cwd = opts.cwd ?? startFolder();
    const title = opts.title !== undefined ? opts.title : isSshLoc(cwd) ? remoteLabel(cwd) : null;
    const pane = await startPane(cwd, !!opts.elevated, title);
    const tab: Tab = { id: uid(), panes: [pane], active: pane.id };
    set((s) => ({ tabs: [...s.tabs, tab], active: tab.id, open: true }));
  },

  openForSsh: (url) => openOrRunSsh(url),

  openFileInSsh: (fileUrl, forceSudo = false) => {
    const folder = parentPath(fileUrl) ?? fileUrl;
    const { path: remotePath } = splitRemote(fileUrl);
    return openOrRunSsh(folder, sshNanoCommand(remotePath, forceSudo));
  },

  split: async () => {
    const { tabs, active } = get();
    const tab = tabs.find((t) => t.id === active);
    if (!tab) return get().newTab();
    const current = tab.panes.find((p) => p.id === tab.active) ?? tab.panes[0];
    const pane = await startPane(current.cwd, current.elevated);
    set((s) => ({ tabs: s.tabs.map((t) => (t.id === tab.id ? { ...t, panes: [...t.panes, pane], active: pane.id } : t)) }));
  },

  activate: (id) => set({ active: id }),
  activatePane: (tabId, paneId) => set((s) => ({ active: tabId, tabs: s.tabs.map((t) => (t.id === tabId ? { ...t, active: paneId } : t)) })),

  closePane: (tabId, paneId) => {
    const tab = get().tabs.find((t) => t.id === tabId);
    const pane = tab?.panes.find((p) => p.id === paneId);
    if (!tab || !pane) return;
    if (pane.sid) void api.termDo("remove", pane.sid).catch(() => {});
    if (tab.panes.length === 1) return dropTab(tabId);
    set((s) => ({
      tabs: s.tabs.map((t) => (t.id === tabId ? { ...t, panes: t.panes.filter((p) => p.id !== paneId), active: t.active === paneId ? t.panes.find((p) => p.id !== paneId)!.id : t.active } : t)),
    }));
  },

  closeTab: (id) => {
    const tab = get().tabs.find((t) => t.id === id);
    if (!tab) return;
    for (const p of tab.panes) if (p.sid) void api.termDo("remove", p.sid).catch(() => {});
    dropTab(id);
  },

  clearActive: () => {
    const tab = get().tabs.find((t) => t.id === get().active);
    const pane = tab?.panes.find((p) => p.id === tab.active);
    if (pane?.sid) void api.termDo("clear", pane.sid).catch(() => {});
    // The view itself is cleared by the pane when it sees this counter change.
    useTerminalClear.setState((c) => ({ tick: c.tick + 1, paneId: pane?.id ?? null }));
  },

  restart: async (tabId, paneId) => {
    const tab = get().tabs.find((t) => t.id === tabId);
    const old = tab?.panes.find((p) => p.id === paneId);
    if (!tab || !old) return;
    if (old.sid) void api.termDo("remove", old.sid).catch(() => {});
    const fresh = await startPane(old.cwd, old.elevated);
    set((s) => ({ tabs: s.tabs.map((t) => (t.id === tabId ? { ...t, panes: t.panes.map((p) => (p.id === paneId ? { ...fresh, id: paneId } : p)) } : t)) }));
  },

  patchPane: (paneId, patch) => set((s) => ({ tabs: s.tabs.map((t) => ({ ...t, panes: t.panes.map((p) => (p.id === paneId ? { ...p, ...patch } : p)) })) })),

  release: (tabId, paneId) => {
    const tab = get().tabs.find((t) => t.id === tabId);
    const pane = tab?.panes.find((p) => p.id === paneId);
    if (!tab || !pane?.sid) return null;
    if (tab.panes.length === 1) dropTab(tabId);
    else {
      set((s) => ({
        tabs: s.tabs.map((t) => (t.id === tabId ? { ...t, panes: t.panes.filter((p) => p.id !== paneId), active: t.active === paneId ? t.panes.find((p) => p.id !== paneId)!.id : t.active } : t)),
      }));
    }
    return pane.sid;
  },

  adopt: async (sid) => {
    if (!get().ready) await get().init();
    if (get().tabs.some((t) => t.panes.some((p) => p.sid === sid))) return;
    let found: TermSession | undefined;
    try {
      found = (await api.termList()).find((s) => s.sid === sid);
    } catch {
      /* the host is not answering; the shell is shown with what is known */
    }
    if (!found) return;
    const pane: Pane = { id: uid(), sid, title: found.title, cwd: found.cwd, elevated: found.elevated, exited: found.alive ? null : found.exit ?? 0 };
    const tab: Tab = { id: uid(), panes: [pane], active: pane.id };
    set((s) => ({ tabs: [...s.tabs, tab], active: tab.id, open: true }));
  },

  refresh: async () => {
    let live: TermSession[];
    try {
      live = await api.termList();
    } catch {
      return;
    }
    const by = new Map(live.map((s) => [s.sid, s]));
    set((s) => ({
      tabs: s.tabs.map((t) => ({
        ...t,
        panes: t.panes.map((p) => {
          const x = p.sid ? by.get(p.sid) : undefined;
          return x ? { ...p, title: x.title || p.title, cwd: x.cwd || p.cwd, exited: x.alive ? p.exited : x.exit ?? 0 } : p;
        }),
      })),
    }));
  },
}));

/** Remove a tab from the list (the host has already been told about its shells). */
function dropTab(id: string) {
  const { tabs, active } = useTerminal.getState();
  const left = tabs.filter((t) => t.id !== id);
  const i = tabs.findIndex((t) => t.id === id);
  useTerminal.setState({ tabs: left, active: active === id ? left[Math.min(i, left.length - 1)]?.id ?? null : active });
  // Nothing left: put the panel away.
  if (!left.length) useTerminal.setState({ open: false });
}

/** Tells the pane that was cleared to empty its screen. */
export const useTerminalClear = create<{ tick: number; paneId: string | null }>(() => ({ tick: 0, paneId: null }));

// Keep what the panel looks like for the next start.
useTerminal.subscribe((s, prev) => {
  // A terminal in a window of its own shows one shell and must not overwrite what the main window saved.
  if ((window as unknown as { __VIEW__?: string }).__VIEW__ === "terminal") return;
  if (s.open === prev.open && s.tabs === prev.tabs && s.active === prev.active) return;
  const out: Saved = {
    open: s.open,
    active: s.active,
    tabs: s.tabs.map((t) => ({ id: t.id, active: t.active, panes: t.panes.map((p) => ({ id: p.id, sid: p.sid, title: p.title, cwd: p.cwd, elevated: p.elevated })) })),
  };
  try {
    localStorage.setItem(KEY, JSON.stringify(out));
  } catch {
    /* not saved this time */
  }
});

/** Show or hide the terminal. If the layout has it hidden, showing it puts it back at the bottom. */
export function toggleTerminalPanel() {
  const t = useTerminal.getState();
  const s = useStore.getState();
  const layout = resolveLayout(s.settings?.appearance.layout);
  if (!t.open || layout.terminal.dock === "hidden") {
    if (layout.terminal.dock === "hidden") s.updateSettings((x) => void (x.appearance.layout = moveTo(layout, "terminal", "bottom")));
    t.setOpen(true);
  } else {
    t.setOpen(false);
  }
}
