// The larger settings controls: theme gallery, imports, layout editor,
// storage, shortcuts, custom actions and the raw settings editor.

import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { api, type CopyMethod, type MethodsReport, type DefaultFmStatus, type ForgetReport, type Asset, type AssetKind, type CacheEntry, type CustomAction, type UserCommand, type HistoryInfo, type CrashLog } from "../api";
import { KEY_COMMANDS, comboOf, keyFor } from "../settings/keys";
import { ask, useStore } from "../store";
import { formatCount, formatSize } from "../util/format";
import { ALL_METHODS, METHOD_NAME, METHOD_NOTE, jobGroup } from "../util/methods";
import { isMobileOs, openAppDefaults } from "../util/native";
import { pickFolder } from "../util/pickFile";
import { RULES_KEY, cacheRules } from "../util/cacheRules";
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
    <div className="theme-gallery" role="radiogroup" aria-label="Color theme">
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

/** What each kind of job is carried out with, right now and in general. */
const OTHER_OPERATIONS: { label: string; kind?: string; method: string }[] = [
  { label: "Move inside one drive", method: "Rename: nothing is copied" },
  { label: "Delete permanently", kind: "delete", method: "Direct removal, in parallel" },
  { label: "Move to the Recycle Bin / Trash", kind: "trash", method: "The operating system, one item at a time" },
  { label: "Compress to a zip", kind: "compress", method: "Built-in zip writer" },
  { label: "Unpack an archive", kind: "extract", method: "Built-in readers; 7-Zip or bsdtar for rar, cab, iso and the like" },
  { label: "Copy into or out of an archive", method: "Built-in archive reader and writer" },
  { label: "Servers and phones", method: "SFTP over SSH, FTP, WebDAV or MTP, by the kind of server" },
  { label: "Updates", method: "HTTPS download from GitHub" },
];

/** Which way every kind of job is being done, what can be used, and how each has done. */
export function TransferMethodsPanel() {
  const [report, setReport] = useState<MethodsReport | null>(null);
  const transfers = useStore((s) => s.transfers);
  const deletes = useStore((s) => s.deletes);
  const last = useStore((s) => s.lastMethods);
  const chosen = useStore((s) => s.settings?.transfer.method ?? "auto");
  useEffect(() => {
    void api.transferMethods().then(setReport).catch(() => {});
  }, [Object.keys(transfers).length, Object.keys(deletes).length]);
  const live = Object.values(transfers).filter((p) => p.phase === "running" || p.phase === "paused" || p.phase === "scanning");
  const others = Object.values(deletes).filter((p) => p.phase === "running" || p.phase === "scanning");
  const stat = (m: CopyMethod) => report?.stats.find((s) => s.method === m);
  const kindStat = (k?: string) => (k ? report?.jobs.find((j) => j.kind === k) : undefined);
  const lastEntries = Object.entries(last);
  return (
    <div className="methods-panel">
      <dl className="details-list transfer-facts">
        <dt>Copy method setting</dt>
        <dd>{chosen === "auto" ? "Automatic: learns which method is fastest for each pair of drives" : `Always ${METHOD_NAME[chosen as CopyMethod] ?? chosen}, where it can run`}</dd>
        <dt>In use now</dt>
        <dd>
          {live.length + others.length === 0 && <span className="muted">Nothing is running.</span>}
          {live.map((p) => (
            <div key={`t${p.id}`} data-testid="method-live">
              <strong>{p.method ? METHOD_NAME[p.method] : "Choosing…"}</strong>
              <span className="muted"> · {p.op === "move" ? "moving" : "copying"} {formatCount(p.filesTotal)} item{p.filesTotal === 1 ? "" : "s"} to {p.destDir}{p.strategy === "rename" ? " (instant, same drive)" : ""}</span>
            </div>
          ))}
          {others.map((p) => (
            <div key={`d${p.id}`} data-testid="method-live">
              <strong>{p.method || "Working…"}</strong>
              <span className="muted"> · {jobGroup(p.action, p.permanent).toLowerCase()}: {p.selected === 1 && p.firstName ? p.firstName : `${formatCount(p.selected)} items`}</span>
            </div>
          ))}
        </dd>
        <dt>Last used</dt>
        <dd>
          {lastEntries.length === 0 && <span className="muted">Nothing since the app started.</span>}
          {lastEntries.map(([group, l]) => (
            <div key={group}>
              <span className="muted">{group}: </span>
              <strong>{l.method}</strong>
              {l.what && <span className="muted"> · {l.what}</span>}
            </div>
          ))}
        </dd>
      </dl>
      <table className="methods-table">
        <caption>Copying and moving between folders</caption>
        <thead>
          <tr><th>Method</th><th>On this computer</th><th>Copies</th><th>Average speed</th></tr>
        </thead>
        <tbody>
          {ALL_METHODS.map((m) => {
            const ok = report?.installed.includes(m);
            const st = stat(m);
            return (
              <tr key={m} data-available={ok ? "yes" : "no"}>
                <td title={METHOD_NOTE[m]}>{METHOD_NAME[m]}</td>
                <td>{report == null ? "…" : ok ? "Available" : <span className="muted">Not available</span>}</td>
                <td>{st ? formatCount(st.uses) : <span className="muted">0</span>}</td>
                <td>{st && st.avgBytesPerSec > 0 ? `${formatSize(st.avgBytesPerSec)}/s` : <span className="muted">—</span>}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
      <table className="methods-table">
        <caption>Everything else that moves or removes data</caption>
        <thead>
          <tr><th>Operation</th><th>Carried out by</th><th>Done</th><th>Average speed</th></tr>
        </thead>
        <tbody>
          {OTHER_OPERATIONS.map((o) => {
            const st = kindStat(o.kind);
            const n = o.kind ? (st?.uses ?? 0) : o.label === "Move inside one drive" ? (report?.instantMoves ?? 0) : undefined;
            return (
              <tr key={o.label}>
                <td>{o.label}</td>
                <td>{o.method}</td>
                <td>{n === undefined ? <span className="muted">not counted</span> : n > 0 ? formatCount(n) : <span className="muted">0</span>}</td>
                <td>{st && st.avgItemsPerSec > 0 ? `${st.avgItemsPerSec >= 10 ? formatCount(st.avgItemsPerSec) : st.avgItemsPerSec.toFixed(1)} items/s` : <span className="muted">—</span>}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
      <p className="muted">Counts and speeds are from the jobs remembered under File transfers. Servers and phones on a cable use their own connection, never one of the copy methods above.</p>
    </div>
  );
}

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

  const [crashes, setCrashes] = useState<CrashLog[]>([]);
  useEffect(() => {
    void api.crashLogs().then(setCrashes).catch(() => {});
  }, [rows]);
  const copyLog = async (name: string) => {
    const text = await api.crashLogRead(name).catch((e) => {
      showToast(String(e));
      return "";
    });
    if (text) await navigator.clipboard.writeText(text).then(() => showToast("Crash report copied. Paste it where you need it."));
  };
  const saveLog = async (name: string) => {
    const text = await api.crashLogRead(name).catch(() => "");
    if (!text) return;
    const a = document.createElement("a");
    a.href = URL.createObjectURL(new Blob([text], { type: "text/plain" }));
    a.download = name;
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 5000);
  };
  const sendLog = (name: string) => void api.crashSend(name).then((m) => { showToast(m); reload(); }).catch((e) => showToast(String(e)));

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
      {crashes.length > 0 && (
        <div className="crash-logs" data-testid="crash-logs">
          <h4>Crash logs</h4>
          <p className="muted">Each one lists the computer, what the app was doing and the last things you did. Copy one and paste it where it can be looked at. Clear them with the Clear button on the Crash logs row above.</p>
          <ul>
            {crashes.map((c) => (
              <li key={c.name}>
                <span>
                  {new Date(c.modified * 1000).toLocaleString()} · {formatSize(c.bytes)}
                  {c.sent ? " · sent" : ""}
                </span>
                <button className="secondary" onClick={() => void copyLog(c.name)}>
                  Copy
                </button>
                <button className="secondary" onClick={() => void saveLog(c.name)}>
                  Save as text file
                </button>
                {!c.sent && (
                  <button className="secondary" onClick={() => sendLog(c.name)}>
                    Send to GitHub issues
                  </button>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}
      <div className="button-row">
        <button className="secondary" onClick={reload}>
          Refresh sizes
        </button>
      </div>
      <CacheLocations onChanged={reload} />
    </div>
  );
}

function describeForget(r: ForgetReport): string {
  const parts: string[] = [];
  const add = (n: number, one: string, many: string) => n > 0 && parts.push(`${formatCount(n)} ${n === 1 ? one : many}`);
  add(r.listings, "folder listing", "folder listings");
  add(r.sizes, "folder size", "folder sizes");
  add(r.thumbnails, "thumbnail", "thumbnails");
  add(r.snapshots, "disk usage snapshot", "disk usage snapshots");
  add(r.predictions, "prediction", "predictions");
  add(r.indexEntries, "search entry", "search entries");
  return parts.length ? `Cleared ${parts.join(", ")}.` : "Nothing was saved for that place.";
}

/** Clear what is saved for one folder or drive, or switch saving off for it. */
function CacheLocations({ onChanged }: { onChanged: () => void }) {
  const settings = useStore((s) => s.settings)!;
  const drives = useStore((s) => s.drives);
  const { updateSettings, showToast } = useStore.getState();
  const [target, setTarget] = useState("");
  const rules = cacheRules(settings);
  const detached = (window as unknown as { __VIEW__?: string }).__VIEW__ === "settings";
  const place = target.trim();
  const covered = !!place && rules.some((r) => samePlace(r, place));

  const setRules = (next: string[]) =>
    updateSettings((s) => {
      if (next.length) s.advanced[RULES_KEY] = next;
      else delete s.advanced[RULES_KEY];
    });
  const forget = async (path: string, quiet = false) => {
    try {
      const r = await api.clearCacheFor(path);
      if (!quiet) showToast(describeForget(r));
      onChanged();
    } catch (e) {
      showToast(String(e));
    }
  };
  const browse = async () => {
    if (detached) {
      const r = await openDialog({ directory: true, multiple: false, title: "Choose a folder or drive" });
      if (typeof r === "string") setTarget(r);
      return;
    }
    const r = await pickFolder("Choose a folder or drive", place || undefined);
    if (r) setTarget(r);
  };
  const turnOff = async () => {
    if (!place || covered) return;
    setRules([...rules, place]);
    await forget(place, true);
    showToast(`Nothing will be saved for ${place} from now on. What was saved has been cleared.`);
    setTarget("");
  };

  return (
    <section className="cache-locations" aria-label="Caching for specific folders and drives">
      <h3>Folders and drives</h3>
      <p className="muted">
        Clear what is saved about one place, or stop saving anything about it (listings, folder sizes, thumbnails, disk usage, predictions and search entries). A folder covers everything inside it. Searching still works there; it just reads the folder each time.
      </p>
      <div className="loc-pick">
        <select
          aria-label="Pick a drive"
          value=""
          onChange={(e) => e.target.value && setTarget(e.target.value)}
        >
          <option value="">Pick a drive…</option>
          {drives.map((d) => (
            <option key={d.mount} value={d.mount}>
              {d.name && d.name !== d.mount ? `${d.name} (${d.mount})` : d.mount}
            </option>
          ))}
        </select>
        <button className="secondary" onClick={() => void browse()}>
          Choose folder…
        </button>
        <input
          className="text-input"
          value={target}
          placeholder="…or type a full path"
          spellCheck={false}
          onChange={(e) => setTarget(e.target.value)}
          aria-label="Folder or drive"
        />
      </div>
      <div className="button-row">
        <button className="secondary" disabled={!place} onClick={() => void forget(place)}>
          Clear what is saved here
        </button>
        <button className="secondary" disabled={!place || covered} onClick={() => void turnOff()}>
          {covered ? "Saving is already off here" : "Stop saving for this place"}
        </button>
      </div>
      {rules.length > 0 && (
        <ul className="loc-rules" aria-label="Places where nothing is saved">
          {rules.map((r) => (
            <li key={r}>
              <code title={r}>{r}</code>
              <span className="muted">not saved</span>
              <button className="secondary" onClick={() => void forget(r)}>
                Clear
              </button>
              <button className="secondary" onClick={() => setRules(rules.filter((x) => x !== r))}>
                Save again
              </button>
            </li>
          ))}
        </ul>
      )}
      {rules.length === 0 && <p className="muted">Saving is on everywhere.</p>}
    </section>
  );
}

function samePlace(a: string, b: string): boolean {
  const n = (x: string) => x.replace(/[\\/]+$/, "").toLowerCase();
  return n(a) === n(b);
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

const NO_COMMANDS: UserCommand[] = [];
const blankCommand = (): UserCommand => ({ id: Math.random().toString(36).slice(2, 9), label: "New command", icon: "", program: "", params: "%N", startDir: "", runIn: "program", onButtonBar: true, inMenu: true });

/** Commands for the button bar and the Commands menu, with Total Commander style placeholders. */
export function UserCommandsPanel() {
  const commands = useStore((s) => s.settings!.userCommands) ?? NO_COMMANDS;
  const { updateSettings } = useStore.getState();
  const change = (id: string, patch: Partial<UserCommand>) => updateSettings((s) => void (s.userCommands = (s.userCommands ?? []).map((a) => (a.id === id ? { ...a, ...patch } : a))));
  const move = (i: number, by: number) =>
    updateSettings((s) => {
      const list = [...(s.userCommands ?? [])];
      const j = i + by;
      if (j < 0 || j >= list.length) return;
      [list[i], list[j]] = [list[j], list[i]];
      s.userCommands = list;
    });
  return (
    <div className="custom-actions">
      <p className="muted">
        Commands for the button bar above the file list and the Commands menu. In the program, parameters and start folder: <code>%P</code> the folder you are viewing (with a closing slash), <code>%p</code> without it, <code>%N</code> the item under the cursor, <code>%n</code> its name without extension, <code>%E</code> its extension, <code>%F</code> its full path, <code>%S</code> the names of the selected items, <code>%L</code> their full paths, <code>%D</code> the name of the folder, <code>%%</code> a percent sign. Names with spaces are quoted for you. Programs are started directly, never through a shell, unless you choose to run them in a terminal tab.
      </p>
      {commands.map((a, i) => (
        <div key={a.id} className="action-card">
          <div className="action-grid">
            <label><span className="muted">Name</span><input className="text-input" value={a.label} onChange={(e) => change(a.id, { label: e.target.value })} /></label>
            <label><span className="muted">Button symbol</span><input className="text-input" maxLength={2} value={a.icon} placeholder="a letter or emoji" onChange={(e) => change(a.id, { icon: e.target.value })} /></label>
            <label><span className="muted">Program</span><input className="text-input" value={a.program} placeholder="code, notepad.exe, /usr/bin/vlc…" onChange={(e) => change(a.id, { program: e.target.value })} /></label>
            <label><span className="muted">Parameters</span><input className="text-input" value={a.params} onChange={(e) => change(a.id, { params: e.target.value })} /></label>
            <label><span className="muted">Start in</span><input className="text-input" value={a.startDir} placeholder="(the folder you are viewing)" onChange={(e) => change(a.id, { startDir: e.target.value })} /></label>
            <label>
              <span className="muted">Run</span>
              <select value={a.runIn} onChange={(e) => change(a.id, { runIn: e.target.value as UserCommand["runIn"] })}>
                <option value="program">Start the program</option>
                <option value="terminal">In a new terminal tab (for console programs)</option>
              </select>
            </label>
            <label className="inline-toggle"><input type="checkbox" checked={a.onButtonBar} onChange={(e) => change(a.id, { onButtonBar: e.target.checked })} /> On the button bar</label>
            <label className="inline-toggle"><input type="checkbox" checked={a.inMenu} onChange={(e) => change(a.id, { inMenu: e.target.checked })} /> In the Commands menu</label>
          </div>
          <div className="action-buttons">
            <button className="secondary" disabled={i === 0} onClick={() => move(i, -1)}>Move up</button>
            <button className="secondary" disabled={i === commands.length - 1} onClick={() => move(i, 1)}>Move down</button>
            <button className="secondary" onClick={() => updateSettings((s) => void (s.userCommands = (s.userCommands ?? []).filter((x) => x.id !== a.id)))}>Remove</button>
          </div>
        </div>
      ))}
      <button className="secondary" onClick={() => updateSettings((s) => void (s.userCommands = [...(s.userCommands ?? []), blankCommand()]))}>Add a command</button>
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

// ---------------------------------------------------------------------------
// Default file manager
// ---------------------------------------------------------------------------

/** What the system's own file manager is called, for the wording here. */
const SYSTEM_FM: Record<string, string> = { windows: "File Explorer", macos: "Finder", linux: "the system's file manager" };

/** The switch that makes the app what the system starts for folders, and puts things back. */
export function DefaultFileManagerPanel() {
  const os = useStore((s) => s.info?.os) ?? "";
  const { showToast } = useStore.getState();
  const [status, setStatus] = useState<DefaultFmStatus | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!isMobileOs(os)) void api.defaultFmStatus().then(setStatus).catch(() => setStatus(null));
  }, [os]);

  // Phones have no single switch: the system asks which app to use, and remembers "Always".
  if (os === "android") {
    return (
      <div className="default-fm">
        <p className="muted">
          Android has no setting that names one file manager. Instead, the app is offered whenever another app opens a folder, taps "Open downloads", or opens a file with "Open with". Choose
          FAST File Explorer there and tap "Always" to make it the default for that kind of thing.
        </p>
        <div className="button-row">
          <button className="secondary" onClick={() => void (openAppDefaults() || showToast("This device has no such page."))}>
            Open this app's "Open by default" page
          </button>
        </div>
        <p className="muted">That page is also where a choice of "Always" is cleared again. The system's own Files app stays installed.</p>
      </div>
    );
  }
  if (os === "ios") {
    return (
      <div className="default-fm">
        <p className="muted">
          iPhone and iPad do not let any app take the place of Files. The app is listed under Share &gt; "Open in" for files other apps hand over, and its own folder appears inside Files.
        </p>
      </div>
    );
  }
  if (!status) return <p className="muted">Checking…</p>;
  const system = SYSTEM_FM[os] ?? "the system's file manager";

  const change = async (on: boolean) => {
    setBusy(true);
    try {
      const next = await api.defaultFmSet(on);
      setStatus(next);
      if (on && !next.active) showToast("The system did not take the change. Nothing was altered.");
      else showToast(on ? "Folders now open here." : `Folders open in ${system} again.`);
    } catch (e) {
      showToast(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="default-fm">
      <p className="muted" data-on={status.active || undefined}>
        {status.active ? "On. " : status.supported ? "Off. " : ""}
        {status.detail}
      </p>
      {status.supported && (
        <div className="button-row">
          {status.active ? (
            <button className="secondary" disabled={busy} onClick={() => void change(false)}>
              Give folders back to {status.previous ? "what was set before" : system}
            </button>
          ) : (
            <button className="primary" disabled={busy} onClick={() => void change(true)}>
              Make this the default file manager
            </button>
          )}
        </div>
      )}
      {status.limits.length > 0 && (
        <>
          <p className="muted">What stays with {system}:</p>
          <ul className="muted default-fm-limits">
            {status.limits.map((l) => (
              <li key={l}>{l}</li>
            ))}
          </ul>
        </>
      )}
    </div>
  );
}
