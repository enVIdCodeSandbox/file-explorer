import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { autoLevel, planFor, unloadLevelOf, startUnloader, isSuspended, rememberScrolls, restoreScrolls } from "./unload";

const settings = (maxCacheMb: number, advanced: Record<string, unknown> = {}) => ({ performance: { maxCacheMb }, advanced }) as never;

describe("unload level", () => {
  it("follows the performance level when automatic: small caches unload sooner, big ones never", () => {
    expect(autoLevel(settings(16))).toBe("aggressive");
    expect(autoLevel(settings(64))).toBe("aggressive");
    expect(autoLevel(settings(128))).toBe("balanced");
    expect(autoLevel(settings(512))).toBe("balanced");
    expect(autoLevel(settings(1024))).toBe("light");
    expect(autoLevel(settings(2048))).toBe("off");
    expect(autoLevel(settings(0))).toBe("off");
  });
  it("a phone is never left on 'never'", () => {
    expect(autoLevel(settings(0), true)).toBe("light");
    expect(autoLevel(settings(256), true)).toBe("balanced");
  });
  it("a level chosen in settings wins over the performance level", () => {
    expect(unloadLevelOf(settings(4096, { "perf.uiUnload": "aggressive" }))).toBe("aggressive");
    expect(unloadLevelOf(settings(16, { "perf.uiUnload": "off" }))).toBe("off");
    expect(unloadLevelOf(settings(16, { "perf.uiUnload": "auto" }))).toBe("aggressive");
    expect(unloadLevelOf(settings(16, { "perf.uiUnload": "nonsense" }))).toBe("aggressive");
  });
  it("waits longer at lighter levels, and 'off' never unloads", () => {
    expect(planFor("off").afterMs).toBeNull();
    expect(planFor("light").afterMs!).toBeGreaterThan(planFor("balanced").afterMs!);
    expect(planFor("balanced").afterMs!).toBeGreaterThan(planFor("aggressive").afterMs!);
  });
});

describe("the unloader", () => {
  let state: "visible" | "hidden";
  let handler: (() => void) | undefined;
  const doc = {
    get visibilityState() {
      return state;
    },
    addEventListener: (_: string, f: () => void) => void (handler = f),
    removeEventListener: () => void (handler = undefined),
    querySelectorAll: () => [],
    querySelector: () => null,
  } as unknown as Document;
  beforeEach(() => {
    vi.useFakeTimers();
    state = "visible";
  });
  afterEach(() => vi.useRealTimers());

  const go = (to: "visible" | "hidden") => {
    state = to;
    handler!();
  };

  it("takes the interface down after the wait, and puts it back when the window shows", () => {
    const calls: string[] = [];
    const stop = startUnloader(
      { settings: () => settings(16), mobile: () => false, before: () => calls.push("before"), after: () => calls.push("after"), dropPictures: () => calls.push("pictures") },
      doc,
    );
    go("hidden");
    vi.advanceTimersByTime(9_000);
    expect(isSuspended()).toBe(false);
    vi.advanceTimersByTime(2_000);
    expect(isSuspended()).toBe(true);
    expect(calls).toEqual(["before", "pictures"]);
    go("visible");
    expect(isSuspended()).toBe(false);
    expect(calls.at(-1)).toBe("after");
    stop();
  });

  it("coming back in time cancels it, and never unloads at 'off'", () => {
    const stop = startUnloader({ settings: () => settings(16), mobile: () => false }, doc);
    go("hidden");
    vi.advanceTimersByTime(5_000);
    go("visible");
    vi.advanceTimersByTime(60_000);
    expect(isSuspended()).toBe(false);
    stop();
    const stop2 = startUnloader({ settings: () => settings(16, { "perf.uiUnload": "off" }), mobile: () => false }, doc);
    go("hidden");
    vi.advanceTimersByTime(3_600_000);
    expect(isSuspended()).toBe(false);
    stop2();
  });

  it("scroll helpers do nothing without areas to keep", () => {
    expect(() => {
      rememberScrolls(doc);
      restoreScrolls(doc);
    }).not.toThrow();
  });
});

describe("keeping the selection", () => {
  it("hands back only the names inside the folder, and more than once", async () => {
    const { keepSelection, takeSelection } = await import("./unload");
    const parent = (p: string) => p.slice(0, p.lastIndexOf("/"));
    const name = (p: string) => p.slice(p.lastIndexOf("/") + 1);
    keepSelection(["/a/x.txt", "/a/y.txt", "/b/z.txt"]);
    expect(takeSelection("/a", parent, name)).toEqual(["x.txt", "y.txt"]);
    expect(takeSelection("/a", parent, name)).toEqual(["x.txt", "y.txt"]);
    expect(takeSelection("/c", parent, name)).toBeNull();
    keepSelection([]);
    expect(takeSelection("/a", parent, name)).toBeNull();
  });
});
