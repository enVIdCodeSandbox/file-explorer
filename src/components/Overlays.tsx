// Floating UI: modal dialogs, toasts and the in-app Properties window.

import { useEffect, useRef, useState } from "react";
import { useDragView } from "../dnd";
import { useStore } from "../store";
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
