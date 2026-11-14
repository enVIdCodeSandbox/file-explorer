import { describe, it, expect, vi } from "vitest";

vi.mock("../api", async (orig) => {
  const mod = await orig<typeof import("../api")>();
  return { ...mod, api: new Proxy({}, { get: () => () => Promise.resolve(undefined) }) };
});

describe("sort, filter and view memory", () => {
  it("keeps the order across navigation and saves it, and the filter stays unless turned off", async () => {
    const { useStore } = await import("../store");
    const base = { general: {}, appearance: {}, advanced: {}, profiles: [] } as never;
    useStore.setState({ settings: base, tabs: [{ id: "t", location: "/a", history: ["/a"], historyIndex: 0 }] as never, activeTab: "t" });
    useStore.getState().setSort({ key: "modified", asc: false });
    expect(useStore.getState().settings!.advanced["files.sortKey"]).toBe("modified");
    expect(useStore.getState().settings!.advanced["files.sortAsc"]).toBe(false);
    // The filter is kept by default, but "include subfolders" (a search of the old folder) is not.
    useStore.getState().setFilter({ text: "x", subfolders: true });
    useStore.getState().navigate("/b");
    expect(useStore.getState().sort).toEqual({ key: "modified", asc: false });
    expect(useStore.getState().filter.text).toBe("x");
    expect(useStore.getState().filter.subfolders).toBe(false);
    useStore.getState().updateSettings((s) => void (s.advanced["files.keepFilter"] = false));
    useStore.getState().navigate("/c");
    expect(useStore.getState().filter.text).toBe("");
    // Changed in Settings -> applied live.
    useStore.getState().updateSettings((s) => void (s.advanced["files.sortKey"] = "size"));
    expect(useStore.getState().sort.key).toBe("size");
  });

  it("keeps the view in every folder, or only in the open one when turned off", async () => {
    const { useStore } = await import("../store");
    const { viewLevelOf } = await import("./viewLevel");
    const base = { general: {}, appearance: { viewLevel: 0 }, advanced: {}, profiles: [] } as never;
    useStore.setState({ settings: base, folderView: null, tabs: [{ id: "t", location: "/a", history: ["/a"], historyIndex: 0 }] as never, activeTab: "t" });
    useStore.getState().setViewLevel(3);
    expect(useStore.getState().settings!.appearance.viewLevel).toBe(3);
    useStore.getState().navigate("/b");
    expect(viewLevelOf(useStore.getState().settings, useStore.getState().folderView)).toBe(3);
    // Off: the picked view lasts for one folder.
    useStore.getState().updateSettings((s) => void (s.advanced["files.rememberView"] = false));
    useStore.getState().setViewLevel(5);
    expect(useStore.getState().settings!.appearance.viewLevel).toBe(3);
    expect(viewLevelOf(useStore.getState().settings, useStore.getState().folderView)).toBe(5);
    useStore.getState().navigate("/c");
    expect(viewLevelOf(useStore.getState().settings, useStore.getState().folderView)).toBe(3);
  });
});
