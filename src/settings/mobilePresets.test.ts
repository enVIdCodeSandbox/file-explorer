import { describe, expect, it } from "vitest";
import { MOBILE_PERF_PRESETS, PERF_PRESETS, UI_PRESETS, mobilePlatformFor, perfPresetsFor, platformFor, recommendMobilePerf, uiPresetsFor } from "./presets";
import { asChrome, asPhoneChrome } from "../util/chrome";
import { makerFromUserAgent } from "../util/native";

const num = (p: (typeof PERF_PRESETS)[number], key: string) => Number(p.patch[key]);

describe("phone performance presets", () => {
  it("are fewer than the computer ones and go from light to heavy", () => {
    expect(MOBILE_PERF_PRESETS.length).toBeLessThan(PERF_PRESETS.length);
    expect(perfPresetsFor(true)).toBe(MOBILE_PERF_PRESETS);
    expect(perfPresetsFor(false)).toBe(PERF_PRESETS);
    for (let i = 1; i < MOBILE_PERF_PRESETS.length; i++) {
      const [a, b] = [MOBILE_PERF_PRESETS[i - 1], MOBILE_PERF_PRESETS[i]];
      expect(b.minRamGb).toBeGreaterThan(a.minRamGb);
      expect(b.caps.cpu).toBeGreaterThan(a.caps.cpu);
      expect(b.caps.memory).toBeGreaterThanOrEqual(a.caps.memory);
      expect(num(b, "performance.maxCacheMb")).toBeGreaterThan(num(a, "performance.maxCacheMb"));
    }
  });

  it("are much lighter than the computer preset for the same memory", () => {
    for (const m of MOBILE_PERF_PRESETS) {
      // The computer preset someone with this much memory would be offered.
      const pc = [...PERF_PRESETS].reverse().find((p) => p.minRamGb <= Math.max(m.minRamGb, 2)) ?? PERF_PRESETS[0];
      expect(m.caps.cpu).toBeLessThanOrEqual(pc.caps.cpu || 100);
      expect(m.caps.memory).toBeLessThanOrEqual(pc.caps.memory || 100);
    }
    const heaviest = MOBILE_PERF_PRESETS[MOBILE_PERF_PRESETS.length - 1];
    const average = PERF_PRESETS.find((p) => p.name === "Average")!;
    expect(heaviest.caps.cpu).toBeLessThan(average.caps.cpu);
    expect(heaviest.caps.memory).toBeLessThan(average.caps.memory);
    expect(num(heaviest, "performance.maxCacheMb")).toBeLessThan(num(average, "performance.maxCacheMb"));
  });

  it("always save the battery and give memory back", () => {
    for (const p of MOBILE_PERF_PRESETS) {
      const text = JSON.stringify(p.patch);
      expect(text).not.toContain('"performance.reduceOnBattery":false');
      expect(text).not.toContain('"advanced.perf.compactWhenIdle":false');
    }
  });

  it("have ids no computer preset uses", () => {
    const ids = new Set(PERF_PRESETS.map((p) => p.id));
    for (const p of MOBILE_PERF_PRESETS) expect(ids.has(p.id)).toBe(false);
  });

  it("recommends by what the phone reports, which is a little under the size on the box", () => {
    expect(recommendMobilePerf(1900, 4).name).toBe("Essential");
    expect(recommendMobilePerf(2800, 8).name).toBe("Light");
    expect(recommendMobilePerf(3700, 8).name).toBe("Balanced");
    expect(recommendMobilePerf(5600, 8).name).toBe("Smooth");
    expect(recommendMobilePerf(7400, 8).name).toBe("Fast");
    expect(recommendMobilePerf(11500, 8).name).toBe("Maximum");
    expect(recommendMobilePerf(15500, 8).name).toBe("Maximum");
    // Plenty of memory but few cores: the cores decide.
    expect(recommendMobilePerf(11500, 4).name).toBe("Light");
  });
});

describe("phone looks", () => {
  it("are offered on phones only, and the computer looks on computers only", () => {
    expect(uiPresetsFor(true).map((p) => p.id)).toEqual(["google", "samsung", "apple"]);
    expect(uiPresetsFor(false).map((p) => p.id)).toEqual(["windows", "macos", "linux"]);
    for (const p of UI_PRESETS) expect(p.patch["advanced.ui.chrome"]).toBe(p.id);
  });

  it("match the device", () => {
    expect(platformFor("ios")).toBe("apple");
    expect(platformFor("android", "samsung")).toBe("samsung");
    expect(platformFor("android", "Google")).toBe("google");
    expect(platformFor("android")).toBe("google");
    expect(platformFor("windows", "samsung")).toBe("windows");
    expect(mobilePlatformFor("macos")).toBe("apple");
    expect(mobilePlatformFor("windows")).toBe("google");
  });

  it("fall back to the nearest arrangement when a look from the other kind of device is saved", () => {
    expect(asPhoneChrome("windows")).toBe("google");
    expect(asPhoneChrome("macos")).toBe("apple");
    expect(asPhoneChrome("samsung")).toBe("samsung");
    expect(asChrome("apple")).toBe("macos");
    expect(asChrome("google")).toBe("windows");
    expect(asChrome("linux")).toBe("linux");
  });

  it("tells a Galaxy from other phones by how the browser describes it", () => {
    expect(makerFromUserAgent("Mozilla/5.0 (Linux; Android 14; SM-S926U Build/UP1A; wv) AppleWebKit/537.36")).toBe("samsung");
    expect(makerFromUserAgent("Mozilla/5.0 (Linux; Android 15; Pixel 9 Build/AP3A; wv) AppleWebKit/537.36")).toBe("google");
    expect(makerFromUserAgent("Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)")).toBe("apple");
    expect(makerFromUserAgent("Mozilla/5.0 (Linux; Android 13; moto g power)")).toBe("");
  });
});
