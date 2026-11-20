// Floating UI: modal dialogs, toasts and the in-app Properties window.

import { forgetOpeners } from "../util/opener";
import { useEffect, useRef, useState } from "react";
import { useDragView } from "../dnd";
import { api, type OpenApp, type OpenChoices } from "../api";
import { useStore, type DialogRequest } from "../store";
import { formatDate, formatSize } from "../util/format";
import { Thumb } from "./Thumb";
import { baseName } from "../util/path";
import { pickFile } from "../util/pickFile";
import { DetailsPane } from "./DetailsPane";
import { Icon } from "./Icons";

/** Both versions of a file that exists on both sides: a picture (when there is one), size and date, and which is newer or bigger. */
function ConflictPreview({ c }: { c: NonNullable<DialogRequest["compare"]> }) {
  const side = (label: string, f: typeof c.here, other: typeof c.here) => {
    const newer = f.modified != null && other.modified != null && f.modified !== other.modified ? (f.modified > other.modified ? "newer" : "older") : null;
    const bigger = f.size !== other.size ? (f.size > other.size ? "larger" : "smaller") : null;
    return (
      <div className="conflict-side">
        <strong>{label}</strong>
        <div className="conflict-thumb"><Thumb path={f.path} name={c.name} isDir={false} modified={f.modified} size={160} iconSize={48} view="xl" /></div>
        <span>{formatSize(f.size)}{bigger && <em className="conflict-tag" data-kind={bigger}> {bigger}</em>}</span>
        <span className="muted">{f.modified ? formatDate(f.modified * 1000) : "unknown date"}{newer && <em className="conflict-tag" data-kind={newer}> {newer}</em>}</span>
      </div>
    );
  };
  return (
    <div className="conflict-preview">
      {side("Already here", c.here, c.incoming)}
      {side("Coming in", c.incoming, c.here)}
    </div>
  );
}

export function Dialog() {
  const dialog = useStore((s) => s.dialog);
  const [input, setInput] = useState("");
  const [selected, setSelected] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    setInput(dialog?.input ?? "");
    setSelected(dialog?.select?.options[0]?.value ?? "");
    if (dialog?.input != null) setTimeout(() => inputRef.current?.select(), 0);
  }, [dialog]);

  if (!dialog) return null;
  const primary = dialog.buttons.find((b) => b.primary) ?? dialog.buttons[0];
  const choose = (button: string) => dialog.resolve({ button, input, selected });

  return (
    <div className="modal-backdrop" onMouseDown={() => dialog.resolve(null)}>
      <div
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-label={dialog.title}
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") dialog.resolve(null);
          if (e.key === "Enter") choose(primary.id);
          e.stopPropagation();
        }}
      >
        <h3>{dialog.title}</h3>
        {dialog.compare && <ConflictPreview c={dialog.compare} />}
        {dialog.message && !dialog.compare && <p className="dialog-message">{dialog.message}</p>}
        {dialog.message && dialog.compare && <p className="dialog-message muted">{dialog.compare.here.path}</p>}
        {dialog.input != null && (
          <input ref={inputRef} className="text-input" value={input} autoFocus onChange={(e) => setInput(e.target.value)} />
        )}
        {dialog.select && (
          <label className="dialog-select">
            <span className="muted">{dialog.select.label}</span>
            <select value={selected} onChange={(e) => setSelected(e.target.value)}>
              {dialog.select.options.map((o) => (
                <option key={o.value} value={o.value}>{o.label}</option>
              ))}
            </select>
          </label>
        )}
        <div className="dialog-buttons">
          {dialog.buttons.map((b) => (
            <button
              key={b.id}
              className={b.primary ? "primary" : "secondary"}
              data-danger={b.danger || undefined}
              autoFocus={b.primary && dialog.input == null}
              onClick={() => choose(b.id)}
            >
              {b.label}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

export function Toast() {
  const toast = useStore((s) => s.toast);
  if (!toast) return null;
  return (
    <div className="toast" role="status" onClick={() => useStore.setState({ toast: null })}>
      {toast}
    </div>
  );
}

/** Our own Properties window, for systems without one (or when it is switched off). */
export function PropertiesDialog() {
  const paths = useStore((s) => s.properties);
  if (!paths) return null;
  const close = () => useStore.setState({ properties: null });
  return (
    <div className="modal-backdrop" onMouseDown={close}>
      <div
        className="dialog properties-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Properties"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => (e.key === "Escape" && close(), e.stopPropagation())}
      >
        <div className="dialog-title-row">
          <h3>Properties</h3>
          <button className="icon-button" aria-label="Close" onClick={close} autoFocus>
            <Icon name="close" />
          </button>
        </div>
        <DetailsPane paths={paths} embedded />
      </div>
    </div>
  );
}

/** The label that follows the pointer while something is being dragged. */
export function DragOverlay() {
  const view = useDragView((s) => s.view);
  if (!view) return null;
  return (
    <div className="drag-ghost" data-ok={view.ok || undefined} style={{ left: view.x + 14, top: view.y + 12 }}>
      <span className="drag-ghost-name">{view.label}</span>
      {view.count > 1 && <span className="drag-ghost-count">{view.count}</span>}
      {view.text && <span className="drag-ghost-text">{view.text}</span>}
    </div>
  );
}

/**
 * "Select an app to open this .zip file": the default, suggested and other
 * programs, with Always (make it the default) and Just once.
 */
/** Where programs usually live, so "Find a program" starts somewhere useful. */
function programsFolder(os: string | undefined): string | undefined {
  return os === "windows" ? "C:\\Program Files" : os === "macos" ? "/Applications" : "/usr/bin";
}

export function OpenWithDialog() {
  const request = useStore((s) => s.openWith);
  const [choices, setChoices] = useState<OpenChoices | null>(null);
  const [picked, setPicked] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const info = useStore((st) => st.info);
  const path = request?.path;
  const picking = useStore((st) => !!st.picker);
  const [view, setView] = useState<"main" | "all">("main");
  const [all, setAll] = useState<OpenApp[] | null>(null);
  const [query, setQuery] = useState("");
  const [offset, setOffset] = useState({ x: 0, y: 0 });

  useEffect(() => {
    setChoices(null);
    setPicked(null);
    setError(null);
    setView("main");
    setAll(null);
    setQuery("");
    setOffset({ x: 0, y: 0 });
    if (!path) return;
    let alive = true;
    api.openChoices(path).then((c) => {
      if (!alive) return;
      setChoices(c);
      setPicked(c.default?.id ?? null);
    }).catch((e) => alive && setError(String(e)));
    return () => { alive = false; };
  }, [path]);

  // The list of every installed app is looked up when it is first asked for.
  useEffect(() => {
    if (view !== "all" || all || !path) return;
    let alive = true;
    api.installedApps(path).then((l) => alive && setAll(l)).catch((e) => alive && (setError(String(e)), setAll([])));
    return () => { alive = false; };
  }, [view, all, path]);

  // While a program is being chosen in the file view, the dialog steps aside.
  if (!request || !path || picking) return null;
  const close = () => useStore.getState().closeChooser();
  const name = baseName(path);
  const label = choices?.extension ? `${choices.extension} file` : "file";

  const run = async (always: boolean) => {
    if (!picked) return;
    setBusy(true);
    try {
      await api.openWithApp(path, picked, always);
      if (always) forgetOpeners();
      close();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const browse = async () => {
    const chosen = await pickFile("Choose the program to open it with", programsFolder(info?.os));
    if (chosen) {
      setPicked(chosen);
      setBusy(true);
      try { await api.openWithApp(path, chosen, false); close(); } catch (e) { setError(String(e)); } finally { setBusy(false); }
    }
  };

  const row = (a: OpenApp) => (
    <button key={a.id} className="openwith-row" data-picked={picked === a.id || undefined} onClick={() => setPicked(a.id)} onDoubleClick={() => void run(false)} title={a.id}>
      {a.iconUrl ? <img src={a.iconUrl} width={32} height={32} alt="" draggable={false} /> : <span className="openwith-icon" aria-hidden="true">{(a.name.match(/[\p{L}\p{N}]/u)?.[0] ?? "?").toUpperCase()}</span>}
      <span className="openwith-name">{a.name}</span>
    </button>
  );
  const group = (title: string, list: OpenApp[]) => list.length > 0 && (
    <section key={title}>
      <h4>{title}</h4>
      {list.map(row)}
    </section>
  );
  const shown = (all ?? []).filter((a) => !query.trim() || a.name.toLowerCase().includes(query.trim().toLowerCase()));
  const nothing = choices && !choices.default && !choices.suggested.length && !choices.general.length;

  // The window is moved by its title bar, like any other, and has a close button of its own.
  const startMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0 || (e.target as HTMLElement).closest("button")) return;
    e.preventDefault();
    const el = e.currentTarget;
    el.setPointerCapture(e.pointerId);
    const [sx, sy, ox, oy] = [e.clientX, e.clientY, offset.x, offset.y];
    const box = el.parentElement!.getBoundingClientRect();
    // Keep the whole window on the screen.
    const limitX = Math.max(0, (window.innerWidth - box.width) / 2);
    const limitY = Math.max(0, (window.innerHeight - box.height) / 2);
    const move = (ev: PointerEvent) => setOffset({ x: Math.max(-limitX, Math.min(limitX, ox + ev.clientX - sx)), y: Math.max(-limitY, Math.min(limitY, oy + ev.clientY - sy)) });
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  };

  return (
    <div className="modal-backdrop">
      <div className="dialog openwith-dialog" role="dialog" aria-modal="true" aria-label="Open with" style={{ transform: `translate(${offset.x}px, ${offset.y}px)` }} onKeyDown={(e) => (e.key === "Escape" && close(), e.stopPropagation())}>
        <div className="openwith-titlebar" onPointerDown={startMove} onDoubleClick={() => setOffset({ x: 0, y: 0 })} title="Drag to move">
          <span className="openwith-titlebar-text">Open with</span>
          <button className="icon-button" aria-label="Close" title="Close (Esc)" onClick={close}><Icon name="close" size={14} /></button>
        </div>
        <h3>{view === "all" ? "All apps on this computer" : `Choose an app to open this ${label}`}</h3>
        <p className="openwith-why">
          <strong title={path}>{name}</strong>
          {request.reason ? ` — ${request.reason}` : ""}
        </p>
        {view === "all" && (
          <div className="openwith-search">
            <input className="text-input" autoFocus placeholder="Search apps" aria-label="Search apps" value={query} onChange={(e) => setQuery(e.target.value)} spellCheck={false} />
          </div>
        )}
        <div className="openwith-list">
          {view === "main" && !choices && !error && <div className="muted" style={{ padding: 12 }}>Looking for apps…</div>}
          {view === "main" && choices && (
            <>
              {group("Default app", choices.default ? [choices.default] : [])}
              {group("Suggested for this file", choices.suggested)}
              {group("Editors and terminals", choices.general)}
              {nothing && <div className="muted" style={{ padding: 12 }}>No apps were found for this file. Use “All apps on this computer” or “Find a program”.</div>}
            </>
          )}
          {view === "all" && !all && !error && <div className="muted" style={{ padding: 12 }}>Looking through the installed apps…</div>}
          {view === "all" && all && (shown.length ? <section>{shown.map(row)}</section> : <div className="muted" style={{ padding: 12 }}>{query ? "No app has that name." : "No apps were found."}</div>)}
        </div>
        {error && <div className="openwith-error" role="alert">{error}</div>}
        <div className="openwith-footer">
          {view === "main" ? <button className="secondary" onClick={() => setView("all")} disabled={busy}>All apps on this computer…</button> : <button className="secondary" onClick={() => setView("main")} disabled={busy}>Back</button>}
          <button className="secondary" onClick={() => void browse()} disabled={busy}>Find a program…</button>
          <span className="grow" />
          <button disabled={!picked || busy} onClick={() => void run(true)} title="Open this kind of file with this app from now on">Always</button>
          <button className="primary" disabled={!picked || busy} onClick={() => void run(false)}>Just once</button>
        </div>
      </div>
    </div>
  );
}
