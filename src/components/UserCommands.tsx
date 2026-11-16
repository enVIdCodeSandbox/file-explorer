// The button bar and the Commands menu (user-defined commands, Total Commander style) and the command line.

import { useRef, useState } from "react";
import { api, type UserCommand } from "../api";
import { activeLocation, useStore } from "../store";
import { adv } from "../settings/advanced";
import { isArchiveLoc, isRemote } from "../util/path";
import { buildRun, insertAtCaret, joinPath, type CommandContext } from "../util/userCommands";
import { useTerminal } from "../terminal/state";
import { ContextMenu, type MenuState } from "./ContextMenu";

const NONE: UserCommand[] = [];

/** Where things are right now: the folder, the item under the cursor and the selection. */
export function currentContext(): CommandContext | null {
  const s = useStore.getState();
  const folder = activeLocation();
  if (!folder || isRemote(folder) || isArchiveLoc(folder)) return null;
  const inFolder = s.selection.filter((p) => p.startsWith(folder));
  const names = inFolder.map((p) => p.slice(folder.length).replace(/^[\\/]/, "")).filter((n) => n && !/[\\/]/.test(n));
  return { folder, cursor: names[0] ? { name: names[0], isDir: false } : null, selected: names };
}

const toB64 = (text: string) => {
  const bytes = new TextEncoder().encode(text);
  let bin = "";
  bytes.forEach((b) => (bin += String.fromCharCode(b)));
  return btoa(bin);
};

/** Type a line into a new terminal tab that starts in `cwd`. */
export async function runInTerminal(line: string, cwd: string) {
  const t = useTerminal.getState();
  await t.newTab({ cwd });
  const tab = useTerminal.getState().tabs.find((x) => x.id === useTerminal.getState().active);
  const pane = tab?.panes.find((p) => p.id === tab.active) ?? tab?.panes[0];
  if (pane?.sid) await api.termInput(pane.sid, toB64(line + "\r"));
}

export async function runUserCommand(cmd: UserCommand) {
  const ctx = currentContext();
  const toast = useStore.getState().showToast;
  if (!ctx) {
    toast("User commands run in folders on this computer.");
    return;
  }
  const run = buildRun(cmd, ctx);
  try {
    if (cmd.runIn === "terminal") await runInTerminal(run.line, run.cwd);
    else await api.runProgram(run.program, run.args, run.cwd);
  } catch (e) {
    toast(String(e).replace(/^Error: /, ""));
  }
}

export function ButtonBar() {
  const commands = useStore((s) => s.settings?.userCommands ?? NONE);
  const show = useStore((s) => (s.settings ? adv<boolean>(s.settings, "ui.buttonBar") : true));
  const [menu, setMenu] = useState<MenuState | null>(null);
  const buttons = commands.filter((c) => c.onButtonBar);
  const inMenu = commands.filter((c) => c.inMenu);
  if (!show || (!buttons.length && !inMenu.length)) return null;
  return (
    <div className="button-bar" role="toolbar" aria-label="Button bar">
      {inMenu.length > 0 && (
        <button
          className="button-bar-item"
          aria-haspopup="menu"
          onClick={(e) => {
            const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
            setMenu({ x: r.left, y: r.bottom, items: inMenu.map((c) => ({ id: `uc-${c.id}`, label: c.label || c.program, icon: undefined, run: () => void runUserCommand(c) })) });
          }}
        >
          Commands ▾
        </button>
      )}
      {buttons.map((c) => (
        <button key={c.id} className="button-bar-item" title={`${c.label}\n${c.program} ${c.params}`.trim()} aria-label={c.label || c.program} onClick={() => void runUserCommand(c)}>
          <span className="button-bar-icon">{c.icon || (c.label || c.program).slice(0, 1).toUpperCase()}</span>
          {c.label && <span>{c.label}</span>}
        </button>
      ))}
      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
    </div>
  );
}

const history: string[] = (() => {
  try {
    return JSON.parse(localStorage.getItem("fast.cmdline") ?? "[]") as string[];
  } catch {
    return [];
  }
})();

/** The command line under the file list: Enter runs it in a new terminal tab, started in the folder you are viewing. */
export function CommandLine() {
  const on = useStore((s) => (s.settings ? adv<boolean>(s.settings, "ui.commandLine") : false));
  const folder = useStore(() => activeLocation());
  const [text, setText] = useState("");
  const [hist, setHist] = useState(-1);
  const input = useRef<HTMLInputElement>(null);
  if (!on || !folder || isRemote(folder) || isArchiveLoc(folder)) return null;

  const run = async () => {
    const line = text.trim();
    if (!line) return;
    if (history[0] !== line) history.unshift(line);
    history.length = Math.min(history.length, 100);
    try {
      localStorage.setItem("fast.cmdline", JSON.stringify(history));
    } catch {
      /* not saved */
    }
    setText("");
    setHist(-1);
    try {
      await runInTerminal(line, folder);
    } catch (e) {
      useStore.getState().showToast(String(e).replace(/^Error: /, ""));
    }
  };

  const insert = (what: string) => {
    const el = input.current;
    const r = insertAtCaret(text, el?.selectionStart ?? text.length, what);
    setText(r.text);
    requestAnimationFrame(() => el?.setSelectionRange(r.caret, r.caret));
  };

  return (
    <div className="command-line">
      <span className="command-line-prompt" title={folder}>{folder}&gt;</span>
      <input
        ref={input}
        className="command-line-input"
        aria-label="Command line"
        placeholder="Type a command and press Enter. Ctrl+Enter adds the file name, Ctrl+Shift+Enter its path."
        value={text}
        spellCheck={false}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
            e.preventDefault();
            const ctx = currentContext();
            if (ctx?.cursor) insert(e.shiftKey ? joinPath(ctx.folder, ctx.cursor.name) : ctx.cursor.name);
          } else if (e.key === "Enter") void run();
          else if (e.key === "ArrowUp" && history.length) {
            e.preventDefault();
            const i = Math.min(history.length - 1, hist + 1);
            setHist(i);
            setText(history[i]);
          } else if (e.key === "ArrowDown") {
            e.preventDefault();
            const i = hist - 1;
            setHist(i);
            setText(i < 0 ? "" : history[i]);
          } else if (e.key === "Escape") {
            (e.target as HTMLElement).blur();
            document.querySelector<HTMLElement>(".file-scroller")?.focus();
          }
        }}
      />
    </div>
  );
}
