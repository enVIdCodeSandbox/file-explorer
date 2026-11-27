import { BAR_SCALE_LIMITS, UI_ICON_LIMITS, clampBarScale, clampUiIconSize } from "./limits";

export type ZoomableBar = "top" | "left";

/** Identify whether a DOM element belongs to the top chrome bar or the left navigation bar. */
export function barAtElement(target: EventTarget | Element | null | undefined): ZoomableBar | null {
  if (!target) return null;
  const raw = target as { closest?: Element["closest"]; parentElement?: Element | null };
  const el = typeof raw.closest === "function" ? (raw as Element) : raw.parentElement ?? null;
  if (!el || typeof el.closest !== "function") return null;
  if (el.closest(".term-body, .xterm, .context-menu, .dialog, .tooltip, .popout-panel")) return null;
  // Left bar: vertical navigation sidebar (or left-docked command strip / vertical tab bar).
  if (el.closest(".dock-left .sidebar-wrap, .sidebar-wrap:not([data-horizontal]), .dock-left .command-bar, .dock-left .title-bar")) {
    return "left";
  }
  // Top bar: title/caption strip, tab bar, Quick Access toolbar, toolbar/address bar, ribbon, or top-docked command bar.
  if (el.closest(".caption-strip, .title-bar:not([data-vertical]), .dock-top .toolbar, .dock-top .command-bar, .dock-top .win10-ribbon, .win10-ribbon, .win10-qat, .toolbar, .command-bar:not([data-vertical]), .tab-strip:not([data-vertical])")) {
    return "top";
  }
  return null;
}

/** Step a bar scale percentage up/down (`steps > 0` zooms in, `0` resets to 100%). */
export function stepBarScale(current: number, steps: number): number {
  if (steps === 0) return BAR_SCALE_LIMITS.default;
  const base = Number.isFinite(current) ? current : BAR_SCALE_LIMITS.default;
  return clampBarScale(base + steps * BAR_SCALE_LIMITS.step);
}

/** Step the global icon size in pixels (`steps > 0` grows icons, `0` resets to default). */
export function stepIconSize(current: number, steps: number, fallback = UI_ICON_LIMITS.default): number {
  if (steps === 0) return clampUiIconSize(fallback);
  const base = Number.isFinite(current) ? current : fallback;
  return clampUiIconSize(base + steps * UI_ICON_LIMITS.step);
}
