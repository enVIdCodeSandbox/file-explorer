import { useT } from "./i18n";
import { useEffect, useState } from "react";
import { api, on } from "./api";
import { adv } from "./settings/advanced";
import { CaptionStrip, CommandBar, FilterPanel, Sidebar, TabBar, Toolbar } from "./components/Chrome";
import { useChrome } from "./util/chrome";
import { DetailsPane } from "./components/DetailsPane";
import { FileView } from "./components/FileView";
import { Shell, useLayout } from "./components/Layout";
import { Dialog, DragOverlay, OpenWithDialog, PropertiesDialog, Toast } from "./components/Overlays";
import { LockDialog } from "./components/Locks";
import { PickBar } from "./components/PickBar";
import { Setup } from "./components/Setup";
import { latestLabel } from "./components/UpdatesPanel";
import { ShareDialog } from "./components/Share";
import { BatchRename } from "./components/BatchRename";
import { ConnectServer } from "./components/ConnectServer";
import { SettingsView } from "./components/SettingsView";
import { Notices } from "./components/Notices";
import { DetachedTransfers, Transfers } from "./components/Transfers";
import { AnalyzerView } from "./components/Analyzer";
import { SmartClearView } from "./components/SmartClear";
import { DrivesView, HomeView, SearchView } from "./components/Views";
import { commandFor } from "./settings/keys";
import { ask, onJobFinished, redoLast, undoLast, useStore } from "./store";
import { useAppearance } from "./useAppearance";
import { HOME, THIS_PC, isAppLocation, isArchiveLoc, isRemote, parseLocation } from "./util/path";
import { Tooltips } from "./components/Tooltip";
import { listenForExternalDrops } from "./externalDrop";
import { panelsAt, type PanelId } from "./util/layout";
import { MobileApp } from "./components/Mobile";
import { TasksView } from "./components/Tasks";
import { useMobile } from "./util/device";

export default function App() {
  useT(); // keeps the page language (and every label) in step with the language setting
  const ready = useStore((s) => s.ready);
  const location = useStore((s) => s.tabs.find((t) => t.id === s.activeTab)?.location);
  const activeTab = useStore((s) => s.activeTab);
  const settingsOpen = useStore((s) => s.settingsOpen);
  const tasksOpen = useStore((s) => s.tasksOpen);
  const settings = useStore((s) => s.settings);
  const showDetails = useStore((s) => s.settings?.appearance.showDetailsPane);
  // "Start with the computer" is an operating-system setting: keep it in step with the choice made in Settings.
  const startWithComputer = useStore((s) => (s.unsaved ? null : s.settings?.advanced["tasks.startWithComputer"] === true));
  useEffect(() => {
    if (startWithComputer === null || !useStore.getState().ready) return;
    void api.autostartGet().then((now) => (now === startWithComputer ? undefined : api.autostartSet(startWithComputer))).catch((e) => {
      if (startWithComputer) useStore.getState().showToast(String(e));
    });
  }, [startWithComputer]);
  const os = useStore((s) => s.info?.os);
  const [fatal, setFatal] = useState<string | null>(null);
  const [filterAnchor, setFilterAnchor] = useState<DOMRect | null>(null);
  const [layout] = useLayout();
  const chrome = useChrome();
  const tabCount = useStore((s) => s.tabs.length);
  const mobile = useMobile();

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
    s.refreshLink();
    // A cable plugged in or Wi-Fi taking over changes what thumbnails of network shares do in automatic mode.
    const linkTimer = setInterval(() => !document.hidden && useStore.getState().refreshLink(), 10_000);
    const subscriptions = [
      on("transfer-progress", (p) => useStore.getState().onProgress(p)),
      on("delete-progress", (p) => useStore.getState().onDeleteProgress(p)),
      on("job-finished", onJobFinished),
      on("fs-changed", (dirs) => useStore.getState().bumpDirs(dirs)),
      on("dir-updated", (l) => useStore.getState().bumpDirs([l.path])),
      on("index-status", (indexStatus) => useStore.setState({ indexStatus })),
      on("update-available", (u) =>
        useStore.getState().notify("info", "A new version is out", `${latestLabel(u.latest)} is ready to download`, {
          facts: [["Your build", u.sha?.slice(0, 7) ?? u.version], ["New version", latestLabel(u.latest)], ["Where", "Settings > Updates"]],
        }),
      ),
      on("update-ready", () => void askToRestart()),
      on("budget-changed", (budget) => {
        document.documentElement.toggleAttribute("data-background", !!budget.background);
        useStore.setState({ budget });
      }),
      on("drives-changed", (p) => useStore.getState().onDrivesChanged(p)),
      on("detached", ({ kind, open }) => {
        const s = useStore.getState();
        if (kind !== "settings" && kind !== "transfers") return;
        useStore.setState({ detached: { ...s.detached, [kind]: open } });
        // The main window notes what is detached, for "remember detached windows".
        if (!s.isSecondary) rememberDetached({ ...s.detached, [kind]: open });
      }),
      // Another app (or a second start of this one) asked for a folder.
      on("task-notice", (m) => useStore.getState().showToast(m)),
      on("launch", () => void api.takeLaunch().then((l) => useStore.getState().showLaunches(l)).catch(() => {})),
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
    return () => {
      clearInterval(linkTimer);
      subscriptions.forEach((p) => void p.then((off) => off()));
    };
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
        case "reopenTab": s.reopenTab(); break;
        case "settings": s.openSettings(); break;
        case "nextTab": s.activate(s.tabs[(i + 1) % n].id); break;
        case "prevTab": s.activate(s.tabs[(i + n - 1) % n].id); break;
        case "toggleSidebar": s.updateSettings((x) => void (x.appearance.sidebarCollapsed = !x.appearance.sidebarCollapsed)); break;
        case "toggleDetails": s.updateSettings((x) => void (x.appearance.showDetailsPane = !x.appearance.showDetailsPane)); break;
        case "toggleTypeFind": s.updateSettings((x) => void (x.advanced["files.typeFind"] = !adv<boolean>(x, "files.typeFind"))); s.showToast(adv<boolean>(useStore.getState().settings, "files.typeFind") ? "Type to find is on." : "Type to find is off."); break;
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

  // Phones and tablets: the same pages and dialogs in a one-column touch layout.
  if (mobile)
    return (
      <MobileApp
        location={location}
        overlays={
          <>
            <PickBar />
            <Transfers />
            <Notices />
            {settings && !settings.setupDone && <Setup />}
            {settingsOpen && <SettingsView />}
            <PropertiesDialog />
            <OpenWithDialog />
            <LockDialog />
            <ShareDialog />
            <BatchRename />
            <ConnectServer />
            <Dialog />
            <Toast />
          </>
        }
      />
    );

  const loc = parseLocation(location);
  // A hidden tab strip (Finder and GNOME show it only with two or more tabs) cannot own the window buttons.
  const topFirst = panelsAt(layout, "top").filter((id) => !(id === "tabs" && chrome !== "windows" && tabCount < 2))[0];
  const tabsOwnWindowButtons = topFirst === "tabs";
  // The Finder-like and GNOME-like windows put the window buttons in the header row.
  const headerOwnsControls = chrome !== "windows" && topFirst === "address" && !layout.address.collapsed;

  const render = (id: PanelId, dock: string) => {
    const vertical = dock === "left" || dock === "right";
    switch (id) {
      case "tabs":
        // Finder and GNOME Files show the tab strip only once there is more than one tab.
        if (chrome !== "windows" && tabCount < 2) return null;
        return <TabBar controls={tabsOwnWindowButtons && dock === "top"} vertical={vertical} />;
      case "address":
        if (chrome === "macos")
          return <Toolbar parts={["nav", "title", "search"]} controls={headerOwnsControls} extra={<CommandBar onFilter={setFilterAnchor} only={["view", "sort", "share", "more"]} iconOnly />} />;
        if (chrome === "linux")
          return <Toolbar parts={["nav", "address", "search"]} controls={headerOwnsControls} extra={<CommandBar onFilter={setFilterAnchor} only={["view", "sort", "more"]} iconOnly />} />;
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
      {!(tabsOwnWindowButtons && layout.tabs.dock === "top" && !layout.tabs.collapsed) && !headerOwnsControls && <CaptionStrip />}
      <PickBar />
      <Shell
        render={render}
        content={
          <div className="content-inner" key={activeTab}>
            {loc.kind === "home" && <HomeView />}
            {loc.kind === "thispc" && <DrivesView />}
            {loc.kind === "search" && <SearchView query={loc.query} scope={loc.scope} mode={loc.mode} />}
            {loc.kind === "analyze" && <AnalyzerView path={loc.path} />}
            {loc.kind === "clear" && <SmartClearView drive={loc.drive} />}
            {loc.kind === "folder" && <FileView path={loc.path} />}
            {/* Finder shows the path of the folder along the bottom, under the item count. */}
            {chrome === "macos" && loc.kind === "folder" && <Toolbar parts={["address"]} />}
          </div>
        }
      />
      <Transfers />
      <Notices />
      {settings && !settings.setupDone && <Setup />}
      {filterAnchor && <FilterPanel anchor={filterAnchor} onClose={() => setFilterAnchor(null)} />}
      {settingsOpen && <SettingsView />}
      {tasksOpen && <TasksView />}
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

let asking = false;
/** An update has been downloaded: restart now (the new version is installed and opens again) or wait. */
async function askToRestart() {
  if (asking) return;
  asking = true;
  try {
    const r = await ask({
      title: "The update is ready",
      message: "The new version has been downloaded. Restart FAST File Explorer to finish installing it. It reopens on its own, and transfers still running are stopped, so you may want to wait for them.",
      buttons: [
        { id: "restart", label: "Restart now", primary: true },
        { id: "later", label: "Later" },
      ],
    });
    if (r?.button === "restart") await api.updateInstall().catch((e) => useStore.getState().notify("error", "Could not install the update", String(e)));
    else useStore.getState().notify("info", "Update waiting", "Restart from Settings > Updates when you are ready");
  } finally {
    asking = false;
  }
}
