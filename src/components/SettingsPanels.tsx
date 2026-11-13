// The larger settings controls: theme gallery, imports, layout editor,
// storage, shortcuts, custom actions and the raw settings editor.

import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { api, type Asset, type AssetKind, type CacheEntry, type CustomAction, type HistoryInfo } from "../api";
import { KEY_COMMANDS, comboOf, keyFor } from "../settings/keys";
import { ask, useStore } from "../store";
import { formatCount, formatSize } from "../util/format";
import { DOCK_LABEL, PANELS, PRESETS, moveTo, nudge, panelsAt, type Dock } from "../util/layout";
import { THEMES } from "../util/themes";
import { FONTS } from "../useAppearance";
import { patchPanel, useLayout } from "./Layout";

// ---------------------------------------------------------------------------
// Themes
// ---------------------------------------------------------------------------

export function ThemeGallery() {
  const theme = useStore((s) => s.settings!.appearance.theme);
  const { updateSettings } = useStore.getState();
  const pick = (id: string) => updateSettings((s) => void (s.appearance.theme = id));
  return (
    <div className="theme-gallery" role="radiogroup" aria-label="Colour theme">
      <button className="theme-card" role="radio" aria-checked={theme === "system"} data-on={theme === "system" || undefined} onClick={() => pick("system")}>
        <span className="theme-swatch" style={{ background: "linear-gradient(135deg,#1c1c1c 50%,#eef1f5 50%)" }} />
        <strong>Match system</strong>
        <span className="muted">Light or dark with your OS</span>
      </button>
      {THEMES.map((t) => (
        <button key={t.id} className="theme-card" role="radio" aria-checked={theme === t.id} data-on={theme === t.id || undefined} onClick={() => pick(t.id)}>
          <span className="theme-swatch" style={{ background: t.swatch[0] }}>
            <span style={{ background: t.swatch[1] }} />
            <i style={{ background: t.swatch[2] }} />
            <b style={{ background: t.swatch[3] }} />
          </span>
          <strong>{t.name}</strong>
          <span className="muted">{t.dark ? "Dark" : "Light"}</span>
        </button>
      ))}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Imported assets
// ---------------------------------------------------------------------------

function useAssets(kind: AssetKind) {
  const [assets, setAssets] = useState<Asset[]>([]);
  const reload = () => void api.listAssets(kind).then(setAssets).catch(() => {});
  useEffect(reload, [kind]); // eslint-disable-line react-hooks/exhaustive-deps
  return { assets, reload };
}

const FILTERS: Record<AssetKind, { name: string; extensions: string[] }> = {
  font: { name: "Fonts", extensions: ["ttf", "otf", "woff", "woff2"] },
  css: { name: "Style sheets", extensions: ["css"] },
  image: { name: "Pictures", extensions: ["png", "jpg", "jpeg", "webp", "gif", "bmp"] },
};

export function FontPicker() {
  const font = useStore((s) => s.settings!.appearance.style.font);
  const { assets } = useAssets("font");
  const set = (v: string) => useStore.getState().updateSettings((s) => void (s.appearance.style.font = v));
  return (
    <select aria-label="Font" value={font} onChange={(e) => set(e.target.value)}>
      {FONTS.map((f) => (
        <option key={f.id} value={f.id}>
          {f.name}
        </option>
      ))}
      {assets.map((a) => (
        <option key={a.name} value={`imported:${a.name}`}>
          {a.name} (imported)
        </option>
      ))}
      {font.startsWith("imported:") && !assets.some((a) => `imported:${a.name}` === font) && <option value={font}>{font.slice(9)} (missing)</option>}
    </select>
  );
}

function AssetList({ kind, title, hint }: { kind: AssetKind; title: string; hint: string }) {
  const style = useStore((s) => s.settings!.appearance.style);
  const { updateSettings, showToast } = useStore.getState();
  const { assets, reload } = useAssets(kind);

  const add = async () => {
    try {
      const picked = await openDialog({ multiple: true, filters: [FILTERS[kind]], title: `Import ${title.toLowerCase()}` });
      const files = picked == null ? [] : Array.isArray(picked) ? picked : [picked];
      for (const f of files) await api.importAsset(kind, f);
      if (files.length) {
        reload();
        showToast(`Imported ${files.length} file${files.length === 1 ? "" : "s"}.`);
      }
    } catch (e) {
      showToast(String(e));
    }
  };

  const remove = async (name: string) => {
    await api.deleteAsset(kind, name).catch((e) => showToast(String(e)));
    updateSettings((s) => {
      s.appearance.style.enabledSheets = s.appearance.style.enabledSheets.filter((x) => x !== name);
      if (s.appearance.style.font === `imported:${name}`) s.appearance.style.font = "system";
      if (s.appearance.style.backgroundImage === name) {
        s.appearance.style.backgroundImage = "";
        if (s.appearance.style.background === "image") s.appearance.style.background = "gradient";
      }
    });
    reload();
  };

  return (
    <div className="asset-list">
      <div className="section-head">
        <h4 className="grow">{title}</h4>
        <button className="secondary" onClick={() => void add()}>
          Import…
        </button>
      </div>
      <p className="muted">{hint}</p>
      {assets.length === 0 && <p className="muted">Nothing imported yet.</p>}
      {assets.map((a) => (
        <div key={a.name} className="asset-row">
          <span className="grow file-name">{a.name}</span>
          <span className="muted">{formatSize(a.bytes)}</span>
          {kind === "css" && (
            <label className="inline-toggle">
              <input
                type="checkbox"
                checked={style.enabledSheets.includes(a.name)}
                onChange={(e) =>
                  updateSettings((s) => {
                    const list = s.appearance.style.enabledSheets.filter((x) => x !== a.name);
                    s.appearance.style.enabledSheets = e.target.checked ? [...list, a.name] : list;
                  })
                }
              />
              On
            </label>
          )}
          {kind === "font" && (
            <button className="secondary" onClick={() => updateSettings((s) => void (s.appearance.style.font = `imported:${a.name}`))}>
              {style.font === `imported:${a.name}` ? "In use" : "Use"}
            </button>
          )}
          {kind === "image" && (
            <button
              className="secondary"
              onClick={() =>
                updateSettings((s) => {
                  s.appearance.style.backgroundImage = a.name;
                  s.appearance.style.background = "image";
                })
              }
            >
              {style.background === "image" && style.backgroundImage === a.name ? "In use" : "Use as background"}
            </button>
          )}
          <button className="icon-button danger" aria-label={`Remove ${a.name}`} title="Remove" onClick={() => void remove(a.name)}>
            ✕
          </button>
        </div>
      ))}
    </div>
  );
}

export function ImportsManager() {
  return (
    <div className="imports">
      <AssetList kind="font" title="Fonts" hint="TrueType, OpenType or WOFF files." />
      <AssetList kind="css" title="Style sheets" hint="CSS files layered on top of the theme, in the order shown." />
      <AssetList kind="image" title="Background pictures" hint="PNG, JPEG, WebP, GIF or BMP." />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

export function LayoutEditor() {
  const [layout, setLayout] = useLayout();
  const chips = (d: Dock) =>
    panelsAt(layout, d).map((id) => (
      <span key={id} className="layout-chip">
        {PANELS.find((p) => p.id === id)!.label}
      </span>
    ));
  return (
    <div className="layout-editor">
      <div className="layout-presets">
        {PRESETS.map((p) => (
          <button key={p.id} className="secondary" title={p.description} onClick={() => setLayout(structuredClone(p.layout))}>
            {p.name}
          </button>
        ))}
      </div>

      <div className="layout-diagram" aria-label="Preview of the window layout">
        <div className="ld-top">{chips("top")}</div>
        <div className="ld-middle">
          <div className="ld-left">{chips("left")}</div>
          <div className="ld-center">
            Files
            <div className="ld-float">{chips("float")}</div>
          </div>
          <div className="ld-right">{chips("right")}</div>
        </div>
        <div className="ld-bottom">{chips("bottom")}</div>
      </div>
      <p className="muted">
        Pop-out: {panelsAt(layout, "popout").map((id) => PANELS.find((p) => p.id === id)!.label).join(", ") || "none"} · Hidden:{" "}
        {panelsAt(layout, "hidden").map((id) => PANELS.find((p) => p.id === id)!.label).join(", ") || "none"}
      </p>

      <table className="layout-table">
        <tbody>
          {PANELS.map((info) => {
            const st = layout[info.id];
            return (
              <tr key={info.id}>
                <th scope="row">{info.label}</th>
                <td>
                  <select aria-label={`${info.label} position`} value={st.dock} onChange={(e) => setLayout(moveTo(layout, info.id, e.target.value as Dock, e.target.value === "float" ? { x: 140, y: 140 } : undefined))}>
                    {info.docks.map((d) => (
                      <option key={d} value={d}>
                        {DOCK_LABEL[d]}
                      </option>
                    ))}
                  </select>
                </td>
                <td>
                  <button className="icon-button" aria-label={`Move ${info.label} earlier`} title="Earlier" onClick={() => setLayout(nudge(layout, info.id, -1))}>
                    ↑
                  </button>
                  <button className="icon-button" aria-label={`Move ${info.label} later`} title="Later" onClick={() => setLayout(nudge(layout, info.id, 1))}>
                    ↓
                  </button>
                </td>
                <td>
                  <label className="inline-toggle">
                    <input type="checkbox" checked={st.collapsed} disabled={info.id === "sidebar"} onChange={(e) => setLayout(patchPanel(layout, info.id, { collapsed: e.target.checked }))} />
                    Collapsed
                  </label>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>

      <div className="button-row">
        <button
          className="primary"
          onClick={() => {
            // The arranging happens on the main window, so a detached Settings
            // goes back there first and the button is pressed again.
            if (useStore.getState().isSecondary) void api.reattach("settings");
            else useStore.setState({ settingsOpen: false, layoutEdit: true });
          }}
        >
          Arrange on screen
        </button>
        <button className="secondary" onClick={() => setLayout(structuredClone(PRESETS[0].layout))}>
          Reset layout
        </button>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

export function StoragePanel() {
  const [rows, setRows] = useState<CacheEntry[] | null>(null);
  const [history, setHistory] = useState<HistoryInfo | null>(null);
  const { showToast } = useStore.getState();
  const reload = () => {
    void api.cacheUsage().then(setRows).catch((e) => showToast(String(e)));
    void api.historyInfo().then(setHistory).catch(() => {});
  };
  useEffect(reload, []); // eslint-disable-line react-hooks/exhaustive-deps

  const clear = async (row: CacheEntry | null) => {
    const what = row ? row.label.toLowerCase() : "every cache";
    const r = await ask({
      title: `Clear ${what}?`,
      message: row
        ? row.rebuildable
          ? "It is rebuilt automatically as you use the app."
          : "The app forgets what it learned from you here; it starts learning again from scratch."
        : "Removes all of the above. Settings, pins and tabs are kept.",
      buttons: [
        { id: "ok", label: "Clear", primary: true },
        { id: "cancel", label: "Cancel" },
      ],
    });
    if (r?.button !== "ok") return;
    await api.clearCache(row ? row.id : "all").catch((e) => showToast(String(e)));
    showToast("Cleared.");
    reload();
  };

  if (!rows) return <p className="muted">Measuring…</p>;
  const disk = rows.reduce((s, r) => s + r.diskBytes, 0);
  const memory = rows.reduce((s, r) => s + r.memoryBytes, 0);
  return (
    <div className="storage">
      <table className="storage-table">
        <thead>
          <tr>
            <th>Feature</th>
            <th>On disk</th>
            <th>In memory</th>
            <th>Items</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.id} data-cache={r.id}>
              <th scope="row">
                {r.label}
                <span className="muted">{r.description}</span>
              </th>
              <td>{r.diskBytes ? formatSize(r.diskBytes) : "—"}</td>
              <td>{r.memoryBytes ? formatSize(r.memoryBytes) : "—"}</td>
              <td>{r.items ? formatCount(r.items) : "—"}</td>
              <td>
                <button className="secondary" onClick={() => void clear(r)}>
                  Clear
                </button>
              </td>
            </tr>
          ))}
        </tbody>
        <tfoot>
          <tr>
            <th scope="row">Total</th>
            <td>{formatSize(disk)}</td>
            <td>{formatSize(memory)}</td>
            <td />
            <td>
              <button className="secondary" onClick={() => void clear(null)}>
                Clear all
              </button>
            </td>
          </tr>
        </tfoot>
      </table>
      {history && history.records > 0 && (
        <p className="muted">
          Transfer history: {formatCount(history.records)} {history.records === 1 ? "transfer" : "transfers"} across {formatCount(history.routes)} {history.routes === 1 ? "route" : "routes"} in {formatSize(history.bytes)}.
        </p>
      )}
      <div className="button-row">
        <button className="secondary" onClick={reload}>
          Refresh sizes
        </button>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Advanced extras: shortcuts, custom actions, raw settings, maintenance
// ---------------------------------------------------------------------------

export function KeysPanel() {
  const settings = useStore((s) => s.settings)!;
  const { updateSettings } = useStore.getState();
  const [recording, setRecording] = useState<string | null>(null);
  const set = (id: string, combo: string | null) =>
    updateSettings((s) => {
      const keys = { ...s.keys };
      const def = KEY_COMMANDS.find((k) => k.id === id)!.default;
      if (combo == null || combo === def) delete keys[id];
      else keys[id] = combo;
      s.keys = keys;
    });
  return (
    <div className="keys-panel">
      {KEY_COMMANDS.map((k) => {
        const current = keyFor(settings, k.id);
        const clash = KEY_COMMANDS.find((o) => o.id !== k.id && keyFor(settings, o.id).toLowerCase() === current.toLowerCase());
        return (
          <div key={k.id} className="setting">
            <div className="setting-text">
              <span className="setting-label">{k.label}</span>
              {clash && <span className="muted warn">Also used for “{clash.label}”.</span>}
            </div>
            <span className="key-controls">
              <button
                className="secondary key-capture"
                data-capture-key
                onClick={() => setRecording(k.id)}
                onBlur={() => setRecording((r) => (r === k.id ? null : r))}
                onKeyDown={(e) => {
                  if (recording !== k.id) return;
                  e.preventDefault();
                  e.stopPropagation();
                  if (e.key === "Escape") return setRecording(null);
                  const combo = comboOf(e.nativeEvent);
                  if (combo) {
                    set(k.id, combo);
                    setRecording(null);
                  }
                }}
              >
                {recording === k.id ? "Press the keys…" : current}
              </button>
              <button className="link-button" disabled={!settings.keys[k.id]} onClick={() => set(k.id, null)}>
                Reset
              </button>
            </span>
          </div>
        );
      })}
    </div>
  );
}

const blankAction = (): CustomAction => ({ id: Math.random().toString(36).slice(2, 9), label: "Open with…", program: "", args: "{path}", appliesTo: "any", extensions: "", allAtOnce: false });

export function CustomActionsPanel() {
  const actions = useStore((s) => s.settings!.customActions);
  const { updateSettings } = useStore.getState();
  const change = (id: string, patch: Partial<CustomAction>) => updateSettings((s) => void (s.customActions = s.customActions.map((a) => (a.id === id ? { ...a, ...patch } : a))));
  return (
    <div className="custom-actions">
      <p className="muted">Add your own programs to the right-click menu. Arguments can use {"{path}"} (each selected item) and {"{dir}"} (its folder). Programs are started directly, never through a shell.</p>
      {actions.map((a) => (
        <div key={a.id} className="action-card">
          <div className="action-grid">
            <label>
              <span className="muted">Menu text</span>
              <input className="text-input" value={a.label} onChange={(e) => change(a.id, { label: e.target.value })} />
            </label>
            <label>
              <span className="muted">Program</span>
              <input className="text-input" value={a.program} placeholder="code, notepad.exe, /usr/bin/vlc…" onChange={(e) => change(a.id, { program: e.target.value })} />
            </label>
            <label>
              <span className="muted">Arguments</span>
              <input className="text-input" value={a.args} onChange={(e) => change(a.id, { args: e.target.value })} />
            </label>
            <label>
              <span className="muted">Shows for</span>
              <select value={a.appliesTo} onChange={(e) => change(a.id, { appliesTo: e.target.value as CustomAction["appliesTo"] })}>
                <option value="any">Files and folders</option>
                <option value="file">Files only</option>
                <option value="folder">Folders only</option>
              </select>
            </label>
            <label>
              <span className="muted">Only these extensions</span>
              <input className="text-input" value={a.extensions} placeholder="png, jpg (empty = all)" onChange={(e) => change(a.id, { extensions: e.target.value })} />
            </label>
            <label className="inline-toggle">
              <input type="checkbox" checked={a.allAtOnce} onChange={(e) => change(a.id, { allAtOnce: e.target.checked })} />
              Run once with all selected items
            </label>
          </div>
          <button className="secondary" onClick={() => updateSettings((s) => void (s.customActions = s.customActions.filter((x) => x.id !== a.id)))}>
            Remove
          </button>
        </div>
      ))}
      <button className="secondary" onClick={() => updateSettings((s) => void (s.customActions = [...s.customActions, blankAction()]))}>
        Add an action
      </button>
    </div>
  );
}

export function MaintenancePanel() {
  const settings = useStore((s) => s.settings)!;
  const { showToast } = useStore.getState();
  const [raw, setRaw] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const apply = async () => {
    try {
      const next = await api.importSettings(raw ?? "");
      useStore.setState({ settings: next });
      setError(null);
      showToast("Settings applied.");
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="maintenance">
      <div className="button-row">
        <button className="secondary" onClick={() => void navigator.clipboard.writeText(JSON.stringify(settings, null, 2)).then(() => showToast("Settings copied."))}>
          Copy all settings
        </button>
        <button className="secondary" onClick={() => setRaw(raw == null ? JSON.stringify(settings, null, 2) : null)}>
          {raw == null ? "Edit settings as text…" : "Close the text editor"}
        </button>
        <button className="secondary" onClick={() => void api.configFolder().then((p) => { if (p) return api.openItem(p); }).catch((e) => showToast(String(e)))}>
          Open the settings folder
        </button>
        <button
          className="secondary"
          data-danger
          onClick={async () => {
            const r = await ask({
              title: "Reset every setting?",
              message: "All options return to their defaults, including theme, layout, shortcuts and custom actions. Files you imported stay in the settings folder.",
              buttons: [
                { id: "ok", label: "Reset everything", primary: true, danger: true },
                { id: "cancel", label: "Cancel" },
              ],
            });
            if (r?.button !== "ok") return;
            try {
              useStore.setState({ settings: await api.importSettings("{}") });
              showToast("Settings reset.");
            } catch (e) {
              showToast(String(e));
            }
          }}
        >
          Reset all settings
        </button>
      </div>
      {raw != null && (
        <>
          <textarea className="css-editor" spellCheck={false} aria-label="Settings as JSON" value={raw} onChange={(e) => setRaw(e.target.value)} />
          {error && <p className="muted warn">{error}</p>}
          <div className="button-row">
            <button className="primary" onClick={() => void apply()}>
              Apply
            </button>
          </div>
        </>
      )}
    </div>
  );
}
