import { useEffect, useState } from "react";
import { api, on } from "./api";
import { adv } from "./settings/advanced";
import { CaptionStrip, CommandBar, FilterPanel, Sidebar, TabBar, Toolbar } from "./components/Chrome";
import { DetailsPane } from "./components/DetailsPane";
import { FileView } from "./components/FileView";
import { Shell, useLayout } from "./components/Layout";
import { Dialog, DragOverlay, OpenWithDialog, PropertiesDialog, Toast } from "./components/Overlays";
import { LockDialog } from "./components/Locks";
import { Setup } from "./components/Setup";
import { ShareDialog } from "./components/Share";
import { BatchRename } from "./components/BatchRename";
import { ConnectServer } from "./components/ConnectServer";
import { SettingsView } from "./components/SettingsView";
import { Notices } from "./components/Notices";
import { DetachedTransfers, Transfers } from "./components/Transfers";
import { AnalyzerView } from "./components/Analyzer";
import { DrivesView, HomeView, SearchView } from "./components/Views";
import { commandFor } from "./settings/keys";
import { onJobFinished, redoLast, undoLast, useStore } from "./store";
import { useAppearance } from "./useAppearance";
import { HOME, THIS_PC, isAppLocation, isArchiveLoc, isRemote, parseLocation } from "./util/path";
import { Tooltips } from "./components/Tooltip";
import { listenForExternalDrops } from "./externalDrop";
import { panelsAt, type PanelId } from "./util/layout";

export default function App() {
  const ready = useStore((s) => s.ready);
  const location = useStore((s) => s.tabs.find((t) => t.id === s.activeTab)?.location);
  const activeTab = useStore((s) => s.activeTab);
  const settingsOpen = useStore((s) => s.settingsOpen);
  const settings = useStore((s) => s.settings);
  const showDetails = useStore((s) => s.settings?.appearance.showDetailsPane);
  const os = useStore((s) => s.info?.os);
  const [fatal, setFatal] = useState<string | null>(null);
  const [filterAnchor, setFilterAnchor] = useState<DOMRect | null>(null);
  const [layout] = useLayout();

  useAppearance();

  // Tell the backend which folders are open, so it can watch them and refresh
  // the listing when another program changes one. The key is a string so the
  // effect only runs when the set of folders really changes.
  const openFolders = useStore((s) => {
    if (!s.ready || !s.settings?.general.liveRefresh) return "";
    return [...new Set(s.tabs.map((t) => t.location).filter((l) => !isAppLocation(l) && !isRemote(l) && !isArchiveLoc(l)))].sort().join("\n");
  });
  useEffect(() => {
    const timer = window.setTimeout(() => void api.watchDirs(openFolders ? openFolders.split("\n") : []).catch(() => {}), 150);
    return () => window.clearTimeout(timer);
  }, [openFolders]);

  useEffect(() => {
    const s = useStore.getState();
    s.init().catch((e) => setFatal(String(e)));
    const subscriptions = [
      on("transfer-progress", (p) => useStore.getState().onProgress(p)),
      on("delete-progress", (p) => useStore.getState().onDeleteProgress(p)),
      on("job-finished", onJobFinished),
      on("fs-changed", (dirs) => useStore.getState().bumpDirs(dirs)),
      on("dir-updated", (l) => useStore.getState().bumpDirs([l.path])),
      on("index-status", (indexStatus) => useStore.setState({ indexStatus })),
      on("budget-changed", (budget) => useStore.setState({ budget })),
      on("detached", ({ kind, open }) => {
        const s = useStore.getState();
        if (kind !== "settings" && kind !== "transfers") return;
        useStore.setState({ detached: { ...s.detached, [kind]: open } });
        // The main window notes what is detached, for "remember detached windows".
        if (!s.isSecondary) rememberDetached({ ...s.detached, [kind]: open });
      }),
      on("reattach", ({ kind }) => {
        if (kind === "settings") useStore.setState({ settingsOpen: true });
        else if (kind === "transfers") useStore.setState({ transfersOpen: true });
      }),
      // A tab was dragged here from another window: slot it in under the pointer.
      on("tab-drop", ({ location, x }) => {
        const slots = [...document.querySelectorAll<HTMLElement>(".tab-bar .tab[data-tab-id]")];
        const index = slots.filter((el) => {
          const b = el.getBoundingClientRect();
          return x > b.left + b.width / 2;
        }).length;
        useStore.setState({ incomingTab: null });
        useStore.getState().insertTabs([location], index);
      }),
      // Another window is dragging a tab over our tab strip: outline where it would land.
      on("tab-hover", ({ x, w }) => {
        if (x == null) {
          if (useStore.getState().incomingTab) useStore.setState({ incomingTab: null });
          return;
        }
        const slots = [...document.querySelectorAll<HTMLElement>(".tab-bar .tab[data-tab-id]")];
        const index = slots.filter((el) => {
          const b = el.getBoundingClientRect();
          return x > b.left + b.width / 2;
        }).length;
        const cur = useStore.getState().incomingTab;
        if (!cur || cur.index !== index || cur.w !== w) useStore.setState({ incomingTab: { index, w } });
      }),
      // Files dragged in from other programs.
      listenForExternalDrops().then((off) => () => off()),
      // Settings changed in another window: follow it.
      on("settings-changed", (next) => {
        const cur = useStore.getState().settings;
        if (JSON.stringify(cur) !== JSON.stringify(next)) useStore.setState({ settings: next });
      }),
    ];
    return () => subscriptions.forEach((p) => void p.then((off) => off()));
  }, []);

  // Bring back panels that were detached last time, only if asked to.
  useEffect(() => {
    if (!ready) return;
    const s = useStore.getState();
    if (s.isSecondary || !adv<boolean>(s.settings, "windows.rememberDetached")) return;
    const was = recalledDetached();
    if (was.settings) void api.openSettingsWindow().catch(() => {});
    if (was.transfers) void api.openTransfersWindow().catch(() => {});
  }, [ready]);

  // Give memory back a couple of minutes after the window goes to the background.
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const away = () => {
      clearTimeout(timer);
      if (!adv<boolean>(useStore.getState().settings, "perf.compactWhenIdle")) return;
      timer = setTimeout(() => void api.compactMemory().catch(() => {}), 120_000);
    };
    const back = () => clearTimeout(timer);
    window.addEventListener("blur", away);
    window.addEventListener("focus", back);
    return () => {
      clearTimeout(timer);
      window.removeEventListener("blur", away);
      window.removeEventListener("focus", back);
    };
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const s = useStore.getState();
      if (s.dialog) return;
      // While a shortcut is being recorded in Settings, leave the keys alone.
      if ((e.target as HTMLElement | null)?.closest?.("[data-capture-key]")) return;
      // Undo and Redo work wherever the focus is (a dialog that just closed leaves
      // none), except while typing, where the text box has its own undo.
      const typing = (e.target as HTMLElement | null)?.closest?.("input, textarea, select, [contenteditable]");
      const mod = e.ctrlKey || e.metaKey;
      if (mod && !e.altKey && !typing && ["z", "y"].includes(e.key.toLowerCase())) {
        e.preventDefault();
        if (e.key.toLowerCase() === "y" || e.shiftKey) void redoLast();
        else void undoLast();
        return;
      }
      const command = commandFor(s.settings, e);
      if (!command) return;
      const i = s.tabs.findIndex((t) => t.id === s.activeTab);
      const n = s.tabs.length;
      switch (command) {
        case "newTab": s.newTab(); break;
        case "closeTab": s.closeTab(s.activeTab); break;
        case "settings": s.openSettings(); break;
        case "nextTab": s.activate(s.tabs[(i + 1) % n].id); break;
        case "prevTab": s.activate(s.tabs[(i + n - 1) % n].id); break;
        case "toggleSidebar": s.updateSettings((x) => void (x.appearance.sidebarCollapsed = !x.appearance.sidebarCollapsed)); break;
        case "toggleDetails": s.updateSettings((x) => void (x.appearance.showDetailsPane = !x.appearance.showDetailsPane)); break;
        case "editLayout": useStore.setState({ layoutEdit: !s.layoutEdit }); break;
        case "thisPc": s.navigate(THIS_PC); break;
        case "home": s.navigate(HOME); break;
        case "toggleHidden": s.updateSettings((x) => void (x.general.showHidden = !x.general.showHidden)); break;
        default: return;
      }
      e.preventDefault();
    };
    // The webview's own right-click menu (Reload, Inspect…) is not part of the app.
    const onContext = (e: MouseEvent) => {
      const el = e.target as HTMLElement;
      if (!["INPUT", "TEXTAREA"].includes(el.tagName)) e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("contextmenu", onContext);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("contextmenu", onContext);
    };
  }, []);

  if (fatal) return <div className="empty-state" data-error>{fatal}</div>;
  if (!ready || !location) return <div className="app-loading" />;

  if ((window as unknown as { __VIEW__?: string }).__VIEW__ === "transfers") return <DetachedTransfers />;

  if ((window as unknown as { __VIEW__?: string }).__VIEW__ === "settings") return <SettingsView detached />;

  const loc = parseLocation(location);
  const topFirst = panelsAt(layout, "top")[0];
  const tabsOwnWindowButtons = topFirst === "tabs";

  const render = (id: PanelId, dock: string) => {
    const vertical = dock === "left" || dock === "right";
    switch (id) {
      case "tabs":
        return <TabBar controls={tabsOwnWindowButtons && dock === "top"} vertical={vertical} />;
      case "address":
        return <Toolbar />;
      case "commands":
        return <CommandBar vertical={vertical} onFilter={setFilterAnchor} />;
      case "sidebar":
        return <Sidebar />;
      case "details":
        return showDetails && loc.kind === "folder" ? <DetailsPane /> : null;
    }
  };

  return (
    <div className="app" data-os={os}>
      {!(tabsOwnWindowButtons && layout.tabs.dock === "top" && !layout.tabs.collapsed) && <CaptionStrip />}
      <Shell
        render={render}
        content={
          <div className="content-inner" key={activeTab}>
            {loc.kind === "home" && <HomeView />}
            {loc.kind === "thispc" && <DrivesView />}
            {loc.kind === "search" && <SearchView query={loc.query} scope={loc.scope} />}
            {loc.kind === "analyze" && <AnalyzerView path={loc.path} />}
            {loc.kind === "folder" && <FileView path={loc.path} />}
          </div>
        }
      />
      <Transfers />
      <Notices />
      {settings && !settings.setupDone && <Setup />}
      {filterAnchor && <FilterPanel anchor={filterAnchor} onClose={() => setFilterAnchor(null)} />}
      {settingsOpen && <SettingsView />}
      <PropertiesDialog />
      <OpenWithDialog />
      <LockDialog />
      <ShareDialog />
      <BatchRename />
      <ConnectServer />
      <DragOverlay />
      <Tooltips />
      <Dialog />
      <Toast />
    </div>
  );
}

const DETACHED_KEY = "fast.detached";

function rememberDetached(flags: { settings: boolean; transfers: boolean }) {
  try {
    localStorage.setItem(DETACHED_KEY, JSON.stringify(flags));
  } catch {
    /* storage unavailable: the panels simply come back docked */
  }
}

function recalledDetached(): { settings: boolean; transfers: boolean } {
  try {
    return { settings: false, transfers: false, ...JSON.parse(localStorage.getItem(DETACHED_KEY) ?? "{}") };
  } catch {
    return { settings: false, transfers: false };
  }
}
