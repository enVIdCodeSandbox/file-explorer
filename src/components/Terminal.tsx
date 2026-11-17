// The built-in terminal panel: tabs, split panes, and one xterm.js screen for each shell kept by
// the terminal host (see src/terminal/state.ts and crates/fast-core/src/terminal).

import "@xterm/xterm/css/xterm.css";
import { fontStack, importedFamily } from "../util/fontAreas";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Terminal as XTerm } from "@xterm/xterm";
import { useEffect, useRef, useState } from "react";
import { api, on } from "../api";
import { adv } from "../settings/advanced";
import { commandFor } from "../settings/keys";
import { ask, useStore } from "../store";
import { DOCK_LABEL, PANELS, moveTo, type Dock } from "../util/layout";
import { startFolder, useTerminal, useTerminalClear, type Pane, type Tab } from "../terminal/state";
import { ContextMenu, type MenuItem, type MenuState } from "./ContextMenu";
import { Icon } from "./Icons";
import { useLayout } from "./Layout";
import { wheelSteps, zoomStep } from "../util/zoomKeys";

const encoder = new TextEncoder();
const toB64 = (bytes: Uint8Array) => {
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
};
const fromB64 = (b64: string) => Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));

const DEFAULT_FONT = 13;

/**
 * Only fixed-width fonts, ending in the system's own. The terminal draws each letter in a cell of one size, so a font
 * whose letters differ in width (what a theme may set for the app's own text) comes out cramped and uneven.
 */
export const TERMINAL_FONTS = '"Cascadia Mono", "Cascadia Code", Consolas, "Lucida Console", Menlo, "SF Mono", "DejaVu Sans Mono", "Liberation Mono", "Courier New", monospace';

/** The font stack to use: the person's choice (followed by the fixed-width ones) or the built-in list. */
export function terminalFont(chosen: string): string {
  const own = chosen.trim();
  return fontStack(chosen, TERMINAL_FONTS, own.startsWith("imported:") ? importedFamily(own.slice(9)) : null) || TERMINAL_FONTS;
}

/** Make the terminal text bigger or smaller (0 = back to the default); the size is the one in Settings, so it is kept. */
function zoomText(step: 1 | -1 | 0) {
  const st = useStore.getState();
  const now = adv<number>(st.settings, "terminal.fontSize");
  const next = step === 0 ? DEFAULT_FONT : Math.max(8, Math.min(32, now + step));
  if (next !== now) st.updateSettings((s) => void (s.advanced["terminal.fontSize"] = next));
}

/** The xterm option that tells it a Windows ConPTY is on the other end (nothing on other systems). */
export function windowsScreen(info: { os: string; windowsBuild?: number } | null | undefined) {
  if (info?.os !== "windows") return {};
  return { windowsPty: { backend: "conpty" as const, buildNumber: info.windowsBuild || 19045 } };
}

/** Colors for the screen, from the theme in use. */
function themeColors() {
  const css = getComputedStyle(document.documentElement);
  const v = (name: string, fallback: string) => css.getPropertyValue(name).trim() || fallback;
  return {
    background: v("--bg", "#1b1b1b"),
    foreground: v("--text", "#e6e6e6"),
    cursor: v("--accent", "#4cc2ff"),
    selectionBackground: "rgba(120,160,255,0.35)",
  };
}

function TermPane({ tab, pane, visible, focused }: { tab: Tab; pane: Pane; visible: boolean; focused: boolean }) {
  const host = useRef<HTMLDivElement>(null);
  const term = useRef<XTerm | null>(null);
  const fit = useRef<FitAddon | null>(null);
  const exited = useRef<number | null>(pane.exited);
  exited.current = pane.exited;
  const settings = useStore((s) => s.settings);
  const theme = useStore((s) => s.settings?.appearance.theme);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const clear = useTerminalClear();
  const { patchPane, activatePane, restart } = useTerminal.getState();

  // One screen per shell.
  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const s = useStore.getState().settings;
    const t = new XTerm({
      fontSize: adv<number>(s, "terminal.fontSize"),
      fontFamily: terminalFont(adv<string>(s, "terminal.fontFamily")),
      cursorStyle: adv<string>(s, "terminal.cursor") as "block" | "bar" | "underline",
      cursorBlink: adv<boolean>(s, "terminal.cursorBlink"),
      scrollback: adv<number>(s, "terminal.scrollback"),
      theme: themeColors(),
      allowProposedApi: true,
      macOptionIsMeta: true,
      // Windows shells run behind ConPTY, which redraws the screen itself when it is resized. The screen must know,
      // or both reflow the text and lines and letters come out doubled.
      ...windowsScreen(useTerminal.getState().info),
    });
    const f = new FitAddon();
    t.loadAddon(f);
    t.loadAddon(new WebLinksAddon((_e, url) => void api.termOpenUrl(url).catch(() => {})));
    t.open(el);
    term.current = t;
    fit.current = f;
    const sid = pane.sid;
    let live = true;
    let ready = !sid;
    const queue: Uint8Array[] = [];
    const unlisten: (() => void)[] = [];

    const note = (text: string) => t.write(`\r\n\x1b[2m${text}\x1b[0m\r\n`);
    void on("term-out", (e) => {
      if (e.sid !== sid) return;
      const bytes = fromB64(e.data);
      if (ready) t.write(bytes);
      else queue.push(bytes);
    }).then((u) => (live ? unlisten.push(u) : u()));
    void on("term-exit", (e) => {
      if (e.sid !== sid) return;
      patchPane(pane.id, { exited: e.code ?? 0 });
      note(`The process ended with code ${e.code ?? 0}. Press Enter to start a new one.`);
    }).then((u) => (live ? unlisten.push(u) : u()));

    // What it printed last time, then what the shell has printed since.
    void (async () => {
      if (pane.transcript) {
        t.write(fromB64(pane.transcript));
        note("Restored from last time. Those commands were not run again.");
        patchPane(pane.id, { transcript: undefined });
      }
      if (!sid) {
        note(pane.error ?? "The terminal could not start.");
        return;
      }
      try {
        // Measure the screen before replaying: what the shell printed was laid out for a screen of the right size.
        if (el.offsetWidth > 0 && el.offsetHeight > 0) {
          try {
            f.fit();
          } catch {
            /* hidden */
          }
        }
        const a = await api.termAttach(sid);
        if (!live) return;
        t.write(fromB64(a.data));
        ready = true;
        for (const q of queue.splice(0)) t.write(q);
        if (!a.info.alive) note(`The process ended with code ${a.info.exit ?? 0}. Press Enter to start a new one.`);
        f.fit();
        void api.termResize(sid, t.cols, t.rows).catch(() => {});
      } catch (e) {
        note(String(e));
      }
    })();

    t.onData((d) => {
      if (exited.current !== null) {
        if (d === "\r") void restart(tab.id, pane.id);
        return;
      }
      if (sid) void api.termInput(sid, toB64(encoder.encode(d))).catch(() => {});
    });
    t.onBinary((d) => {
      if (sid && exited.current === null) void api.termInput(sid, toB64(Uint8Array.from(d, (c) => c.charCodeAt(0)))).catch(() => {});
    });
    t.onResize(({ cols, rows }) => {
      if (sid && ready) void api.termResize(sid, cols, rows).catch(() => {});
    });
    let titleTimer: ReturnType<typeof setTimeout> | undefined;
    t.onTitleChange((title) => {
      patchPane(pane.id, { title });
      clearTimeout(titleTimer);
      if (sid) titleTimer = setTimeout(() => void api.termDo("title", sid, title).catch(() => {}), 400);
    });
    t.onSelectionChange(() => {
      if (adv<boolean>(useStore.getState().settings, "terminal.copyOnSelect") && t.hasSelection()) void navigator.clipboard.writeText(t.getSelection()).catch(() => {});
    });
    const copy = () => {
      if (t.hasSelection()) void navigator.clipboard.writeText(t.getSelection()).catch(() => {});
    };
    const paste = () => void navigator.clipboard.readText().then((x) => t.paste(x)).catch(() => {});
    t.attachCustomKeyEventHandler((e) => {
      if (e.type !== "keydown") return true;
      const ctrl = e.ctrlKey || e.metaKey;
      if (ctrl && e.shiftKey && e.code === "KeyC") return copy(), false;
      if (ctrl && e.shiftKey && e.code === "KeyV") return paste(), false;
      // Ctrl+C copies when something is selected (as in Windows Terminal) and interrupts otherwise; Cmd+C / Cmd+V on a Mac.
      if (e.ctrlKey && !e.shiftKey && e.key === "c" && t.hasSelection()) return copy(), t.clearSelection(), false;
      if (e.metaKey && e.key === "c") return copy(), false;
      if (e.metaKey && e.key === "v") return paste(), false;
      const zoom = zoomStep(e);
      if (zoom !== null) return zoomText(zoom), false;
      if (commandFor(useStore.getState().settings, e) === "toggleTerminal") return useTerminal.getState().toggle(), false;
      return true;
    });

    const refit = () => {
      if (el.offsetWidth > 0 && el.offsetHeight > 0) {
        try {
          f.fit();
        } catch {
          /* hidden */
        }
      }
    };
    // Ctrl + scroll changes the text size, as it does in the folder view.
    let wheelCarry = 0;
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey) return;
      e.preventDefault();
      e.stopPropagation();
      const r = wheelSteps(wheelCarry, e.deltaY);
      wheelCarry = r.carry;
      if (r.steps) zoomText(r.steps > 0 ? 1 : -1);
    };
    el.addEventListener("wheel", onWheel, { passive: false, capture: true });
    const ro = new ResizeObserver(refit);
    ro.observe(el);
    return () => {
      live = false;
      clearTimeout(titleTimer);
      ro.disconnect();
      el.removeEventListener("wheel", onWheel, { capture: true });
      unlisten.forEach((u) => u());
      t.dispose();
      term.current = null;
      fit.current = null;
    };
    // The screen is made again only for another shell.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pane.sid]);

  // Look and feel follow the settings and the theme.
  useEffect(() => {
    const t = term.current;
    if (!t) return;
    t.options.fontSize = adv<number>(settings, "terminal.fontSize");
    t.options.fontFamily = terminalFont(adv<string>(settings, "terminal.fontFamily"));
    t.options.cursorStyle = adv<string>(settings, "terminal.cursor") as "block" | "bar" | "underline";
    t.options.cursorBlink = adv<boolean>(settings, "terminal.cursorBlink");
    t.options.scrollback = adv<number>(settings, "terminal.scrollback");
    t.options.theme = themeColors();
    try {
      fit.current?.fit();
    } catch {
      /* hidden */
    }
  }, [settings, theme]);

  useEffect(() => {
    if (clear.paneId === pane.id) term.current?.clear();
  }, [clear.tick]); // eslint-disable-line react-hooks/exhaustive-deps

  // Fit and take the keyboard when this pane is the one in view.
  useEffect(() => {
    if (!visible) return;
    const t = setTimeout(() => {
      try {
        fit.current?.fit();
      } catch {
        /* hidden */
      }
      if (focused) term.current?.focus();
    }, 30);
    return () => clearTimeout(t);
  }, [visible, focused]);

  const showMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const t = term.current;
    const items: MenuItem[] = [
      { id: "copy", label: "Copy", shortcut: "Ctrl+Shift+C", disabled: !t?.hasSelection(), run: () => t && void navigator.clipboard.writeText(t.getSelection()) },
      { id: "paste", label: "Paste", shortcut: "Ctrl+Shift+V", run: () => void navigator.clipboard.readText().then((x) => t?.paste(x)).catch(() => {}) },
      { id: "all", label: "Select all", run: () => t?.selectAll() },
      { id: "clear", label: "Clear", separatorBefore: true, run: () => useTerminal.getState().clearActive() },
    ];
    setMenu({ x: e.clientX, y: e.clientY, items });
  };

  return (
    <div className="term-pane" data-focused={focused || undefined} onMouseDown={() => activatePane(tab.id, pane.id)} onContextMenu={showMenu}>
      <div ref={host} className="term-screen" />
      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
    </div>
  );
}

export function TerminalPanel({ dock }: { dock: string }) {
  const { tabs, active, ready, error, info } = useTerminal();
  const [layout, setLayout] = useLayout();
  const [menu, setMenu] = useState<MenuState | null>(null);
  const initOnce = useRef(false);
  useEffect(() => {
    if (initOnce.current) return;
    initOnce.current = true;
    void useTerminal.getState().init();
  }, []);
  // A shell ended or was added elsewhere.
  useEffect(() => {
    let off: (() => void) | undefined;
    void on("term-sessions", () => void useTerminal.getState().refresh()).then((u) => (off = u));
    return () => off?.();
  }, []);

  const t = useTerminal.getState();
  const current = tabs.find((x) => x.id === active) ?? tabs[0];
  const sideBySide = dock !== "left" && dock !== "right";

  const openAdmin = async () => {
    // On Windows a window cannot hand administrator rights to a terminal inside it; the app itself would have to run as administrator.
    if (info?.os === "windows" && !info.appElevated) {
      const r = await ask({
        title: "Terminal as administrator",
        message:
          "A terminal inside this window has the same rights as the app. To get administrator rights here, start the app as administrator (right-click it, Run as administrator). Or open a separate administrator terminal window now.",
        buttons: [
          { id: "window", label: "Open an administrator window", primary: true },
          { id: "cancel", label: "Cancel" },
        ],
      });
      if (r?.button === "window") void api.openTerminal(startFolder(), "admin").catch((e) => useStore.getState().showToast(String(e)));
      return;
    }
    await t.newTab({ elevated: true });
  };

  const newMenu = (e: React.MouseEvent) => {
    const r = e.currentTarget.getBoundingClientRect();
    setMenu({
      x: r.left,
      y: dock === "top" ? r.bottom : r.top - 4,
      items: [
        { id: "new", label: "New terminal", run: () => void t.newTab() },
        { id: "admin", label: info?.os === "windows" ? "New terminal as administrator…" : "New terminal as administrator (sudo)", run: () => void openAdmin() },
        { id: "split", label: "Split terminal", separatorBefore: true, run: () => void t.split() },
      ],
    });
  };

  const positionMenu = (e: React.MouseEvent) => {
    const r = e.currentTarget.getBoundingClientRect();
    const info2 = PANELS.find((p) => p.id === "terminal")!;
    setMenu({
      x: r.right - 160,
      y: dock === "top" ? r.bottom : r.top - 4,
      items: info2.docks.map((d: Dock) => ({
        id: `dock-${d}`,
        label: `Move to: ${DOCK_LABEL[d]}`,
        checked: dock === d,
        run: () => setLayout(moveTo(layout, "terminal", d, d === "float" ? { x: 140, y: 140 } : undefined)),
      })),
    });
  };

  const activePane = current?.panes.find((p) => p.id === current.active) ?? current?.panes[0];
  const label = (p: Pane) => p.title || p.cwd.split(/[\\/]/).filter(Boolean).pop() || "Terminal";

  return (
    <section className="term" aria-label="Terminal" data-dock={dock} onKeyDown={(e) => e.stopPropagation()}>
      <div className="term-bar">
        <div className="term-tabs" role="tablist" aria-label="Terminals">
          {tabs.map((tab) => {
            const p = tab.panes.find((x) => x.id === tab.active) ?? tab.panes[0];
            return (
              <div
                key={tab.id}
                role="tab"
                aria-selected={tab.id === current?.id}
                tabIndex={0}
                className="term-tab"
                data-active={tab.id === current?.id || undefined}
                data-ended={p.exited !== null || undefined}
                onClick={() => t.activate(tab.id)}
                onAuxClick={(e) => e.button === 1 && t.closeTab(tab.id)}
                onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && t.activate(tab.id)}
                title={`${label(p)}\n${p.cwd}${p.elevated ? "\nRunning as administrator" : ""}`}
              >
                <Icon name={p.elevated ? "shield" : "terminal"} size={13} />
                <span className="term-tab-name">{label(p)}</span>
                {tab.panes.length > 1 && <span className="term-tab-count">{tab.panes.length}</span>}
                <button className="term-tab-x" aria-label={`Close ${label(p)}`} onClick={(e) => (e.stopPropagation(), t.closeTab(tab.id))}>
                  <Icon name="close" size={10} />
                </button>
              </div>
            );
          })}
        </div>
        <button className="icon-button" aria-label="New terminal" title="New terminal" onClick={() => void t.newTab()}>
          <Icon name="plus" size={14} />
        </button>
        <button className="icon-button" aria-label="More ways to open a terminal" title="Administrator terminal, split…" onClick={newMenu}>
          <Icon name="expand" size={12} />
        </button>
        <span className="grow" />
        <button className="icon-button" aria-label="Split terminal" title="Split terminal" disabled={!current} onClick={() => void t.split()}>
          <Icon name="split" size={14} />
        </button>
        <button className="icon-button" aria-label="Clear terminal" title="Clear (what is on screen and the kept output)" disabled={!current} onClick={() => t.clearActive()}>
          <Icon name="broom" size={14} />
        </button>
        <button
          className="icon-button"
          aria-label="Kill terminal"
          title="Close this terminal and end its shell"
          disabled={!current}
          onClick={() => activePane && (current!.panes.length > 1 ? t.closePane(current!.id, activePane.id) : t.closeTab(current!.id))}
        >
          <Icon name="trash" size={14} />
        </button>
        <button className="icon-button" aria-label="Open in a terminal window" title="Open in the system's terminal app (it keeps running here too)" disabled={!activePane?.sid} onClick={() => activePane?.sid && void api.termPopout(activePane.sid).catch((e) => useStore.getState().showToast(String(e)))}>
          <Icon name="popout" size={14} />
        </button>
        <button className="icon-button" aria-label="Terminal position" title="Move the terminal" onClick={positionMenu}>
          <Icon name="settings" size={13} />
        </button>
        <button className="icon-button" aria-label="Hide terminal" title="Hide (the shells keep running)" onClick={() => t.setOpen(false)}>
          <Icon name="close" size={14} />
        </button>
      </div>
      <div className="term-body" data-side={sideBySide ? "row" : "column"}>
        {!ready && <div className="loading-bar" />}
        {ready && error && (
          <div className="empty-state" data-error>
            <strong>The terminal could not start</strong>
            <span>{error}</span>
            <button className="primary" onClick={() => (useTerminal.setState({ ready: false, error: null }), void useTerminal.getState().init())}>Try again</button>
          </div>
        )}
        {tabs.map((tab) => (
          <div key={tab.id} className="term-group" hidden={tab.id !== current?.id} data-side={sideBySide ? "row" : "column"}>
            {tab.panes.map((p) => (
              <TermPane key={p.id} tab={tab} pane={p} visible={tab.id === current?.id} focused={tab.id === current?.id && tab.active === p.id} />
            ))}
          </div>
        ))}
        {ready && !error && tabs.length === 0 && (
          <div className="empty-state">
            <strong>No terminals open</strong>
            <button className="primary" onClick={() => void t.newTab()}>New terminal</button>
          </div>
        )}
      </div>
      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
    </section>
  );
}
