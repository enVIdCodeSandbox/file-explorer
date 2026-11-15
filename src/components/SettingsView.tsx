// Settings: one searchable screen, divided into sections, generated from
// src/settings/schema.ts. Changes apply and save immediately.

import { isMobileNow } from "../util/device";
import { useT } from "../i18n";
import { About } from "./About";
import { Docs } from "./Docs";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "../api";
import { useEffect, useMemo, useRef, useState } from "react";
import { SECTIONS, SETTINGS, getSetting, setSetting, type SettingDef } from "../settings/schema";
import { useStore } from "../store";
import { adv } from "../settings/advanced";
import { MENU_CATALOG } from "./ContextMenu";
import { Icon } from "./Icons";
import { LocksPanel } from "./Locks";
import { UpdatesPanel } from "./UpdatesPanel";
import { PerfPresetsPanel, ProfilesPanel, ResourcesPanel, UiPresetsPanel } from "./PresetPanels";
import { CustomActionsPanel, DefaultFileManagerPanel, FontPicker, ImportsManager, KeysPanel, LayoutEditor, MaintenancePanel, StoragePanel, ThemeGallery } from "./SettingsPanels";

function ControlFor({ def }: { def: SettingDef }) {
  const t = useT();
  const settings = useStore((s) => s.settings)!;
  const { updateSettings } = useStore.getState();
  const value = getSetting(settings, def.key);
  const set = (v: unknown) =>
    updateSettings((s) => {
      setSetting(s, def.key, v);
      // A hand-made choice is kept when a performance preset is applied.
      if (def.key === "performance.backgroundMode") s.performance.backgroundLocked = true;
    });
  const c = def.control;

  // Text-like fields save when editing stops, not on every keystroke.
  const [draft, setDraft] = useState(String(value ?? ""));
  useEffect(() => setDraft(String(value ?? "")), [value]);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const setLater = (text: string, convert: (t: string) => unknown = (t) => t) => {
    setDraft(text);
    clearTimeout(timer.current);
    timer.current = setTimeout(() => set(convert(text)), 350);
  };

  switch (c.type) {
    case "toggle":
      return (
        <button
          role="switch"
          aria-checked={!!value}
          aria-label={def.label}
          className="switch"
          data-on={value ? true : undefined}
          onClick={() => set(!value)}
        >
          <span />
        </button>
      );
    case "select":
      return (
        <select aria-label={t(def.label)} value={String(value)} onChange={(e) => set(e.target.value)}>
          {c.options.map((o) => (
            <option key={o.value} value={o.value}>
              {t(o.label)}
            </option>
          ))}
        </select>
      );
    case "number":
      return (
        <span className="number-field">
          <input
            type="number"
            aria-label={def.label}
            min={c.min}
            max={c.max}
            step={c.step ?? 1}
            value={draft}
            onChange={(e) =>
              setLater(e.target.value, (t) => Math.max(c.min, Math.min(c.max, Math.round(Number(t) || 0))))
            }
          />
          {c.unit && <span className="muted">{c.unit}</span>}
        </span>
      );
    case "text":
      return (
        <input
          className="text-input"
          type={c.secret ? "password" : "text"}
          autoComplete="off"
          aria-label={def.label}
          value={draft}
          placeholder={c.placeholder}
          onChange={(e) => setLater(e.target.value)}
        />
      );
    case "color":
      return <input type="color" aria-label={def.label} value={String(value)} onChange={(e) => setLater(e.target.value)} />;
    case "css":
      return (
        <textarea
          className="css-editor"
          aria-label={def.label}
          spellCheck={false}
          value={draft}
          placeholder={":root {\n  --accent: #ff8a3d;\n  --radius: 4px;\n}\n\n.file-row[data-kind=\"folder\"] .file-name {\n  font-weight: 600;\n}"}
          onChange={(e) => setLater(e.target.value)}
        />
      );
    case "themes":
      return <ThemeGallery />;
    case "font":
      return <FontPicker />;
    case "imports":
      return <ImportsManager />;
    case "layout":
      return <LayoutEditor />;
    case "storage":
      return <StoragePanel />;
    case "perfPresets":
      return <PerfPresetsPanel />;
    case "uiPresets":
      return <UiPresetsPanel />;
    case "profiles":
      return <ProfilesPanel />;
    case "resources":
      return <ResourcesPanel />;
    case "locks":
      return <LocksPanel />;
    case "defaultFm":
      return <DefaultFileManagerPanel />;
    case "updates":
      return <UpdatesPanel />;
    case "menuItems": {
      const hidden = new Set(value as string[]);
      return (
        <div className="check-grid">
          {MENU_CATALOG.map((m) => (
            <label key={m.id}>
              <input
                type="checkbox"
                checked={!hidden.has(m.id)}
                onChange={(e) => {
                  const next = new Set(hidden);
                  e.target.checked ? next.delete(m.id) : next.add(m.id);
                  set([...next]);
                }}
              />
              {m.label}
            </label>
          ))}
        </div>
      );
    }
  }
}

/** Keep a dragged panel at least partly on screen. */
function clampOffset(x: number, y: number, w: number, h: number) {
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const maxX = vw / 2 + w / 2 - 80;
  const maxY = vh / 2 + h / 2 - 40;
  return { x: Math.max(-maxX, Math.min(maxX, x)), y: Math.max(-(vh / 2 - h / 2), Math.min(maxY, y)) };
}

/** Always at the bottom of Settings: save, discard, and whether changes save by themselves. */
function SaveBar() {
  const unsaved = useStore((s) => s.unsaved);
  const auto = useStore((s) => adv<boolean>(s.settings, "ui.autoSave"));
  const { saveSettings, discardSettings, updateSettings } = useStore.getState();
  return (
    <div className="save-bar" data-unsaved={unsaved || undefined}>
      <label className="save-auto">
        <input type="checkbox" checked={auto} onChange={(e) => updateSettings((s) => void (s.advanced["ui.autoSave"] = e.target.checked))} /> Save changes automatically
      </label>
      <span className="muted grow">{unsaved ? "You have changes that are applied but not saved." : auto ? "Every change is saved as you make it." : "Everything is saved."}</span>
      <button className="secondary" disabled={!unsaved} onClick={discardSettings}>Discard</button>
      <button className="primary" onClick={saveSettings}>Save</button>
    </div>
  );
}

export function SettingsView({ detached = false }: { detached?: boolean }) {
  const t = useT();
  const os = useStore((s) => s.info?.os);
  const [query, setQuery] = useState(() => {
    // Opened from somewhere that knows what to look for (the Tasks window's Settings button).
    const q = useStore.getState().settingsQuery;
    if (q) useStore.setState({ settingsQuery: "" });
    return q;
  });
  const [section, setSection] = useState<string>(SECTIONS[0]);
  const pos = useStore((s) => s.settingsPos);
  const panel = useRef<HTMLDivElement>(null);
  const picking = useStore((s) => !detached && !!s.picker);
  const close = () => (detached ? void getCurrentWindow().close() : useStore.setState({ settingsOpen: false }));

  // Drag the panel by its title. Where it was left is kept for next time.
  const startDrag = (e: React.PointerEvent) => {
    if (detached || e.button !== 0 || !panel.current) return;
    const start = { px: e.clientX, py: e.clientY, x: pos.x, y: pos.y };
    const box = panel.current.getBoundingClientRect();
    const move = (ev: PointerEvent) =>
      useStore.setState({ settingsPos: clampOffset(start.x + ev.clientX - start.px, start.y + ev.clientY - start.py, box.width, box.height) });
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    e.preventDefault();
  };
  const detach = () => {
    void api
      .openSettingsWindow()
      .then(() => useStore.setState({ settingsOpen: false }))
      .catch((err) => useStore.getState().showToast(String(err)));
  };

  const available = useMemo(() => SETTINGS.filter((d) => !d.os || (os && d.os.includes(os))), [os]);
  const q = query.trim().toLowerCase();
  const matches = useMemo(
    () =>
      q
        ? available.filter((d) =>
            `${d.label} ${d.description ?? ""} ${d.section} ${d.keywords ?? ""}`.toLowerCase().includes(q),
          )
        : available.filter((d) => d.section === section),
    [available, q, section],
  );

  const bySection = SECTIONS.map((s) => [s, matches.filter((d) => d.section === s)] as const).filter(([, l]) => l.length);

  const dialog = (
      <div
        ref={panel}
        className="settings"
        data-detached={detached || undefined}
        style={detached ? undefined : { transform: `translate(${pos.x}px, ${pos.y}px)` }}
        role="dialog"
        aria-modal="true"
        aria-label="Settings"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") close();
          e.stopPropagation();
        }}
      >
        <aside className="settings-nav">
          <h2
            className="settings-drag"
            data-draggable={!detached || undefined}
            title={detached ? undefined : "Drag to move. Double-click to put it back in the middle."}
            onPointerDown={startDrag}
            onDoubleClick={() => useStore.setState({ settingsPos: { x: 0, y: 0 } })}
          >
            Settings
          </h2>
          <div className="search-box">
            <Icon name="search" size={14} />
            <input autoFocus={!isMobileNow()} value={query} placeholder="Search settings" aria-label="Search settings" onChange={(e) => setQuery(e.target.value)} />
          </div>
          {SECTIONS.map((s) => (
            <button
              key={s}
              className="side-item"
              data-active={(!q && section === s) || undefined}
              onClick={() => {
                setQuery("");
                setSection(s);
              }}
            >
              {s}
            </button>
          ))}
        </aside>
        <div className="settings-body">
          <div className="settings-scroll">
          <div className="settings-close">
            {detached ? (
              <button className="secondary reattach" title="Put Settings back inside the main window" onClick={() => void api.reattach("settings").catch((err) => useStore.getState().showToast(String(err)))}>
                <Icon name="popin" size={13} /> Reattach to window
              </button>
            ) : (
              <button className="icon-button" aria-label="Open in its own window" title="Open in its own window: move it anywhere and resize it" onClick={detach}>
                <Icon name="popout" />
              </button>
            )}
            <button className="icon-button" aria-label="Close settings" onClick={close}>
              <Icon name="close" />
            </button>
          </div>
          {q && bySection.length === 0 && <p className="muted">No settings match "{query}".</p>}
          {bySection.map(([name, defs]) => (
            <section key={name}>
              <h3>{t(name)}</h3>
              {defs.map((d) => {
                const wide = ["css", "menuItems", "themes", "imports", "layout", "storage", "perfPresets", "uiPresets", "profiles", "resources", "locks"].includes(d.control.type);
                return (
                  <div key={d.key} className="setting" data-wide={wide || undefined} data-setting={d.key}>
                    <div className="setting-text">
                      <span className="setting-label">{t(d.label)}</span>
                      {d.description && <span className="muted">{t(d.description)}</span>}
                    </div>
                    <ControlFor def={d} />
                  </div>
                );
              })}
            </section>
          ))}
          {!q && section === "Advanced" && (
            <>
              <section>
                <h3>Keyboard shortcuts</h3>
                <KeysPanel />
              </section>
              <section>
                <h3>Your own right-click actions</h3>
                <CustomActionsPanel />
              </section>
              <section>
                <h3>Maintenance</h3>
                <MaintenancePanel />
              </section>
            </>
          )}
          {!q && section === "Documentation" && <Docs />}
          {!q && section === "About" && (
            <section>
              <h3>About</h3>
              <About />
            </section>
          )}
          </div>
          <SaveBar />
        </div>
      </div>
  );
  // Out of the way while a folder is chosen, but still mounted so what was typed and the scroll position are kept.
  return detached ? dialog : (
    <div className="modal-backdrop" onMouseDown={close} hidden={picking} style={picking ? { display: "none" } : undefined}>
      {dialog}
    </div>
  );
}
