import { describe, it, expect, vi, beforeEach } from "vitest";
import type { Settings } from "../api";
import { autoShellLevel, shellCacheLevel, shellCacheSeconds, shellPrefetch } from "./shellCache";

const st = (maxCacheMb: number, advanced: Record<string, unknown> = {}) => ({ performance: { maxCacheMb }, advanced, appearance: {}, general: {}, profiles: [] }) as unknown as Settings;

describe("right-click menu cache settings", () => {
  it("automatic follows the performance level's cache size", () => {
    expect(autoShellLevel(st(0))).toBe("long");
    expect(autoShellLevel(st(4096))).toBe("long");
    expect(autoShellLevel(st(1024))).toBe("normal");
    expect(autoShellLevel(st(256))).toBe("short");
    expect(shellCacheSeconds(st(4096))).toBe(1800);
    expect(shellCacheSeconds(st(1024))).toBe(300);
    expect(shellCacheSeconds(st(64))).toBe(60);
  });

  it("a chosen level wins, and off remembers nothing and prefetches nothing", () => {
    expect(shellCacheLevel(st(4096, { "perf.shellMenuCache": "short" }))).toBe("short");
    expect(shellCacheSeconds(st(4096, { "perf.shellMenuCache": "off" }))).toBe(0);
    expect(shellPrefetch(st(4096, { "perf.shellMenuCache": "off", "perf.shellMenuPrefetch": "on" }))).toBe(false);
    expect(shellCacheLevel(st(4096, { "perf.shellMenuCache": "nonsense" }))).toBe("long");
  });

  it("prefetch: automatic is on except on the lightest levels; on/off override", () => {
    expect(shellPrefetch(st(4096))).toBe(true);
    expect(shellPrefetch(st(1024))).toBe(true);
    expect(shellPrefetch(st(128))).toBe(false);
    expect(shellPrefetch(st(128, { "perf.shellMenuPrefetch": "on" }))).toBe(true);
    expect(shellPrefetch(st(4096, { "perf.shellMenuPrefetch": "off" }))).toBe(false);
  });
});

describe("remembered system entries", () => {
  beforeEach(() => vi.resetModules());

  it("peek returns entries only while they are young enough, and sameShell spots an unchanged refresh", async () => {
    const calls: unknown[][] = [];
    const items = [{ id: "7", label: "7-Zip", disabled: false, checked: false, iconUrl: null, children: [{ id: "8", label: "Add to archive", disabled: false, checked: false, iconUrl: null, children: [] }] }];
    vi.doMock("../api", async (orig) => {
      const mod = await orig<typeof import("../api")>();
      return { ...mod, api: { shellMenu: (...a: unknown[]) => (calls.push(a), Promise.resolve(items)), shellInvoke: () => Promise.resolve() } };
    });
    const { useStore } = await import("../store");
    useStore.setState({ settings: st(4096) });
    const { peekShell, prefetchShell, sameShell, useShellItems } = await import("../components/shellMenu");
    expect(peekShell(["/a/b.txt"], false)).toBeNull();
    const first = await useShellItems().fetchFor(["/a/b.txt"], false);
    expect(calls[0]).toEqual([["/a/b.txt"], false, 1800]);
    const again = peekShell(["/a/b.txt"], false)!;
    expect(again.map((m) => m.label)).toEqual(["7-Zip"]);
    expect(sameShell(again, first)).toBe(true);
    expect(sameShell(null, first)).toBe(false);
    expect(peekShell(["/a/other.txt"], false)).toBeNull();
    expect(peekShell(["/a/b.txt"], true)).toBeNull(); // the folder's own menu is a different one
    prefetchShell(["/a/c.txt"], false);
    await Promise.resolve();
    expect(calls.length).toBe(2);
    // Older than the setting allows: not shown any more.
    vi.useFakeTimers();
    vi.setSystemTime(Date.now() + 1801 * 1000);
    expect(peekShell(["/a/b.txt"], false)).toBeNull();
    vi.useRealTimers();
    // Never remember: nothing kept.
    useStore.setState({ settings: st(4096, { "perf.shellMenuCache": "off" }) });
    await useShellItems().fetchFor(["/a/z.txt"], false);
    expect(peekShell(["/a/z.txt"], false)).toBeNull();
  });
});
