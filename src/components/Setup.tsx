// First-run setup: a short wizard with one view per topic. Move with the
// Back and Next buttons or the left and right arrow keys. The last view lists
// everything that was chosen, and a button confirms it. The recommended answer
// is marked on every view. Choices show their effect straight away (so the look
// can be judged) and "Skip" puts the settings back exactly as they were.

import type { ReactNode } from "react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { Settings } from "../api";
import { adv } from "../settings/advanced";
import {
  UI_PRESETS,
  applyPatch,
  applyUiPreset,
  currentPerfPreset,
  differences,
  mobilePlatformFor,
  perfPresetsFor,
  platformFor,
  recommendMobilePerf,
  recommendPerf,
  uiPresetsFor,
  type Platform,
} from "../settings/presets";
import { LANGUAGES, useT } from "../i18n";
import { useStore } from "../store";
import { THEMES } from "../util/themes";
import { CaptionStrip } from "./Chrome";
import { PerfSlider } from "./PerfSlider";
import { deviceMaker, useMobile } from "../util/device";

const systemDark = () => window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? true;

const svg = (d: ReactNode) => (
  <svg viewBox="0 0 24 24" width="30" height="30" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    {d}
  </svg>
);
const ICONS = {
  folder: svg(<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />),
  image: svg(<><rect x="3" y="4" width="18" height="16" rx="2" /><circle cx="9" cy="10" r="1.6" /><path d="m21 16-5-5-8 8" /></>),
  chip: svg(<><rect x="6" y="6" width="12" height="12" rx="2" /><path d="M9 2v4M15 2v4M9 18v4M15 18v4M2 9h4M2 15h4M18 9h4M18 15h4" /></>),
  route: svg(<><circle cx="6" cy="18" r="2" /><circle cx="18" cy="6" r="2" /><path d="M8 18h6a3 3 0 0 0 0-6h-4a3 3 0 0 1 0-6h6" /></>),
  search: svg(<><circle cx="11" cy="11" r="7" /><path d="m21 21-4.3-4.3" /></>),
  eyeoff: svg(<><path d="M3 3l18 18" /><path d="M10.6 6.2A9.8 9.8 0 0 1 12 6c5 0 9 6 9 6a15 15 0 0 1-3.2 3.7M6.6 7.6C4.3 9.2 3 12 3 12s4 6 9 6c1.3 0 2.5-.3 3.6-.8" /><path d="M9.9 9.9a3 3 0 0 0 4.2 4.2" /></>),
  refresh: svg(<><path d="M20 11a8 8 0 0 0-14.5-4M4 13a8 8 0 0 0 14.5 4" /><path d="M20 4v5h-5M4 20v-5h5" /></>),
  shield: svg(<><path d="M12 3 4 6v6c0 4.5 3.2 8 8 9 4.8-1 8-4.5 8-9V6z" /><path d="m9 12 2 2 4-4" /></>),
};

/** A big tile that is also a switch: the whole tile toggles. */
function Tile({ icon, title, hint, on, onToggle }: { icon: ReactNode; title: string; hint: string; on: boolean; onToggle: (v: boolean) => void }) {
  const t = useT();
  return (
    <button role="switch" aria-checked={on} aria-label={t(title)} className="setup-tile" data-on={on || undefined} onClick={() => onToggle(!on)}>
      <span className="tile-icon">{icon}</span>
      <strong>{t(title)}</strong>
      <span className="muted">{t(hint)}</span>
      <span className="tile-foot">
        <span className="pill">{t("Recommended")}</span>
        <span className="grow" />
        <em>{on ? t("On") : t("Off")}</em>
        <span className="switch" data-on={on || undefined}>
          <span />
        </span>
      </span>
    </button>
  );
}

const OS_CARDS: { id: Platform; name: string; tagline: string }[] = [
  { id: "windows", name: "Windows 11", tagline: "Like File Explorer in Windows 11" },
  { id: "windows10", name: "Windows 10", tagline: "Like File Explorer in Windows 10" },
  { id: "macos", name: "macOS", tagline: "Like Finder on a Mac" },
  { id: "linux", name: "Linux", tagline: "Clean and simple, like GNOME or KDE" },
  { id: "google", name: "Google", tagline: "Like Files by Google on a Pixel" },
  { id: "samsung", name: "Samsung", tagline: "Like My Files on a Galaxy" },
  { id: "apple", name: "Apple", tagline: "Like Files on an iPhone" },
];

const STEPS = ["Welcome", "Interface", "Colors", "Performance", "Saving", "Privacy", "Updates", "Review"] as const;

export function Setup() {
  const t = useT();
  const settings = useStore((s) => s.settings)!;
  const info = useStore((s) => s.info);
  const { updateSettings } = useStore.getState();
  const hw = info?.system;
  const mobile = useMobile();
  const maker = useMemo(deviceMaker, []);
  // On a phone the choices are the three phone looks and the lighter phone presets.
  const mine = mobile ? mobilePlatformFor(info?.os, maker) : platformFor(info?.os, maker);
  const LOOKS = uiPresetsFor(mobile);
  const PRESETS = perfPresetsFor(mobile);
  const recommended = useMemo(
    () => (hw ? (mobile ? recommendMobilePerf(hw.totalRamMb, hw.cpus) : recommendPerf(hw.totalRamMb, hw.cpus, hw.batteryPercent != null)) : null),
    [hw, mobile],
  );
  const [step, setStep] = useState(0);
  const original = useRef<Settings | null>(null);
  if (!original.current) original.current = structuredClone(settings);
  const last = step === STEPS.length - 1;
  const [perfPick, setPerfPick] = useState<number | null>(null);
  // The interface style for the operating system the app runs on starts selected, and
  // "match my system" is the recommended color choice. Done once, as soon as the OS is known.
  const autoUi = useRef(false);
  useEffect(() => {
    if (!info || autoUi.current) return;
    autoUi.current = true;
    updateSettings((s) => {
      applyUiPreset(s, UI_PRESETS.find((p) => p.id === mine)!, systemDark());
      s.appearance.theme = "system";
    });
  }, [info]); // eslint-disable-line react-hooks/exhaustive-deps

  // The same sentences, saying "device" where a phone or tablet is meant.
  const w = (text: string) => (mobile ? text.replace(/your computer/g, "your device").replace(/computer/g, "device").replace(/Computer/g, "Device") : text);

  const go = (to: number) => setStep(Math.max(0, Math.min(STEPS.length - 1, to)));

  // Left and right arrows move between views, unless a control is using them.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.altKey || e.ctrlKey || e.metaKey) return;
      if ((e.target as HTMLElement | null)?.closest?.("input, select, textarea")) return;
      if (e.key === "ArrowRight") {
        e.preventDefault();
        setStep((s) => Math.min(STEPS.length - 1, s + 1));
      } else if (e.key === "ArrowLeft") {
        e.preventDefault();
        setStep((s) => Math.max(0, s - 1));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const set = (key: string, value: unknown) => updateSettings((s) => void (s.advanced = { ...s.advanced, [key]: value }));
  const theme = settings.appearance.theme;
  const mode: "system" | "dark" | "light" = theme === "system" ? "system" : THEMES.find((t) => t.id === theme)?.dark === false ? "light" : "dark";
  const pickMode = (m: "system" | "dark" | "light") =>
    updateSettings((s) => {
      const preset = LOOKS.find((p) => differences(s, p.patch).length === 0) ?? UI_PRESETS.find((p) => p.id === mine)!;
      s.appearance.theme = m === "system" ? "system" : m === "dark" ? preset.themes.dark : preset.themes.light;
    });

  // What will be used: the person's pick, or the recommendation where nothing was picked.
  const pickedUi = LOOKS.find((p) => differences(settings, p.patch).length === 0);
  const ui = pickedUi ?? UI_PRESETS.find((p) => p.id === mine)!;
  const pickedPerf = currentPerfPreset(settings);
  const recIdx = Math.max(0, PRESETS.findIndex((p) => p.id === (recommended ?? PRESETS[Math.min(3, PRESETS.length - 1)]).id));
  const pickedIdx = pickedPerf ? PRESETS.findIndex((p) => p.id === pickedPerf.id) : -1;
  const showIdx = perfPick ?? (pickedIdx >= 0 ? pickedIdx : recIdx);
  const perf = PRESETS[showIdx];
  const pickPerf = (i: number) => {
    setPerfPick(i);
    updateSettings((s) => applyPatch(s, PRESETS[i].patch));
  };
  const pickUi = (id: string) =>
    updateSettings((s) => {
      const keep = s.appearance.theme;
      applyUiPreset(s, UI_PRESETS.find((p) => p.id === id)!, systemDark());
      if (keep === "system") s.appearance.theme = "system";
    });

  const confirm = () =>
    updateSettings((s) => {
      if (!pickedUi) {
        const keep = s.appearance.theme;
        applyUiPreset(s, ui, systemDark());
        if (keep === "system") s.appearance.theme = "system";
      }
      if (pickedIdx < 0 || perfPick != null) applyPatch(s, perf.patch);
      s.setupDone = true;
    });
  const skip = () =>
    updateSettings((s) => {
      // Put everything back as it was, then just remember that setup was shown.
      const was = structuredClone(original.current!);
      const language = s.advanced["ui.language"]; // the language is not a "look" setting, so it stays
      Object.assign(s, was);
      if (language !== undefined) s.advanced = { ...s.advanced, "ui.language": language };
      s.setupDone = true;
    });

  const yes = (v: boolean) => (v ? t("On") : t("Off"));
  const rec = ` ${t("(recommended)")}`;
  const summary: [string, string, string][] = [
    [t("Interface style"), `${ui.name}${pickedUi ? "" : rec}`, pickedUi ? "" : t("Layout, icons, colors and shortcuts like {name}", { name: ui.name })],
    [t("Colors"), mode === "system" ? t("Match my system") : mode === "dark" ? t("Dark") : t("Light"), THEMES.find((th) => th.id === theme)?.name ?? ""],
    [t("Performance"), `${perf.name}${pickedIdx >= 0 || perfPick != null ? "" : rec}`, perf.recommendedFor],
    [t("Folder sizes"), yes(adv(settings, "files.sizeCache")), ""],
    [t("Thumbnails"), yes(adv(settings, "cache.thumbnails")), ""],
    [t(w("This computer's details")), yes(adv(settings, "cache.hardware")), ""],
    [t("Folders you open next"), yes(adv(settings, "cache.predictions")), ""],
    [t("Previous searches"), adv<number>(settings, "search.historyLimit") > 0 ? t("Last {n}", { n: adv<number>(settings, "search.historyLimit") }) : t("Off"), ""],
    [t("Hide server addresses"), yes(settings.appearance.hideNetworkAddress), ""],
    ...(mobile ? [] : ([[t("Updates"), settings.updates.enabled ? (settings.updates.autoDownload ? t("Download automatically") : t("Check only")) : t("Off"), settings.updates.enabled ? t("You choose when to restart") : ""]] as [string, string, string][])),
  ];

  const ram = hw ? `${(hw.totalRamMb / 1024).toFixed(hw.totalRamMb >= 10240 ? 0 : 1)} GB` : "";
  const MODES: { id: "system" | "dark" | "light"; name: string; hint: string }[] = [
    { id: "system", name: "Match my system", hint: w("Follows your computer, day and night.") },
    { id: "dark", name: "Dark", hint: "Easy on the eyes in dim rooms." },
    { id: "light", name: "Light", hint: "Bright and crisp in daylight." },
  ];

  return (
    <div className="setup-backdrop" data-mobile={mobile || undefined} role="dialog" aria-modal="true" aria-label={t("Welcome to FAST File Explorer")}>
      {/* The top strip drags the window and holds minimize, maximize and close. */}
      <CaptionStrip />
      <div className="setup" data-step={STEPS[step]} data-mobile={mobile || undefined}>
        <div className="setup-top">
          <ol className="setup-steps" aria-label={t("Setup progress")}>
            {STEPS.map((name, i) => (
              <li key={name} data-state={i === step ? "current" : i < step ? "done" : "todo"}>
                <button onClick={() => go(i)} aria-current={i === step ? "step" : undefined} title={t(name)}>
                  <span className="setup-dot">{i < step ? "✓" : i + 1}</span>
                  <span className="setup-step-name">{t(name)}</span>
                </button>
              </li>
            ))}
          </ol>
          <button className="link-button" onClick={skip}>
            {t("Skip setup")}
          </button>
        </div>
        <div className="setup-progress" aria-hidden="true">
          <span style={{ width: `${((step + 1) / STEPS.length) * 100}%` }} />
        </div>

        <div className="setup-view" key={STEPS[step]}>
          <div className="setup-stage" data-hero={step === 0 || undefined} data-fill={(step >= 1 && step <= 6) || undefined}>
            {step === 0 && (
              <div className="setup-hero">
                <h1>{t("Welcome to FAST File Explorer")}</h1>
                <p className="lead">{t("A few quick choices to make it yours, one topic at a time. Use Next and Back or the arrow keys. At the end you will see everything you chose before it is saved.")}</p>
                <label className="setup-lang">
                  <span>{t("Language")}</span>
                  <select value={adv<string>(settings, "ui.language")} onChange={(e) => set("ui.language", e.target.value)} aria-label={t("Language")}>
                    <option value="auto">{t("Automatic (system language)")}</option>
                    {LANGUAGES.map((l) => (
                      <option key={l.code} value={l.code}>
                        {l.name}
                      </option>
                    ))}
                  </select>
                </label>
                {hw && (
                  <div className="setup-hw" aria-label="This computer">
                    <div><b>{hw.cpus}</b><span>{t("processor threads")}</span></div>
                    <div><b>{ram}</b><span>{t("memory")}</span></div>
                    <div><b>{hw.batteryPercent != null ? t("Yes") : t("No")}</b><span>{t("battery")}</span></div>
                    <div style={{ flex: 1, minWidth: 220, justifyContent: "center" }}><span>{t(w("The recommendations on the next views are based on this computer."))}</span></div>
                  </div>
                )}
              </div>
            )}
            {step === 1 && (
              <>
                <h2>{t("Which file manager are you used to?")}</h2>
                <p className="lead">{t("This sets the layout, icons and shortcuts together.")}</p>
                <div className="os-grid" role="radiogroup" aria-label={t("Interface style")}>
                  {OS_CARDS.filter((c) => LOOKS.some((l) => l.id === c.id)).map((c) => (
                    <div className="os-col" key={c.id}>
                      <span className="os-rec" aria-hidden={c.id !== mine} data-show={c.id === mine || undefined}>{t("Recommended")}</span>
                      <button className="os-card" role="radio" aria-checked={ui.id === c.id} data-on={ui.id === c.id || undefined} onClick={() => pickUi(c.id)}>
                        <div className="os-preview" data-os={c.id} aria-hidden="true">
                          <div className="wf-top"><b /><b /><b /><u /></div>
                          <div className="wf-bar" />
                          <div className="wf-body">
                            <div className="wf-side"><i /><i /><i /><i /></div>
                            <div className="wf-main"><i /><i /><i /><i /><i /></div>
                          </div>
                        </div>
                        <strong>{c.name}</strong>
                        <span className="muted">{t(c.tagline)}</span>
                      </button>
                    </div>
                  ))}
                </div>
              </>
            )}
            {step === 2 && (
              <>
                <h2>{t("Light or dark?")}</h2>
                <p className="lead">{t("You can pick from many more themes later in Settings.")}</p>
                <div className="setup-modes" role="radiogroup" aria-label={t("Light or dark?")}>
                  {MODES.map((m) => (
                    <div className="os-col" key={m.id}>
                      <span className="os-rec" aria-hidden={m.id !== "system"} data-show={m.id === "system" || undefined}>{t("Recommended")}</span>
                      <button className="setup-mode" role="radio" aria-checked={mode === m.id} data-on={mode === m.id || undefined} onClick={() => pickMode(m.id)}>
                        <div className="mode-preview" data-mode={m.id} aria-hidden="true">
                          {(m.id === "system" ? ["light", "dark"] : [m.id]).map((tone) => (
                            <div className="pane" data-tone={tone} key={tone}>
                              <b />
                              <div className="pbody">
                                <span className="pside"><i /><i /><i /></span>
                                <span className="prows"><i /><i /><i /><i /><i /><i /></span>
                              </div>
                            </div>
                          ))}
                          <span className="dot" />
                        </div>
                        <strong>{t(m.name)}</strong>
                        <span className="muted">{t(m.hint)}</span>
                      </button>
                    </div>
                  ))}
                </div>
              </>
            )}
            {step === 3 && (
              <>
                <h2>{t(w("How much of this computer should it use?"))}</h2>
                <p className="lead">
                  {t("More means faster search and folders, but more memory and background work.")}
                  {hw && <> {t(w("This computer has {cpus} threads and {ram} of memory."), { cpus: hw.cpus, ram })}</>}
                </p>
                <PerfSlider presets={PRESETS} index={showIdx} recIndex={recIdx} onPick={pickPerf} mobile={mobile} />
              </>
            )}
            {step === 4 && (
              <>
                <h2>{t("What may it remember?")}</h2>
                <p className="lead">{t(w("Remembering makes things faster. It is all saved on this computer only. Turn off anything you do not want kept."))}</p>
                <div className="setup-tiles">
                  <Tile icon={ICONS.folder} title="Folder sizes" hint="Show a folder's size at once next time." on={adv(settings, "files.sizeCache")} onToggle={(v) => set("files.sizeCache", v)} />
                  <Tile icon={ICONS.image} title="Thumbnails" hint="Keep small previews so picture folders open at once." on={adv(settings, "cache.thumbnails")} onToggle={(v) => set("cache.thumbnails", v)} />
                  <Tile icon={ICONS.chip} title={w("This computer's details")} hint="Keep hardware and drive health so About opens at once." on={adv(settings, "cache.hardware")} onToggle={(v) => set("cache.hardware", v)} />
                  <Tile icon={ICONS.route} title="Folders you open next" hint="Learn your habits to get the next folder ready." on={adv(settings, "cache.predictions")} onToggle={(v) => set("cache.predictions", v)} />
                  <Tile
                    icon={ICONS.search}
                    title="Previous searches"
                    hint="Show recent searches under the search box."
                    on={adv<number>(settings, "search.historyLimit") > 0}
                    onToggle={(v) => set("search.historyLimit", v ? 20 : 0)}
                  />
                  <div className="setup-tile-note" role="note">
                    <span className="tile-icon">{ICONS.shield}</span>
                    <strong>{t(w("Saved only on this computer"))}</strong>
                    <span className="muted">{t("Nothing here is ever uploaded.")}</span>
                  </div>
                </div>
              </>
            )}
            {step === 5 && (
              <>
                <h2>{t("Privacy")}</h2>
                <div className="privacy-grid">
                  <div className="privacy-local" role="note">
                    <span className="tile-icon big">{ICONS.shield}</span>
                    <h3>{t(w("Everything stays on this computer"))}</h3>
                    <p>{t("All information is stored locally. Nothing gets sent to a cloud.")}</p>
                    <p className="muted">{t("Your settings, history and saved details never leave this device.")}</p>
                  </div>
                  <Tile
                    icon={ICONS.eyeoff}
                    title="Hide server addresses"
                    hint="Network drives show a friendly name instead of an IP address or host."
                    on={settings.appearance.hideNetworkAddress}
                    onToggle={(v) => updateSettings((s) => void (s.appearance.hideNetworkAddress = v))}
                  />
                </div>
                <p className="muted">{t("Tip: lock a folder or drive later by right-clicking it and choosing “Lock with passcode…”.")}</p>
              </>
            )}
            {step === 6 && (
              <>
                <h2>{t("Updates")}</h2>
                {mobile ? (
                  <p className="lead">{t("Phones and tablets are updated through the file you install them from.")}</p>
                ) : (
                  <>
                    <p className="lead">{t("The app can keep itself up to date from the builds published on GitHub. A new version is downloaded with your other transfers and installed without extra screens; you choose when to restart.")}</p>
                    <div className="setup-tiles">
                      <Tile icon={ICONS.refresh} title="Check for updates" hint="Look for a newer build now and then." on={settings.updates.enabled} onToggle={(v) => updateSettings((s) => void (s.updates.enabled = v))} />
                      <Tile icon={ICONS.refresh} title="Check when it opens" hint="Look once each time the app starts." on={settings.updates.checkOnOpen} onToggle={(v) => updateSettings((s) => void (s.updates.checkOnOpen = v))} />
                      <Tile icon={ICONS.refresh} title="Keep checking" hint="Look again every few hours while the app stays open." on={settings.updates.background} onToggle={(v) => updateSettings((s) => void (s.updates.background = v))} />
                      <Tile icon={ICONS.refresh} title="Download automatically" hint="Fetch a new build as soon as it is found. You still choose when to restart." on={settings.updates.autoDownload} onToggle={(v) => updateSettings((s) => void (s.updates.autoDownload = v))} />
                      <Tile icon={ICONS.shield} title="Tell me about new builds" hint="A short note in the notifications, once for each build." on={settings.updates.notify} onToggle={(v) => updateSettings((s) => void (s.updates.notify = v))} />
                    </div>
                  </>
                )}
              </>
            )}
            {step === 7 && (
              <>
                <h2>{t("Here is what you chose")}</h2>
                <dl className="setup-summary">
                  {summary.map(([k, v, note]) => (
                    <div key={k}>
                      <dt>{k}</dt>
                      <dd>
                        {v}
                        {note && <span className="muted">{note}</span>}
                      </dd>
                    </div>
                  ))}
                </dl>
              </>
            )}
          </div>
        </div>

        <footer>
          {last && (
            <p className="setup-note" role="note">
              {t("All of this can be changed at any time in Settings, at the top right (or press Ctrl+,).")}
            </p>
          )}
          <div className="setup-buttons">
            <button className="secondary" onClick={() => go(step - 1)} disabled={step === 0}>
              {t("← Back")}
            </button>
            <span className="grow" />
            {last ? (
              <button className="primary" onClick={confirm} autoFocus>
                {t("Confirm and start")}
              </button>
            ) : (
              <button className="primary" onClick={() => go(step + 1)}>
                {t("Next →")}
              </button>
            )}
          </div>
        </footer>
      </div>
    </div>
  );
}
