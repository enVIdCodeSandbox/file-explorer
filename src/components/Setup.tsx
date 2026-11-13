// First-run setup: a short wizard with one view per topic. Move with the
// Back and Next buttons or the left and right arrow keys. The last view lists
// everything that was chosen, and a button confirms it. The recommended answer
// is marked on every view. Choices show their effect straight away (so the look
// can be judged) and "Skip" puts the settings back exactly as they were.

import { useEffect, useMemo, useRef, useState } from "react";
import type { Settings } from "../api";
import { adv } from "../settings/advanced";
import {
  PERF_PRESETS,
  UI_PRESETS,
  applyPatch,
  applyUiPreset,
  currentPerfPreset,
  differences,
  platformFor,
  recommendPerf,
} from "../settings/presets";
import { useStore } from "../store";
import { THEMES } from "../util/themes";
import { PerfPresetsPanel, UiPresetsPanel } from "./PresetPanels";

const systemDark = () => window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? true;

function Choice({ title, hint, recommended, on, onToggle }: { title: string; hint: string; recommended: boolean; on: boolean; onToggle: (v: boolean) => void }) {
  return (
    <div className="setup-choice">
      <div className="setting-text">
        <span className="setting-label">
          {title} <span className="pill" data-kind={on === recommended ? "recommended" : "other"}>{recommended ? "Recommended: on" : "Recommended: off"}</span>
        </span>
        <span className="muted">{hint}</span>
      </div>
      <button role="switch" aria-checked={on} aria-label={title} className="switch" data-on={on ? true : undefined} onClick={() => onToggle(!on)}>
        <span />
      </button>
    </div>
  );
}

const STEPS = ["Welcome", "Interface", "Colours", "Performance", "Saving", "Privacy", "Review"] as const;

export function Setup() {
  const settings = useStore((s) => s.settings)!;
  const info = useStore((s) => s.info);
  const { updateSettings } = useStore.getState();
  const hw = info?.system;
  const mine = platformFor(info?.os);
  const recommended = useMemo(() => (hw ? recommendPerf(hw.totalRamMb, hw.cpus, hw.batteryPercent != null) : null), [hw]);
  const [step, setStep] = useState(0);
  const original = useRef<Settings | null>(null);
  if (!original.current) original.current = structuredClone(settings);
  const last = step === STEPS.length - 1;
  // Match-my-system is the recommended colour choice, so it starts selected.
  useEffect(() => {
    if (settings.appearance.theme === "dark") updateSettings((s) => void (s.appearance.theme = "system"));
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

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
      const preset = UI_PRESETS.find((p) => differences(s, p.patch).length === 0) ?? UI_PRESETS.find((p) => p.id === mine)!;
      s.appearance.theme = m === "system" ? "system" : m === "dark" ? preset.themes.dark : preset.themes.light;
    });

  // What will be used: the person's pick, or the recommendation where nothing was picked.
  const pickedUi = UI_PRESETS.find((p) => differences(settings, p.patch).length === 0);
  const ui = pickedUi ?? UI_PRESETS.find((p) => p.id === mine)!;
  const pickedPerf = currentPerfPreset(settings);
  const perf = pickedPerf ?? recommended ?? PERF_PRESETS[3];

  const confirm = () =>
    updateSettings((s) => {
      if (!pickedUi) {
        const keep = s.appearance.theme;
        applyUiPreset(s, ui, systemDark());
        if (keep === "system") s.appearance.theme = "system";
      }
      if (!pickedPerf) applyPatch(s, perf.patch);
      s.setupDone = true;
    });
  const skip = () =>
    updateSettings((s) => {
      // Put everything back as it was, then just remember that setup was shown.
      const was = structuredClone(original.current!);
      Object.assign(s, was);
      s.setupDone = true;
    });

  const yes = (v: boolean) => (v ? "On" : "Off");
  const summary: [string, string, string][] = [
    ["Interface style", `${ui.name}${pickedUi ? "" : " (recommended)"}`, pickedUi ? "" : "Layout, icons, colours and shortcuts like " + ui.name],
    ["Colours", mode === "system" ? "Match my system" : mode === "dark" ? "Dark" : "Light", THEMES.find((t) => t.id === theme)?.name ?? ""],
    ["Performance", `${perf.name}${pickedPerf ? "" : " (recommended)"}`, perf.recommendedFor],
    ["Remember folder sizes", yes(adv(settings, "files.sizeCache")), ""],
    ["Keep thumbnails", yes(adv(settings, "cache.thumbnails")), ""],
    ["Save computer details", yes(adv(settings, "cache.hardware")), ""],
    ["Learn folders you open next", yes(adv(settings, "cache.predictions")), ""],
    ["Previous searches", adv<number>(settings, "search.historyLimit") > 0 ? `Last ${adv<number>(settings, "search.historyLimit")}` : "Off", ""],
    ["Hide server addresses", yes(settings.appearance.hideNetworkAddress), ""],
  ];

  return (
    <div className="setup-backdrop" role="dialog" aria-modal="true" aria-label="Welcome to FAST File Explorer">
      <div className="setup" data-step={STEPS[step]}>
        <div className="setup-top">
          <ol className="setup-steps" aria-label="Setup progress">
            {STEPS.map((name, i) => (
              <li key={name} data-state={i === step ? "current" : i < step ? "done" : "todo"}>
                <button onClick={() => go(i)} aria-current={i === step ? "step" : undefined} title={name}>
                  <span className="setup-dot">{i < step ? "✓" : i + 1}</span>
                  <span className="setup-step-name">{name}</span>
                </button>
              </li>
            ))}
          </ol>
          <button className="link-button" onClick={skip}>
            Skip setup
          </button>
        </div>

        <div className="setup-view" key={STEPS[step]}>
          {step === 0 && (
            <>
              <h1>Welcome to FAST File Explorer</h1>
              <p className="muted">
                A few quick choices to make it yours, one topic at a time. Use <b>Next</b> and <b>Back</b> or the left and right arrow keys. At the end you will see everything you chose before it is saved.
              </p>
              {hw && (
                <div className="setup-hw">
                  <strong>This computer</strong>
                  <span>
                    {hw.cpus} processor threads · {(hw.totalRamMb / 1024).toFixed(hw.totalRamMb >= 10240 ? 0 : 1)} GB of memory{hw.batteryPercent != null ? " · has a battery" : ""}
                  </span>
                  <span className="muted">Recommendations on the next views are based on this.</span>
                </div>
              )}
            </>
          )}
          {step === 1 && (
            <>
              <h2>How should it look and work?</h2>
              <p className="muted">Pick the file manager you are used to. This sets the window layout, colours, icons and keyboard shortcuts together.</p>
              <UiPresetsPanel compact />
            </>
          )}
          {step === 2 && (
            <>
              <h2>Light or dark?</h2>
              <div className="segmented" role="radiogroup" aria-label="Light or dark">
                {(["system", "dark", "light"] as const).map((m) => (
                  <button key={m} role="radio" aria-checked={mode === m} data-on={mode === m || undefined} onClick={() => pickMode(m)}>
                    {m === "system" ? "Match my system" : m === "dark" ? "Dark" : "Light"}
                    {m === "system" && <span className="pill">Recommended</span>}
                  </button>
                ))}
              </div>
              <p className="muted">The colours follow the interface style you chose. You can pick any of the other themes later in Settings.</p>
            </>
          )}
          {step === 3 && (
            <>
              <h2>How much should it use of this computer?</h2>
              <p className="muted">More means faster search and quicker folders, at the cost of memory and background work.</p>
              <PerfPresetsPanel compact />
            </>
          )}
          {step === 4 && (
            <>
              <h2>What may it remember?</h2>
              <p className="muted">Remembering makes things faster. Turn any off and nothing of that kind is saved.</p>
              <Choice title="Folder sizes" hint="Show a folder's size at once next time, then check it again quietly." recommended on={adv(settings, "files.sizeCache")} onToggle={(v) => set("files.sizeCache", v)} />
              <Choice title="Thumbnails" hint="Keep small previews of pictures on disk so folders of images open at once." recommended on={adv(settings, "cache.thumbnails")} onToggle={(v) => set("cache.thumbnails", v)} />
              <Choice title="Details about this computer" hint="Save the hardware and drive-health details shown in About so it opens at once." recommended on={adv(settings, "cache.hardware")} onToggle={(v) => set("cache.hardware", v)} />
              <Choice title="Folders you open next" hint="Learn your habits to get the next folder ready early." recommended on={adv(settings, "cache.predictions")} onToggle={(v) => set("cache.predictions", v)} />
              <Choice
                title="Previous searches"
                hint="Show recent searches under the search box."
                recommended
                on={adv<number>(settings, "search.historyLimit") > 0}
                onToggle={(v) => set("search.historyLimit", v ? 20 : 0)}
              />
            </>
          )}
          {step === 5 && (
            <>
              <h2>Privacy</h2>
              <Choice
                title="Hide server addresses"
                hint="Network drives show a friendly name instead of an IP address or host, in names, tabs and tooltips."
                recommended
                on={settings.appearance.hideNetworkAddress}
                onToggle={(v) => updateSettings((s) => void (s.appearance.hideNetworkAddress = v))}
              />
              <p className="muted">You can also lock folders and drives with a passcode later: right-click one and choose “Lock with passcode…” (the last item in the menu).</p>
            </>
          )}
          {step === 6 && (
            <>
              <h2>Here is what you chose</h2>
              <table className="setup-summary">
                <tbody>
                  {summary.map(([k, v, note]) => (
                    <tr key={k}>
                      <th scope="row">{k}</th>
                      <td>
                        <strong>{v}</strong>
                        {note && <span className="muted"> {note}</span>}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </>
          )}
        </div>

        <footer>
          {last && (
            <p className="setup-note" role="note">
              All of this can be changed at any time in Settings, at the top right (or press Ctrl+,).
            </p>
          )}
          <div className="setup-buttons">
            <button className="secondary" onClick={() => go(step - 1)} disabled={step === 0}>
              ← Back
            </button>
            <span className="grow" />
            {last ? (
              <button className="primary" onClick={confirm} autoFocus>
                Confirm and start
              </button>
            ) : (
              <button className="primary" onClick={() => go(step + 1)}>
                Next →
              </button>
            )}
          </div>
        </footer>
      </div>
    </div>
  );
}
