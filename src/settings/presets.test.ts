import { describe, expect, it } from "vitest";
import type { Settings } from "../api";
import { ADVANCED } from "./advanced";
import { KEY_COMMANDS } from "./keys";
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
  snapshotOf,
} from "./presets";
import { THEMES } from "../util/themes";
import { getSetting } from "./schema";

const blank = (): Settings =>
  ({
    general: { showHidden: false },
    appearance: { theme: "dark", accent: "#4cc2ff", density: "compact", hideNetworkAddress: true, layout: {}, style: { icons: "outline", radius: 8, menu: "glass", folderColor: "classic", font: "system" }, showDetailsPane: true },
    performance: { priority: "dynamic", maxThreads: 0, maxCacheMb: 256, backgroundIndexing: true, indexDrives: true, lazyLoading: true, metadataCaching: true, thumbnails: true, thumbnailSize: 96, predictivePrefetch: true, reduceOnBattery: true },
    transfer: {},
    search: {},
    contextMenu: { hidden: [] },
    terminal: {},
    advanced: {},
    keys: {},
    customActions: [],
    profiles: [],
    activeProfile: "",
    setupDone: false,
  }) as unknown as Settings;

describe("performance presets", () => {
  it("has the twelve named steps in order", () => {
    expect(PERF_PRESETS.map((p) => p.name)).toEqual([
      "Barebones", "Feather", "Reduced", "Acceptable", "Average", "Satisfactory", "Decent", "Performance", "Power", "Excelling", "Exceptional", "God-like",
    ]);
    for (const p of PERF_PRESETS) {
      expect(p.description.length).toBeGreaterThan(20);
      expect(p.recommendedFor.length).toBeGreaterThan(20);
    }
  });
  it("only sets real settings", () => {
    const known = new Set(ADVANCED.map((a) => `advanced.${a.key}`));
    for (const p of PERF_PRESETS) {
      for (const key of Object.keys(p.patch)) {
        if (key.startsWith("advanced.")) expect(known.has(key), `${p.id}: ${key}`).toBe(true);
        else expect(getSetting(blank(), key), `${p.id}: ${key}`).not.toBeUndefined();
      }
    }
  });
  it("grows the cache and thread limit step by step", () => {
    const caches = PERF_PRESETS.map((p) => p.patch["performance.maxCacheMb"] as number);
    expect([...caches].sort((a, b) => a - b)).toEqual(caches);
    const threads = PERF_PRESETS.slice(0, -1).map((p) => p.patch["performance.maxThreads"] as number);
    expect([...threads].sort((a, b) => a - b)).toEqual(threads);
  });
  it("recommends by memory and cores, and a step lower for laptops", () => {
    expect(recommendPerf(1024, 1, false).id).toBe("barebones");
    expect(recommendPerf(16 * 1024, 8, false).id).toBe("performance");
    expect(recommendPerf(16 * 1024, 8, true).id).toBe("decent");
    expect(recommendPerf(128 * 1024, 64, false).id).toBe("godlike");
    expect(recommendPerf(128 * 1024, 4, false).id).not.toBe("godlike"); // cores limit it too
    expect(["performance", "decent", "satisfactory"]).toContain(recommendPerf(256 * 1024, 64, true).id);
  });
  it("is recognised once applied", () => {
    const s = blank();
    expect(currentPerfPreset(s)).toBeUndefined();
    applyPatch(s, PERF_PRESETS[4].patch);
    expect(currentPerfPreset(s)?.id).toBe("average");
    s.performance.maxThreads = 3;
    expect(currentPerfPreset(s)).toBeUndefined();
  });
});

describe("interface presets", () => {
  it("covers the three platforms with real themes and shortcut ids", () => {
    expect(UI_PRESETS.map((p) => p.id)).toEqual(["windows", "macos", "linux"]);
    const ids = new Set(THEMES.map((t) => t.id));
    const commands = new Set(KEY_COMMANDS.map((k) => k.id));
    for (const p of UI_PRESETS) {
      expect(ids.has(p.themes.dark) && ids.has(p.themes.light)).toBe(true);
      for (const id of Object.keys((p.patch.keys as Record<string, string>) ?? {})) expect(commands.has(id), `${p.id}: ${id}`).toBe(true);
    }
  });
  it("applies the dark or light variant and swaps shortcuts", () => {
    const s = blank();
    applyUiPreset(s, UI_PRESETS[1], true);
    expect(s.appearance.theme).toBe("mac-dark");
    expect(s.keys.rename).toBe("Enter");
    expect(s.advanced["ui.windowControlsSide"]).toBe("left");
    applyUiPreset(s, UI_PRESETS[0], false);
    expect(s.keys).toEqual({});
    expect(s.appearance.style.icons).toBe("fluent");
    expect(differences(s, UI_PRESETS[0].patch)).toEqual([]);
  });
  it("recognises an applied preset even when saved keys come back in another order", () => {
    const s = blank();
    applyUiPreset(s, UI_PRESETS[2], true);
    // The backend returns object keys alphabetically.
    const sortDeep = (o: unknown): unknown => (o && typeof o === "object" && !Array.isArray(o) ? Object.fromEntries(Object.entries(o).sort(([a], [b]) => b.localeCompare(a)).map(([k, v]) => [k, sortDeep(v)])) : o);
    s.appearance.layout = sortDeep(s.appearance.layout);
    expect(differences(s, UI_PRESETS[2].patch)).toEqual([]);
  });
  it("gives no two shortcuts the same key within a preset", () => {
    for (const p of UI_PRESETS) {
      const keys = { ...Object.fromEntries(KEY_COMMANDS.map((k) => [k.id, k.default])), ...((p.patch.keys as Record<string, string>) ?? {}) };
      const seen = new Map<string, string>();
      for (const [id, combo] of Object.entries(keys)) {
        expect(seen.has(combo), `${p.id}: ${id} and ${seen.get(combo)} both use ${combo}`).toBe(false);
        seen.set(combo, id);
      }
    }
  });
  it("picks the style that matches the operating system", () => {
    expect(platformFor("macos")).toBe("macos");
    expect(platformFor("linux")).toBe("linux");
    expect(platformFor("windows")).toBe("windows");
    expect(platformFor(undefined)).toBe("windows");
  });
});

describe("profiles", () => {
  it("round-trips settings and leaves the profile list out of a snapshot", () => {
    const s = blank();
    s.appearance.theme = "nord";
    s.advanced = { "ui.zebra": true };
    const p = newProfile(s, "Work");
    expect(p.snapshot).not.toHaveProperty("profiles");
    expect(p.snapshot).not.toHaveProperty("setupDone");
    const other = blank();
    other.appearance.theme = "light";
    other.advanced = { "sidebar.folded": "x" };
    applyProfile(other, p);
    expect(other.appearance.theme).toBe("nord");
    expect(other.advanced["ui.zebra"]).toBe(true);
    expect(other.advanced["sidebar.folded"]).toBe("x"); // window-local state survives
    expect(other.activeProfile).toBe(p.id);
    expect(other.profiles).toEqual([]);
  });
  it("keeps an unlimited number with distinct ids", () => {
    const s = blank();
    const ids = new Set(Array.from({ length: 200 }, (_, i) => newProfile(s, `P${i}`).id));
    expect(ids.size).toBe(200);
    expect(snapshotOf(s)).not.toBe(s);
  });
});
