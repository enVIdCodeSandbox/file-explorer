// Settings panels for presets, profiles and what the app is using.

import { useEffect, useMemo, useRef, useState } from "react";
import { api, type ResourceUsage } from "../api";
import { adv } from "../settings/advanced";
import {
  PERF_PRESETS,
  UI_PRESETS,
  applyPatch,
  applyProfile,
  applyUiPreset,
  currentPerfPreset,
  differences,
  newProfile,
  platformFor,
  recommendPerf,
} from "../settings/presets";
import { ask, promptText, useStore } from "../store";
import { formatSize } from "../util/format";
import { Icon } from "./Icons";

const systemDark = () => window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? true;

// ---------------------------------------------------------------------------
// Performance presets
// ---------------------------------------------------------------------------

export function PerfPresetsPanel({ compact = false, roomy = false }: { compact?: boolean; roomy?: boolean }) {
  const settings = useStore((s) => s.settings)!;
  const info = useStore((s) => s.info);
  const { updateSettings, showToast } = useStore.getState();
  const [open, setOpen] = useState<string | null>(null);
  const hw = info?.system;
  const recommended = useMemo(
    () => (hw ? recommendPerf(hw.totalRamMb, hw.cpus, hw.batteryPercent != null) : null),
    [hw],
  );
  const current = currentPerfPreset(settings);
  const choose = (id: string) => {
    const preset = PERF_PRESETS.find((p) => p.id === id)!;
    updateSettings((s) => applyPatch(s, preset.patch));
    showToast(`Performance: ${preset.name}. Every value can still be changed in Settings.`);
  };
  return (
    <div className="preset-panel">
      {hw && recommended && (
        <p className="muted">
          This computer: {hw.cpus} threads, {(hw.totalRamMb / 1024).toFixed(hw.totalRamMb >= 10240 ? 0 : 1)} GB of memory{hw.batteryPercent != null ? ", with a battery" : ""}. Recommended: <strong>{recommended.name}</strong>.
        </p>
      )}
      <div className="preset-grid preset-grid-perf" role="radiogroup" aria-label="Performance preset">
        {PERF_PRESETS.map((p, i) => {
          const isCurrent = current?.id === p.id;
          const isRecommended = recommended?.id === p.id;
          return (
            <div key={p.id} className="preset-card" data-clickable data-on={isCurrent || undefined} data-recommended={isRecommended || undefined} data-preset={p.id} onClick={() => choose(p.id)}>
              <button role="radio" aria-checked={isCurrent} className="preset-pick" onClick={() => choose(p.id)}>
                <span className="preset-step">{i + 1}</span>
                <strong>{p.name}</strong>
                {isRecommended && <span className="pill">Recommended for this computer</span>}
                {isCurrent && <span className="pill" data-kind="current">In use</span>}
              </button>
              {(!compact || roomy) && <span className="muted">{p.description}</span>}
              <span className="preset-for">
                <b>Suits:</b> {p.recommendedFor}
              </span>
              {!compact && !roomy && (
                <button className="link-button" aria-expanded={open === p.id} onClick={(e) => { e.stopPropagation(); setOpen(open === p.id ? null : p.id); }}>
                  {open === p.id ? "Hide what it sets" : "What it sets"}
                </button>
              )}
              {open === p.id && (
                <ul className="preset-values">
                  {Object.entries(p.patch).map(([k, v]) => (
                    <li key={k}>
                      <code>{k.replace(/^advanced\./, "")}</code>: {String(v)}
                    </li>
                  ))}
                </ul>
              )}
            </div>
          );
        })}
      </div>
      {!current && !compact && <p className="muted">Your settings do not match one preset exactly (you changed something by hand). Choosing a preset replaces those values.</p>}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Interface presets
// ---------------------------------------------------------------------------

export function UiPresetsPanel({ compact = false, roomy = false }: { compact?: boolean; roomy?: boolean }) {
  const settings = useStore((s) => s.settings)!;
  const os = useStore((s) => s.info?.os);
  const { updateSettings, showToast } = useStore.getState();
  const mine = platformFor(os);
  return (
    <div className="preset-panel">
      <div className="preset-grid" role="radiogroup" aria-label="Interface style">
        {UI_PRESETS.map((p) => {
          const same = differences(settings, p.patch).length === 0;
          return (
            <div
              key={p.id}
              className="preset-card preset-card-ui"
              data-clickable
              data-on={same || undefined}
              data-recommended={p.id === mine || undefined}
              data-preset={p.id}
              onClick={() => {
                updateSettings((s) => applyUiPreset(s, p, systemDark()));
                showToast(`${p.name}-style interface applied.`);
              }}
            >
              <button role="radio" aria-checked={same} className="preset-pick">
                <strong>{p.name}</strong>
                {p.id === mine && <span className="pill">Matches this computer</span>}
                {same && <span className="pill" data-kind="current">In use</span>}
              </button>
              {(!compact || roomy) && <span className="muted">{p.description}</span>}
              <span className="preset-for">
                <b>Suits:</b> {p.recommendedFor}
              </span>
            </div>
          );
        })}
      </div>
      <p className="muted">
        A style sets the window layout, colors, icons, corner shapes, where the window buttons sit, and the keyboard shortcuts. Anything it changes can still be changed afterwards in Layout, Themes and Advanced. To keep your own version, save it as a profile.
      </p>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Profiles
// ---------------------------------------------------------------------------

export function ProfilesPanel() {
  const settings = useStore((s) => s.settings)!;
  const { updateSettings, showToast } = useStore.getState();
  const active = settings.profiles.find((p) => p.id === settings.activeProfile);
  const save = async () => {
    const name = await promptText("Name for this profile", "");
    if (!name) return;
    updateSettings((s) => {
      const p = newProfile(s, name);
      s.profiles = [...s.profiles, p];
      s.activeProfile = p.id;
    });
    showToast(`Saved “${name}”. Changes you make now are kept in it automatically.`);
  };
  const use = (id: string) => {
    const p = settings.profiles.find((x) => x.id === id);
    if (!p) return;
    updateSettings((s) => applyProfile(s, p));
    showToast(`Using “${p.name}”.`);
  };
  const rename = async (id: string, was: string) => {
    const name = await promptText("Profile name", was);
    if (name) updateSettings((s) => void (s.profiles.find((p) => p.id === id)!.name = name));
  };
  const duplicate = (id: string) =>
    updateSettings((s) => {
      const from = s.profiles.find((p) => p.id === id)!;
      s.profiles = [...s.profiles, { ...structuredClone(from), id: newProfile(s, "x").id, name: `${from.name} copy`, builtIn: false }];
    });
  const remove = async (id: string, name: string) => {
    const r = await ask({
      title: `Delete “${name}”?`,
      message: "Your current settings are not changed.",
      buttons: [
        { id: "ok", label: "Delete", primary: true, danger: true },
        { id: "cancel", label: "Cancel" },
      ],
    });
    if (r?.button === "ok") updateSettings((s) => {
      s.profiles = s.profiles.filter((p) => p.id !== id);
      if (s.activeProfile === id) s.activeProfile = "";
    });
  };
  return (
    <div className="profiles-panel">
      <p className="muted">
        A profile is a saved copy of all your settings: layout, theme, performance, shortcuts and the rest. Keep as many as you like, for example one for work and one for a laptop on battery. While a profile is in use, anything you change is saved into it automatically.
      </p>
      <div className="button-row">
        <button className="secondary" onClick={() => void save()}>
          <Icon name="plus" size={13} /> Save current settings as a profile…
        </button>
        {active && (
          <button className="secondary" onClick={() => updateSettings((s) => void (s.activeProfile = ""))} title="Stop saving changes into the profile. Settings stay as they are.">
            Stop using “{active.name}”
          </button>
        )}
      </div>
      {settings.profiles.length === 0 ? (
        <p className="muted">No profiles yet.</p>
      ) : (
        <ul className="profile-list">
          {settings.profiles.map((p) => (
            <li key={p.id} data-on={p.id === settings.activeProfile || undefined}>
              <span className="profile-name">
                <strong>{p.name}</strong>
                {p.id === settings.activeProfile && <span className="pill" data-kind="current">In use, saving changes</span>}
                {p.description && <span className="muted">{p.description}</span>}
              </span>
              <button className="secondary" disabled={p.id === settings.activeProfile} onClick={() => use(p.id)}>
                Use
              </button>
              <button className="secondary" onClick={() => void rename(p.id, p.name)}>
                Rename
              </button>
              <button className="secondary" onClick={() => duplicate(p.id)}>
                Duplicate
              </button>
              <button className="secondary" onClick={() => void remove(p.id, p.name)}>
                Delete
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Resource use
// ---------------------------------------------------------------------------

const HISTORY = 60;

function Spark({ values, max, label }: { values: number[]; max: number; label: string }) {
  const w = 160;
  const h = 34;
  const pts = values.map((v, i) => `${(i / (HISTORY - 1)) * w},${h - Math.min(1, v / Math.max(1, max)) * (h - 2) - 1}`).join(" ");
  return (
    <svg className="spark" width={w} height={h} viewBox={`0 0 ${w} ${h}`} role="img" aria-label={label}>
      <polyline points={pts} fill="none" stroke="var(--accent)" strokeWidth="1.5" strokeLinejoin="round" />
    </svg>
  );
}

const rate = (bytes: number) => `${formatSize(Math.max(0, bytes))}/s`;

export function ResourcesPanel() {
  const settings = useStore((s) => s.settings)!;
  const { updateSettings, showToast } = useStore.getState();
  const [now, setNow] = useState<ResourceUsage | null>(null);
  const [cpu, setCpu] = useState<number[]>([]);
  const [mem, setMem] = useState<number[]>([]);
  const [io, setIo] = useState({ read: 0, write: 0 });
  const last = useRef<{ r: number; w: number; t: number } | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let alive = true;
    const tick = async () => {
      const u = await api.resourceUsage().catch(() => null);
      if (!alive || !u) return;
      const t = performance.now();
      if (last.current) {
        const dt = Math.max(0.2, (t - last.current.t) / 1000);
        setIo({ read: (u.diskReadTotal - last.current.r) / dt, write: (u.diskWriteTotal - last.current.w) / dt });
      }
      last.current = { r: u.diskReadTotal, w: u.diskWriteTotal, t };
      setNow(u);
      setCpu((c) => [...c.slice(-(HISTORY - 1)), u.cpuPercent]);
      setMem((m) => [...m.slice(-(HISTORY - 1)), u.memoryBytes]);
    };
    void tick();
    const id = setInterval(() => void tick(), 1500);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  const padded = (a: number[]) => [...Array(Math.max(0, HISTORY - a.length)).fill(0), ...a];
  const release = async () => {
    setBusy(true);
    try {
      const [before, after] = await api.compactMemory();
      showToast(before > after ? `Gave back ${formatSize(before - after)} of memory.` : "There was nothing more to give back right now.");
    } catch (e) {
      showToast(String(e));
    } finally {
      setBusy(false);
    }
  };
  const clearRebuildable = async () => {
    const rows = await api.cacheUsage().catch(() => []);
    for (const r of rows.filter((x) => x.rebuildable && x.id !== "scratch")) await api.clearCache(r.id).catch(() => {});
    await api.compactMemory().catch(() => {});
    showToast("Cleared everything that rebuilds itself.");
  };
  const smallest = async () => {
    const r = await ask({
      title: "Use the smallest footprint?",
      message:
        "Switches to the Barebones performance preset, turns off every kind of saved data (thumbnails, folder sizes, search index, hardware details, folder learning), clears what is stored now and gives back memory when the window is in the background. Search then scans the folder you are in. You can turn things back on in Settings.",
      buttons: [
        { id: "ok", label: "Use smallest footprint", primary: true },
        { id: "cancel", label: "Cancel" },
      ],
    });
    if (r?.button !== "ok") return;
    updateSettings((s) => applyPatch(s, PERF_PRESETS[0].patch));
    await api.clearCache("all").catch(() => {});
    await api.compactMemory().catch(() => {});
    showToast("Smallest footprint is on.");
  };

  if (!now) return <p className="muted">Reading…</p>;
  const freeShare = now.systemAvailableBytes / Math.max(1, now.systemTotalBytes);
  return (
    <div className="resources-panel">
      <div className="resource-grid">
        <div className="resource-card" data-kind="active">
          <h4>In use right now</h4>
          <dl className="details-list">
            <div className="about-row">
              <dt>Processor</dt>
              <dd>
                {now.cpuPercent.toFixed(1)}% of one core <span className="muted">({(now.cpuPercent / now.cpus).toFixed(1)}% of this computer)</span>
                <Spark values={padded(cpu)} max={Math.max(25, ...cpu)} label="Processor use over the last minute and a half" />
              </dd>
            </div>
            <div className="about-row">
              <dt>Memory</dt>
              <dd>
                {formatSize(now.memoryBytes)}
                <Spark values={padded(mem)} max={Math.max(1, ...mem) * 1.2} label="Memory use over the last minute and a half" />
              </dd>
            </div>
            <div className="about-row">
              <dt>Disk</dt>
              <dd>
                reading {rate(io.read)}, writing {rate(io.write)}
                <span className="muted"> · {formatSize(now.diskReadTotal)} read and {formatSize(now.diskWriteTotal)} written since it started</span>
              </dd>
            </div>
            {now.threads > 0 && (
              <div className="about-row">
                <dt>Threads</dt>
                <dd>{now.threads}</dd>
              </div>
            )}
          </dl>
        </div>
        <div className="resource-card" data-kind="passive">
          <h4>Kept for later</h4>
          <dl className="details-list">
            <div className="about-row">
              <dt>On disk</dt>
              <dd>
                {formatSize(now.storedDiskBytes)} <span className="muted">in caches (Settings &gt; Storage lists each one)</span>
              </dd>
            </div>
            <div className="about-row">
              <dt>Cached in memory</dt>
              <dd>
                {formatSize(now.cachedMemoryBytes)} <span className="muted">of the memory above: the search index, folder listings and folder sizes</span>
              </dd>
            </div>
            <div className="about-row">
              <dt>Free on this computer</dt>
              <dd>
                {formatSize(now.systemAvailableBytes)} of {formatSize(now.systemTotalBytes)} ({Math.round(freeShare * 100)}% free)
              </dd>
            </div>
          </dl>
        </div>
      </div>
      <h4>Use as little as possible</h4>
      <p className="muted">
        These do not compress the files on your drives. They shrink what the app keeps: cached listings and indexes in memory, and caches on disk. All of it is rebuilt on demand, only slower the first time.
      </p>
      <div className="button-row">
        <button className="secondary" disabled={busy} onClick={() => void release()}>
          <Icon name="memory" size={13} /> Give back memory now
        </button>
        <button className="secondary" onClick={() => void clearRebuildable()}>
          Clear caches that rebuild themselves
        </button>
        <button className="secondary" onClick={() => void smallest()}>
          Smallest footprint…
        </button>
      </div>
      <label className="check-line">
        <input
          type="checkbox"
          checked={adv<boolean>(settings, "perf.compactWhenIdle")}
          onChange={(e) => updateSettings((s) => void (s.advanced = { ...s.advanced, "perf.compactWhenIdle": e.target.checked }))}
        />
        Give memory back automatically two minutes after the window goes to the background
      </label>
    </div>
  );
}
