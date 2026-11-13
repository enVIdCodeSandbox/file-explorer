// Floating UI: modal dialogs, toasts and the in-app Properties window.

import { useEffect, useRef, useState } from "react";
import { useDragView } from "../dnd";
import { api, type OpenApp, type OpenChoices } from "../api";
import { useStore } from "../store";
import { baseName } from "../util/path";
import { pickFile } from "../util/pickFile";
import { DetailsPane } from "./DetailsPane";
import { Icon } from "./Icons";

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
        {dialog.message && <p className="dialog-message">{dialog.message}</p>}
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

  useEffect(() => {
    setChoices(null);
    setPicked(null);
    setError(null);
    if (!path) return;
    let alive = true;
    api.openChoices(path).then((c) => {
      if (!alive) return;
      setChoices(c);
      setPicked(c.default?.id ?? null);
    }).catch((e) => alive && setError(String(e)));
    return () => { alive = false; };
  }, [path]);

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
    <button key={a.id} className="openwith-row" data-picked={picked === a.id || undefined} onClick={() => setPicked(a.id)} onDoubleClick={() => void run(false)}>
      {a.iconUrl ? <img src={a.iconUrl} width={32} height={32} alt="" draggable={false} /> : <span className="openwith-icon"><Icon name="newItem" size={22} /></span>}
      <span className="openwith-name">{a.name}</span>
    </button>
  );
  const group = (title: string, list: OpenApp[]) => list.length > 0 && (
    <section key={title}>
      <h4>{title}</h4>
      {list.map(row)}
    </section>
  );

  return (
    <div className="modal-backdrop" onMouseDown={close}>
      <div className="dialog openwith-dialog" role="dialog" aria-modal="true" aria-label="Open with" onMouseDown={(e) => e.stopPropagation()} onKeyDown={(e) => (e.key === "Escape" && close(), e.stopPropagation())}>
        <h3>Choose an app to open this {label}</h3>
        <p className="openwith-why">
          <strong title={path}>{name}</strong>
          {request.reason ? ` — ${request.reason}` : ""}
        </p>
        <div className="openwith-list">
          {!choices && !error && <div className="muted" style={{ padding: 12 }}>Looking for apps…</div>}
          {choices && (
            <>
              {group("Default app", choices.default ? [choices.default] : [])}
              {group("Suggested apps", choices.suggested)}
              {group("More options", choices.others)}
              {!choices.default && !choices.suggested.length && !choices.others.length && <div className="muted" style={{ padding: 12 }}>No apps were found on this computer.</div>}
            </>
          )}
        </div>
        {error && <div className="openwith-error" role="alert">{error}</div>}
        <div className="openwith-footer">
          <button className="secondary" onClick={() => void browse()} disabled={busy}>Find a program…</button>
          <span className="grow" />
          <button disabled={!picked || busy} onClick={() => void run(true)} title="Open this kind of file with this app from now on">Always</button>
          <button className="primary" disabled={!picked || busy} onClick={() => void run(false)}>Just once</button>
        </div>
      </div>
    </div>
  );
}
