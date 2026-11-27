import { useT } from "./i18n";
import { onSuspendChange, isSuspended, startUnloader, rememberScrolls, restoreScrolls, keepSelection } from "./util/unload";
import { dropThumbCache } from "./components/Thumb";
import { Suspense, lazy, useEffect, useState, useSyncExternalStore } from "react";
import { adoptJobs, api, on } from "./api";
import { adv } from "./settings/advanced";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { CaptionStrip, CommandBar, FilterPanel, Sidebar, TabBar, Toolbar } from "./components/Chrome";
import { isWindowsChrome, useChrome } from "./util/chrome";
import { DetailsPane } from "./components/DetailsPane";
import { FileView } from "./components/FileView";
import { Shell, useLayout } from "./components/Layout";
import { Dialog, DragOverlay, OpenWithDialog, PropertiesDialog, Toast } from "./components/Overlays";
import { PermissionsDialog } from "./components/PermissionsDialog";
import { LockDialog } from "./components/Locks";
import { IconContextMenu, IconPickerDialog } from "./components/IconPicker";
import { ChooseDetails } from "./components/ChooseDetails";
import { openTools } from "./components/toolsOpen";
import { MillerColumns } from "./components/Panes";
import { SplitArea } from "./components/Split";
import { MergeOffer } from "./components/MergeOffer";
import { handOver, useMerge } from "./util/mergeWindows";
import { paneCommand, plainFolder } from "./split/state";
import { startTaskbarProgress } from "./util/taskbarWire";
import { ButtonBar, CommandLine } from "./components/UserCommands";
import { isMobileOs } from "./util/native";
import { toggleTerminalPanel, useTerminal } from "./terminal/state";
import { PickBar } from "./components/PickBar";
import { latestLabel } from "./components/UpdatesPanel";
import { Notices } from "./components/Notices";
import { DetachedTransfers, Transfers } from "./components/Transfers";
import { DrivesView, HomeView, SearchView } from "./components/Views";
import { commandById, commandsFor, mouseComboOf } from "./settings/keys";
import { startLiveTracker } from "./util/liveFiles";
import { ask, onJobFinished, redoLast, undoLast, useStore } from "./store";
import { useAppearance } from "./useAppearance";
import { HOME, THIS_PC, isAppLocation, isArchiveLoc, isRemote, isSshLoc, parentPath, parseLocation } from "./util/path";
import { Tooltips } from "./components/Tooltip";
import { listenForExternalDrops } from "./externalDrop";
import { panelsAt, type PanelId } from "./util/layout";
import { useMobile } from "./util/device";
import { shortVersion } from "./util/format";
import { crumb, installTrail } from "./util/crashTrail";
import { whenIdle } from "./util/idle";

// Parts that most sessions never open are loaded when first needed, so the first screen has less to read.
const Tools = lazy(() => import("./components/Tools").then((m) => ({ default: m.Tools })));
const TodoDialog = lazy(() => import("./components/Todo").then((m) => ({ default: m.TodoDialog })));
const TasksView = lazy(() => import("./components/Tasks").then((m) => ({ default: m.TasksView })));
const Viewer = lazy(() => import("./components/Viewer").then((m) => ({ default: m.Viewer })));
const Setup = lazy(() => import("./components/Setup").then((m) => ({ default: m.Setup })));
const SettingsView = lazy(() => import("./components/SettingsView").then((m) => ({ default: m.SettingsView })));
const TerminalPanel = lazy(() => import("./components/Terminal").then((m) => ({ default: m.TerminalPanel })));
const DetachedTerminal = lazy(() => import("./components/Terminal").then((m) => ({ default: m.DetachedTerminal })));
const BatchRename = lazy(() => import("./components/BatchRename").then((m) => ({ default: m.BatchRename })));
const MacAccessPrompt = lazy(() => import("./components/MacAccess").then((m) => ({ default: m.MacAccessPrompt })));
const NetworkSignIn = lazy(() => import("./components/NetworkSignIn").then((m) => ({ default: m.NetworkSignIn })));
const ConnectServer = lazy(() => import("./components/ConnectServer").then((m) => ({ default: m.ConnectServer })));
const ShareDialog = lazy(() => import("./components/Share").then((m) => ({ default: m.ShareDialog })));
const AnalyzerView = lazy(() => import("./components/Analyzer").then((m) => ({ default: m.AnalyzerView })));
const TrashView = lazy(() => import("./components/Trash").then((m) => ({ default: m.TrashView })));
const SmartClearView = lazy(() => import("./components/SmartClear").then((m) => ({ default: m.SmartClearView })));
const MobileApp = lazy(() => import("./components/Mobile").then((m) => ({ default: m.MobileApp })));

/**
 * Parts that load their code on demand. Each has its own boundary, so the window never waits for them: without it
 * the first screen stays blank until every one of these files has been read. `later` holds back the ones that
 * draw nothing until opened, so their files are read after the first screen is up.
 */
function Part({ children, later }: { children: React.ReactNode; later?: boolean }) {
  const [on, setOn] = useState(!later);
  useEffect(() => (later ? whenIdle(() => setOn(true), 300) : undefined), [later]);
  return on ? <Suspense fallback={null}>{children}</Suspense> : null;
}

function AppInner() {
  // Follows files that are being written, so their size and speed stay current.
  // Progress on the icon in the taskbar while something is copying, deleting or running.
  useEffect(() => startTaskbarProgress(), []);

  useEffect(() => {
    let stop: (() => void) | undefined;
    const cancel = whenIdle(() => (stop = startLiveTracker()), 1000);
    return () => {
      cancel();
      stop?.();
    };
  }, []);
  useT(); // keeps the page language (and every label) in step with the language setting
  const ready = useStore((s) => s.ready);
  const location = useStore((s) => s.tabs.find((t) => t.id === s.activeTab)?.location);
  const activeTab = useStore((s) => s.activeTab);
  const settingsOpen = useStore((s) => s.settingsOpen);
  const tasksOpen = useStore((s) => s.tasksOpen);
  const settings = useStore((s) => s.settings);
  const showDetails = useStore((s) => s.settings?.appearance.showDetailsPane);
  const terminalOpen = useTerminal((s) => s.open);
  const info = useStore((s) => s.info);
  // The crash log's trail: clicks, keys, commands, and a one-line picture of the window.
  useEffect(() => {
    let stop: (() => void) | undefined;
    const cancel = whenIdle(() => {
    stop = installTrail(
      () => {
        const s = useStore.getState();
        const tab = s.tabs.find((t) => t.id === s.activeTab);
        const where = tab?.location ?? "";
        const kind = /^[a-z][a-z0-9+.-]+:\/\//i.test(where) ? where.split("://")[0] : where.startsWith("\\\\") ? "network share (UNC)" : "local";
        return [
          `window ${window.innerWidth}x${window.innerHeight} @${window.devicePixelRatio}x`,
          `tabs ${s.tabs.length}, active folder: ${where} (${kind})`,
          `selected items ${s.selection.length}`,
          `performance tier ${s.budget?.tier ?? "?"}`,
          `transfers running ${Object.keys(s.transfers).length}`,
          `second pane ${s.settings?.advanced["pane.second"] ? "on" : "off"}`,
        ].join("\n");
      },
      (kind, message) => void api.crashReport(kind, message).catch(() => {}),
      () => useStore.getState().settings?.advanced["crash.trail"] !== false,
    );
    }, 1000);
    return () => {
      cancel();
      stop?.();
    };
  }, []);
  // Selected text (Ctrl+A in the Details pane, say) goes away when you click anywhere that is not text you can select.
  useEffect(() => {
    const down = (e: PointerEvent) => {
      const sel = window.getSelection();
      if (!sel || sel.isCollapsed) return;
      const el = e.target instanceof Element ? e.target : null;
      if (el && getComputedStyle(el).userSelect === "text") return;
      sel.removeAllRanges();
    };
    const esc = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      const a = document.activeElement;
      if (a instanceof HTMLInputElement || a instanceof HTMLTextAreaElement) return;
      window.getSelection()?.removeAllRanges();
    };
    window.addEventListener("pointerdown", down, true);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("pointerdown", down, true);
      window.removeEventListener("keydown", esc);
    };
  }, []);
  // Tell the person when the last run stopped unexpectedly, and where the report is.
  useEffect(() => {
    if (!ready) return;
    return whenIdle(() => void api.crashPrevious().then((name) => {
      if (!name) return;
      crumb("startup: the previous run ended unexpectedly");
      const { notify, showToast } = useStore.getState();
      notify("error", "The app closed unexpectedly last time", "A crash report was saved on this computer.", {
        important: true,
        seconds: 90,
        actions: [
          {
            label: "Send to developer",
            // Filled into the bug report form for the person: nothing to open, nothing to press.
            // Once: the backend refuses a report that was already sent, and says so.
            run: () => void api.crashSend(name).then((m) => showToast(m)).catch((e) => showToast(String(e))),
          },
          {
            label: "View crash log",
            run: () => {
              useStore.setState({ settingsSection: "Storage", crashView: name });
              useStore.getState().openSettings();
            },
          },
          {
            label: "Save crash log",
            run: () => void api.crashLogSave(name).then((to) => to && showToast(`Saved to ${to}`)).catch((e) => showToast(String(e))),
          },
        ],
      });
    }).catch(() => {}), 2000);
  }, [ready]);

  // "Start with the computer" is an operating-system setting: keep it in step with the choice made in Settings.
  const startWithComputer = useStore((s) => (s.unsaved ? null : s.settings?.advanced["tasks.startWithComputer"] === true));
  useEffect(() => {
    if (startWithComputer === null || !useStore.getState().ready) return;
    return whenIdle(() => void api.autostartGet().then((now) => (now === startWithComputer ? undefined : api.autostartSet(startWithComputer))).catch((e) => {
      if (startWithComputer) useStore.getState().showToast(String(e));
    }), 3000);
  }, [startWithComputer]);
  const os = useStore((s) => s.info?.os);
  const [fatal, setFatal] = useState<string | null>(null);
  const [filterAnchor, setFilterAnchor] = useState<DOMRect | null>(null);
  const [layout] = useLayout();
  const chrome = useChrome();
  const tabCount = useStore((s) => s.tabs.length);
  const mobile = useMobile();

  useAppearance();

  // Out of sight (minimized, covered, another desktop) for long enough: the heavy interface comes down and returns
  // when the window shows again. How long depends on the performance level, or the choice in Settings > Performance.
  const suspended = useSyncExternalStore(onSuspendChange, isSuspended);
  useEffect(
    () =>
      startUnloader({
        settings: () => useStore.getState().settings,
        mobile: () => isMobileOs(useStore.getState().info?.os),
        before: () => {
          rememberScrolls();
          keepSelection(useStore.getState().selection);
        },
        after: restoreScrolls,
        dropPictures: dropThumbCache,
      }),
    [],
  );

  // Entering or switching to an already-connected SSH folder opens the terminal routed there.
  useEffect(() => {
    if (!ready || !location || !isSshLoc(location)) return;
    let alive = true;
    void api
      .remoteIsConnected(location)
      .then((ok) => {
        if (alive && ok) return useTerminal.getState().openForSsh(location);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [ready, location]);

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
    const firstLink = whenIdle(() => useStore.getState().refreshLink(), 1500);
    // A cable plugged in or Wi-Fi taking over changes what thumbnails of network shares do in automatic mode.
    const linkTimer = setInterval(() => !document.hidden && useStore.getState().refreshLink(), 10_000);
    const subscriptions = [
      on("transfer-progress", (p) => useStore.getState().onProgress(p)),
      on("delete-progress", (p) => useStore.getState().onDeleteProgress(p)),
      on("job-adopted", (ids) => adoptJobs(ids)),
      on("job-finished", onJobFinished),
      on("fs-changed", (dirs) => useStore.getState().bumpDirs(dirs)),
      on("dir-updated", (l) => useStore.getState().bumpDirs([l.path])),
      on("index-status", (indexStatus) => useStore.setState({ indexStatus })),
      on("update-available", (u) =>
        useStore.getState().notify("info", "A new version is out", `${latestLabel(u.latest)} is ready to download`, {
          facts: [["Your build", u.sha?.slice(0, 7) ?? shortVersion(u.version)], ["New version", latestLabel(u.latest)], ["Where", "Settings > Updates"]],
        }),
      ),
      on("update-ready", () => void askToRestart()),
      on("update-failed", (i) => void tellUpdateFailed(i.failed?.note ?? "")),
      on("budget-changed", (budget) => {
        document.documentElement.toggleAttribute("data-background", !!budget.background);
        useStore.setState({ budget });
      }),
      on("rendering-changed", (rendering) => {
        useStore.setState((s) => (s.info ? { info: { ...s.info, rendering } } : {}));
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
      on("reattach", ({ kind, payload }) => {
        if (kind === "terminal") {
          const sid = (payload as { sid?: string } | undefined)?.sid;
          if (sid) void useTerminal.getState().adopt(sid);
        } else if (kind === "settings") useStore.setState({ settingsOpen: true });
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
      // This window was held over another one: say what tabs it has (it minimizes), so that one can offer to take them.
      on("merge-offer-request", ({ target }) => {
        const tabs = useStore.getState().tabs.map((t) => ({ id: t.id, location: t.location }));
        void handOver(target, tabs).catch((e) => useStore.getState().showToast(String(e)));
      }),
      // A window is held over this one, or was moved away.
      on("merge-hover", ({ on: over }) => useMerge.setState({ hover: over })),
      // Another window was dropped here: offer its tabs.
      on("merge-offer", (offer) => useMerge.setState({ offer, hover: false })),
      // The window that was dropped here was taken in: let go of what moved, and tidy up the emptied window.
      on("merge-release", ({ ids }) => {
        const left = useStore.getState().releaseTabs(ids);
        const win = getCurrentWindow();
        if (left > 0) void win.unminimize().catch(() => {});
        else if (adv<string>(useStore.getState().settings, "windows.afterMerge") === "close") void win.close();
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
      firstLink();
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
      // Undo and Redo work wherever the focus is (a dialog that just closed leaves none), except while typing,
      // where the text box has its own undo. Their keys are the ones set in Settings > Advanced > Keyboard.
      const typing = (e.target as HTMLElement | null)?.closest?.("input, textarea, select, [contenteditable]");
      const command = commandsFor(s.settings, e).find((id) => commandById(id)?.scope === "app") ?? null;
      if (!command) return;
      // In the built-in terminal these are the shell's own keys (Ctrl+R searches its history, Ctrl+Y pastes what was cut).
      if (["undo", "redo", "redoAlt", "refresh", "refreshAlt"].includes(command) && (e.target as HTMLElement | null)?.closest?.(".term-body, .xterm")) return;
      // With an extra pane of the split view in focus, the tab keys act on its tabs.
      // (Also while typing: with the cursor in a pane's address box, Ctrl+W used to close the main window's tab instead.)
      if ((!typing || ["closeTab", "newTab", "nextTab", "prevTab"].includes(command)) && !(e.target as HTMLElement | null)?.closest?.(".term-body, .xterm") && paneCommand(command)) {
        e.preventDefault();
        return;
      }
      if (typing && ["zoomIn", "zoomInNum", "zoomOut", "zoomOutNum", "zoomReset"].includes(command)) return;
      if (runAppCommand(command, { typing: !!typing, target: e.target as HTMLElement | null })) e.preventDefault();
    };
    // The extra mouse buttons (the side ones that go back and forward). The page must never go "back" itself.
    const stopNative = (e: MouseEvent) => {
      if (e.button === 3 || e.button === 4) e.preventDefault();
    };
    const onMouse = (e: MouseEvent) => {
      if (e.button !== 3 && e.button !== 4) return;
      e.preventDefault();
      const s = useStore.getState();
      if (s.dialog || !mouseComboOf(e)) return;
      if ((e.target as HTMLElement | null)?.closest?.("[data-capture-key]")) return;
      const command = commandsFor(s.settings, e)[0];
      if (command) runAppCommand(command, { typing: false, target: e.target as HTMLElement | null });
    };
    // The webview's own right-click menu (Reload, Inspect…) is not part of the app.
    const onContext = (e: MouseEvent) => {
      const el = e.target as HTMLElement;
      if (!["INPUT", "TEXTAREA"].includes(el.tagName)) e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("contextmenu", onContext);
    window.addEventListener("mousedown", stopNative, true);
    window.addEventListener("mouseup", onMouse, true);
    window.addEventListener("auxclick", stopNative, true);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("contextmenu", onContext);
      window.removeEventListener("mousedown", stopNative, true);
      window.removeEventListener("mouseup", onMouse, true);
      window.removeEventListener("auxclick", stopNative, true);
    };
  }, []);

  if (fatal) return <div className="empty-state" data-error>{fatal}</div>;
  if (!ready || !location) return <div className="app-loading" />;

  if ((window as unknown as { __VIEW__?: string }).__VIEW__ === "transfers") return <DetachedTransfers />;

  if ((window as unknown as { __VIEW__?: string }).__VIEW__ === "terminal") {
    const sid = (window as unknown as { __SID__?: string }).__SID__ ?? "";
    return <Part><DetachedTerminal sid={sid} /></Part>;
  }

  if ((window as unknown as { __VIEW__?: string }).__VIEW__ === "settings") return <Part><SettingsView detached /></Part>;

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
            {settings && !settings.setupDone && <Part><Setup /></Part>}
            {settingsOpen && <Part><SettingsView /></Part>}
            <PropertiesDialog />
            <PermissionsDialog />
            <OpenWithDialog />
            <LockDialog />
            <IconPickerDialog />
            <IconContextMenu />
            <ChooseDetails />
            <Part later><Viewer /></Part>
            <Part later><Tools /></Part>
            <Part later><TodoDialog /></Part>
            <Part later><ShareDialog /></Part>
            <Part later><BatchRename /></Part>
            <Part later><ConnectServer /></Part>
            <Part later><NetworkSignIn /></Part>
            <Part later><MacAccessPrompt /></Part>
            <Dialog />
            <Toast />
          </>
        }
      />
    );

  const loc = parseLocation(location);
  const winChrome = isWindowsChrome(chrome);
  // A hidden tab strip (Finder and GNOME show it only with two or more tabs) cannot own the window buttons.
  const topFirst = panelsAt(layout, "top").filter((id) => !(id === "tabs" && !winChrome && tabCount < 2))[0];
  const tabsOwnWindowButtons = topFirst === "tabs";
  // The Finder-like and GNOME-like windows put the window buttons in the header row.
  const headerOwnsControls = !winChrome && topFirst === "address" && !layout.address.collapsed;

  const renderLive = (id: PanelId, dock: string) => {
    const vertical = dock === "left" || dock === "right";
    switch (id) {
      case "tabs":
        // Finder and GNOME Files show the tab strip only once there is more than one tab.
        if (!winChrome && tabCount < 2) return null;
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
        return <Sidebar horizontal={dock === "top" || dock === "bottom"} />;
      case "details":
        return showDetails && loc.kind === "folder" ? <DetailsPane /> : null;
      case "terminal":
        return terminalOpen && !isMobileOs(info?.os) ? <Part><TerminalPanel dock={dock} /></Part> : null;
    }
  };

  const render = suspended ? () => null : renderLive;

  return (
    <div className="app" data-os={os} data-suspended={suspended || undefined}>
      {!(tabsOwnWindowButtons && layout.tabs.dock === "top" && !layout.tabs.collapsed) && !headerOwnsControls && <CaptionStrip />}
      <PickBar />
      <Shell
        render={render}
        content={
          suspended ? (
            <div className="content-inner" key="suspended" />
          ) : (
          <div className="content-inner" key={activeTab}>
            {loc.kind === "folder" && <ButtonBar />}
            <SplitArea
              folder={loc.kind === "folder" ? loc.path : plainFolder(location)}
              allowed={loc.kind !== "folder" || (!isRemote(loc.path) && !isArchiveLoc(loc.path))}
              main={
                <>
                  {loc.kind === "home" && <HomeView />}
                  {loc.kind === "thispc" && <DrivesView />}
                  {loc.kind === "trash" && <Part><TrashView /></Part>}
                  {loc.kind === "search" && <SearchView query={loc.query} scope={loc.scope} mode={loc.mode} />}
                  {loc.kind === "analyze" && <Part><AnalyzerView path={loc.path} /></Part>}
                  {loc.kind === "clear" && <Part><SmartClearView drive={loc.drive} /></Part>}
                  {loc.kind === "folder" && (adv<boolean>(settings, "view.miller") ? <MillerColumns path={loc.path} /> : <FileView path={loc.path} />)}
                </>
              }
            />
            {loc.kind === "folder" && <CommandLine />}
            {/* Finder shows the path of the folder along the bottom, under the item count. */}
            {chrome === "macos" && loc.kind === "folder" && <Toolbar parts={["address"]} />}
          </div>
          )
        }
      />
      <Transfers />
      <Notices />
      {settings && !settings.setupDone && <Part><Setup /></Part>}
      {filterAnchor && <FilterPanel anchor={filterAnchor} onClose={() => setFilterAnchor(null)} />}
      {settingsOpen && <Part><SettingsView /></Part>}
      {tasksOpen && <Part><TasksView /></Part>}
      <PropertiesDialog />
      <PermissionsDialog />
      <OpenWithDialog />
      <LockDialog />
      <IconPickerDialog />
      <IconContextMenu />
      <ChooseDetails />
      <Part later><Viewer /></Part>
      <Part later><Tools /></Part>
      <Part later><TodoDialog /></Part>
      <Part later><ShareDialog /></Part>
      <Part later><BatchRename /></Part>
      <Part later><ConnectServer /></Part>
      <Part later><NetworkSignIn /></Part>
      <Part later><MacAccessPrompt /></Part>
      <DragOverlay />
      <Tooltips />
      <Dialog />
      <Toast />
      <MergeOffer />
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

/** Do what a shortcut says, whether it came from a key or a mouse button. Says whether it was one of ours. */
function runAppCommand(command: string, ctx: { typing: boolean; target: HTMLElement | null }): boolean {
  const s = useStore.getState();
  const { typing, target } = ctx;
  const inTerminal = !!target?.closest?.(".term-body, .xterm");
  // In the built-in terminal these are the shell's own keys (Ctrl+R searches its history, Ctrl+Y pastes what was cut).
  if (["undo", "redo", "redoAlt", "refresh", "refreshAlt"].includes(command) && inTerminal) return false;
  const place = s.tabs.find((t) => t.id === s.activeTab)?.location ?? "";
  const i = s.tabs.findIndex((t) => t.id === s.activeTab);
  const n = s.tabs.length;
  const tabAt = (k: number) => s.tabs[Math.min(Math.max(k, 0), n - 1)];
  switch (command) {
    case "zoomIn":
    case "zoomInNum":
    case "zoomOut":
    case "zoomOutNum":
    case "zoomReset": {
      // Ctrl + and Ctrl - resize the folder view, as Ctrl + scroll does. (The built-in terminal sizes its own text.)
      if (isAppLocation(place) || inTerminal || !adv<boolean>(s.settings, "files.wheelZoom")) return false;
      s.zoomView(command === "zoomReset" ? 0 : command.startsWith("zoomIn") ? 1 : -1);
      return true;
    }
    case "back":
    case "backMouse": s.back(); return true;
    case "forward":
    case "forwardMouse": s.forward(); return true;
    case "goUp": {
      const here = parseLocation(place);
      if (here.kind !== "folder") return false;
      s.navigate(parentPath(place) ?? THIS_PC);
      return true;
    }
    case "undo": if (typing) return false; void undoLast(); return true;
    case "redo":
    case "redoAlt": if (typing) return false; void redoLast(); return true;
    case "refresh":
    case "refreshAlt": s.refreshTab(); return true;
    case "newTab": s.newTab(); return true;
    case "closeTab": s.closeTab(s.activeTab); return true;
    case "reopenTab": s.reopenTab(); return true;
    case "duplicateTab": s.duplicateTab(s.activeTab); return true;
    case "settings": s.openSettings(); return true;
    case "nextTab": s.activate(s.tabs[(i + 1) % n].id); return true;
    case "prevTab": s.activate(s.tabs[(i + n - 1) % n].id); return true;
    case "tabLast": s.activate(tabAt(n - 1).id); return true;
    case "toggleSidebar": s.updateSettings((x) => void (x.appearance.sidebarCollapsed = !x.appearance.sidebarCollapsed)); return true;
    case "toggleDetails": s.updateSettings((x) => void (x.appearance.showDetailsPane = !x.appearance.showDetailsPane)); return true;
    case "toggleTypeFind": s.updateSettings((x) => void (x.advanced["files.typeFind"] = !adv<boolean>(x, "files.typeFind"))); s.showToast(adv<boolean>(useStore.getState().settings, "files.typeFind") ? "Type to find is on." : "Type to find is off."); return true;
    case "editLayout": useStore.setState({ layoutEdit: !s.layoutEdit }); return true;
    case "toggleTerminal": toggleTerminalPanel(); return true;
    case "searchInFiles": openTools("grep"); return true;
    case "focusCommandLine": {
      if (!adv<boolean>(s.settings!, "ui.commandLine")) s.updateSettings((x) => void (x.advanced["ui.commandLine"] = true));
      setTimeout(() => document.querySelector<HTMLInputElement>(".command-line-input")?.focus(), 50);
      return true;
    }
    case "thisPc": s.navigate(THIS_PC); return true;
    case "home": s.navigate(HOME); return true;
    case "toggleHidden": s.updateSettings((x) => void (x.general.showHidden = !x.general.showHidden)); return true;
    case "fullscreen": void toggleFullscreen(); return true;
    default: {
      const m = /^tab([1-8])$/.exec(command);
      if (m) {
        s.activate(tabAt(Number(m[1]) - 1).id);
        return true;
      }
      return false;
    }
  }
}

async function toggleFullscreen() {
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    const w = getCurrentWindow();
    await w.setFullscreen(!(await w.isFullscreen()));
  } catch {
    /* no window to change (the browser preview, a phone) */
  }
}

export default function App() {
  return (
    <Suspense fallback={null}>
      <AppInner />
    </Suspense>
  );
}

/** The app came back as the same build after an update: say why, and offer to run the installer by hand. */
async function tellUpdateFailed(note: string) {
  const r = await ask({
    title: "The update did not install",
    message: `${note} It will not be downloaded again by itself.`,
    buttons: [
      { id: "manual", label: "Run the installer myself", primary: true },
      { id: "close", label: "Close" },
    ],
  });
  if (r?.button === "manual") await api.updateInstallManual().catch((e) => useStore.getState().notify("error", "Could not start the installer", String(e)));
}
