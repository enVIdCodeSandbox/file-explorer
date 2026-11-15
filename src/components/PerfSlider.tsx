// The performance presets as one slider: drag (or use the arrow keys) from the
// lightest step to the heaviest and read what the step under the thumb does.
// Used by the first-run setup and by Settings > Presets and profiles.

import type { CSSProperties, ReactNode } from "react";
import { useT } from "../i18n";
import { capsText, type PerfPreset } from "../settings/presets";

interface Props {
  presets: PerfPreset[];
  /** The step shown under the thumb. */
  index: number;
  /** The step recommended for this device. */
  recIndex: number;
  onPick: (index: number) => void;
  /** A phone or tablet: the wording says "device" and names phones at the two ends. */
  mobile: boolean;
  /** The step in use now, marked on the track, when that differs from what is shown. */
  usedIndex?: number;
  /** Shown under the description (a button to apply the step, the values it sets). */
  children?: ReactNode;
}

export function PerfSlider({ presets, index, recIndex, onPick, mobile, usedIndex, children }: Props) {
  const t = useT();
  const perf = presets[index];
  return (
    <div className="perf-slider">
      <div className="perf-wrap" style={{ "--n": presets.length - 1, "--i": index, "--r": recIndex } as CSSProperties}>
        <div className="perf-rec" data-edge={recIndex === presets.length - 1 ? "end" : recIndex === 0 ? "start" : undefined}>
          <span>{t("Recommended")}</span>
          <svg viewBox="0 0 24 14" width="26" height="15" aria-hidden="true"><path d="M2 1h20L12 13z" fill="currentColor" /></svg>
        </div>
        <input
          type="range"
          className="perf-range"
          min={0}
          max={presets.length - 1}
          step={1}
          value={index}
          aria-label={t("Performance level")}
          aria-valuetext={perf.name}
          onChange={(e) => onPick(Number(e.target.value))}
        />
        <div className="perf-ticks" aria-hidden="true">
          {presets.map((p, i) => (
            <i key={p.id} data-on={i <= index || undefined} data-used={i === usedIndex || undefined} />
          ))}
        </div>
      </div>
      <div className="perf-readout">
        <div className="perf-end">
          <b>{t("Light")}</b>
          <span>{mobile ? t("Older and smaller phones. Saves memory and battery.") : t("Old or small devices. Saves memory and battery.")}</span>
        </div>
        <div className="perf-now" aria-live="polite">
          <span className="perf-step">{t("Step {i} of {n}", { i: index + 1, n: presets.length })}</span>
          <h3>{perf.name}</h3>
          <span className="perf-pills">
            {index === recIndex && <span className="pill">{mobile ? t("Recommended for this device") : t("Recommended for this computer")}</span>}
            {index === usedIndex && <span className="pill" data-kind="current">{t("In use")}</span>}
          </span>
        </div>
        <div className="perf-end right">
          <b>{t("Heavy")}</b>
          <span>{mobile ? t("Flagship phones and tablets. Fastest search and folders.") : t("Powerful PCs. Fastest search and folders.")}</span>
        </div>
      </div>
      <div className="perf-detail">
        <p>{perf.description}</p>
        <p className="muted">
          <b>{t("Suits:")}</b> {perf.recommendedFor}
        </p>
        <p className="muted perf-caps">{capsText(perf)}</p>
        {children}
      </div>
    </div>
  );
}
