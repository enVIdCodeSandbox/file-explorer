// Floating UI: modal dialogs, toasts and the in-app Properties window.

import { forgetOpeners } from "../util/opener";
import { useEffect, useRef, useState } from "react";
import { useDragView } from "../dnd";
import { api, type OpenApp, type OpenChoices } from "../api";
import { useStore, type DialogRequest } from "../store";
import { formatDate, formatSize } from "../util/format";
import { looksLikeError } from "../util/errorText";
import { Thumb } from "./Thumb";
import { adv } from "../settings/advanced";
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
  const [secret, setSecret] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    setInput(dialog?.input ?? "");
    setSelected(dialog?.select?.options[0]?.value ?? "");
    setSecret("");
    if (dialog?.input != null) setTimeout(() => inputRef.current?.select(), 0);
  }, [dialog]);

  if (!dialog) return null;
  const primary = dialog.buttons.find((b) => b.primary) ?? dialog.buttons[0];
  const choose = (button: string) => dialog.resolve({ button, input, selected, secret });

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
          <label className="dialog-select" data-stacked={dialog.inputLabel ? true : undefined}>
            {dialog.inputLabel && <span className="muted">{dialog.inputLabel}</span>}
            <input ref={inputRef} className="text-input" value={input} autoFocus autoComplete="off" onChange={(e) => setInput(e.target.value)} />
          </label>
        )}
        {dialog.secret && (
          <label className="dialog-select" data-stacked>
            <span className="muted">{dialog.secret.label}</span>
            <input className="text-input" type="password" value={secret} autoFocus={dialog.input == null} autoComplete="off" onChange={(e) => setSecret(e.target.value)} />
          </label>
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
  // A message about something that went wrong opens the activity log when clicked (what happened, and for how long).
  const problem = looksLikeError(toast);
  return (
    <div
      className="toast"
      role="status"
      data-problem={problem || undefined}
      title={problem ? "Click to see the activity log" : "Click to dismiss"}
      onClick={() => (problem ? useStore.getState().openActivityLog() : useStore.setState({ toast: null }))}
    >
      {toast}
      {problem && <span className="toast-hint">Click for the activity log</span>}
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
  const [all, setAll] = useState<OpenApp[] | null>(null);
  const [query, setQuery] = useState("");
  const [offset, setOffset] = useState({ x: 0, y: 0 });

  useEffect(() => {
    setChoices(null);
    setPicked(null);
    setError(null);
    setAll(null);
    setQuery("");
    setOffset({ x: 0, y: 0 });
    if (!path) return;
    let alive = true;
    api.openChoices(path).then((c) => {
      if (!alive) return;
      setChoices(c);
      setPicked(c.default?.id ?? c.suggested[0]?.id ?? null);
    }).catch((e) => alive && setError(String(e)));
    api.installedApps(path).then((l) => {
      if (alive) setAll(l);
    }).catch(() => {
      if (alive) setAll([]);
    });
    return () => { alive = false; };
  }, [path]);

  const escCloses = useStore((st) => adv<boolean>(st.settings, "files.openWithEsc"));
  const rememberOpenWith = useStore((st) => adv<boolean>(st.settings, "files.rememberOpenWith"));
  const open = !!request && !!path && !picking;
  // Esc closes the window even when it has only just opened and the keyboard is still in the file list behind it,
  // unless a text box in it is being typed into (Esc then clears the search first).
  useEffect(() => {
    if (!open || !escCloses) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      const el = document.activeElement as HTMLElement | null;
      if (el instanceof HTMLInputElement && el.value && el.closest(".openwith-dialog")) return void setQuery("");
      e.preventDefault();
      e.stopPropagation();
      useStore.getState().closeChooser();
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [open, escCloses]);

  // While a program is being chosen in the file view, the dialog steps aside.
  if (!request || !path || picking) return null;
  const close = () => useStore.getState().closeChooser();
  const name = baseName(path);
  const label = choices?.extension ? `${choices.extension}${choices.contentKind ? ` (${choices.contentKind})` : ""} file` : "file";

  const notifySaved = (id: string) => {
    forgetOpeners();
    const appName = [choices?.default, ...(choices?.suggested ?? []), ...(choices?.general ?? []), ...(all ?? [])].find((a) => a?.id === id)?.name ?? (baseName(id) || "that program");
    useStore.getState().showToast(
      info?.os === "windows"
        ? `${label}s now open with ${appName} in this app. Windows' own setting is unchanged (Settings → Apps → Default apps).`
        : `${label}s now open with ${appName}.`,
    );
  };

  const run = async (always: boolean) => {
    if (!picked) return;
    setBusy(true);
    try {
      await api.openWithApp(path, picked, always);
      if (always) notifySaved(picked);
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
      try {
        await api.openWithApp(path, chosen, rememberOpenWith);
        if (rememberOpenWith) notifySaved(chosen);
        close();
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(false);
      }
    }
  };

  const row = (a: OpenApp) => (
    <button key={a.id} className="openwith-row" data-picked={picked === a.id || undefined} onClick={() => setPicked(a.id)} onDoubleClick={() => void run(rememberOpenWith)} title={a.id}>
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
  const needle = query.trim().toLowerCase();
  const filterList = (list: OpenApp[]) => (!needle ? list : list.filter((a) => a.name.toLowerCase().includes(needle)));
  const topSeenIds = new Set(
    [choices?.default, ...(choices?.suggested ?? []), ...(choices?.general ?? [])]
      .filter((a): a is OpenApp => !!a)
      .map((a) => a.id.toLowerCase()),
  );
  const topSeenNames = new Set(
    [choices?.default, ...(choices?.suggested ?? []), ...(choices?.general ?? [])]
      .filter((a): a is OpenApp => !!a)
      .map((a) => a.name.toLowerCase()),
  );
  const defaultList = filterList(choices?.default ? [choices.default] : []);
  const suggestedList = filterList(choices?.suggested ?? []);
  const generalList = filterList(choices?.general ?? []);
  const restApps = filterList(
    (all ?? []).filter((a) => !topSeenIds.has(a.id.toLowerCase()) && !topSeenNames.has(a.name.toLowerCase())),
  );
  const anyVisible = defaultList.length + suggestedList.length + generalList.length + restApps.length > 0;

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
        <h3>{`Choose an app to open this ${label}`}</h3>
        <p className="openwith-why">
          <strong title={path}>{name}</strong>
          {request.reason ? ` — ${request.reason}` : ""}
        </p>
        <div className="openwith-search">
          <input className="text-input" placeholder="Search apps" aria-label="Search apps" value={query} onChange={(e) => setQuery(e.target.value)} spellCheck={false} />
        </div>
        <div className="openwith-list">
          {!choices && all === null && !error && <div className="muted" style={{ padding: 12 }}>Looking for apps…</div>}
          {group("Default app", defaultList)}
          {group("Suggested for this file", suggestedList)}
          {group("Editors and terminals", generalList)}
          {group("Other apps on this computer", restApps)}
          {all === null && !error && <div className="muted" style={{ padding: 12 }}>Loading other installed apps…</div>}
          {choices && all !== null && !anyVisible && (
            <div className="muted" style={{ padding: 12 }}>
              {needle ? "No app has that name." : "No apps were found for this file. Use “Find a program…”."}
            </div>
          )}
        </div>
        {error && <div className="openwith-error" role="alert">{error}</div>}
        <div className="openwith-footer">
          <button className="secondary" onClick={() => void browse()} disabled={busy}>Find a program…</button>
          <span className="grow" />
          <button className="secondary" disabled={!picked || busy} onClick={() => void run(false)} title="Open only this file once without changing the default">Just once</button>
          <button className="primary" disabled={!picked || busy} onClick={() => void run(true)} title="Open this file and remember this app for this file type and its supported media types">Always open</button>
        </div>
      </div>
    </div>
  );
}
