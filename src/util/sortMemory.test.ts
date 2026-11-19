import { describe, it, expect, vi } from "vitest";

vi.mock("../api", async (orig) => {
  const mod = await orig<typeof import("../api")>();
  return { ...mod, api: new Proxy({}, { get: () => () => Promise.resolve(undefined) }) };
});

describe("sort, filter and view memory", () => {
  it("with views not kept per folder: keeps the order across navigation and saves it, and the filter stays unless turned off", async () => {
    const { useStore } = await import("../store");
    const base = { general: {}, appearance: {}, advanced: { "files.perFolderView": false }, profiles: [] } as never;
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

  it("keeps the order, filter and columns of each folder by default", async () => {
    const { useStore } = await import("../store");
    const { DEFAULT_SET } = await import("./columns");
    const base = { general: {}, appearance: {}, advanced: {}, profiles: [] } as never;
    useStore.setState({ ready: true, settings: base, folderViews: {}, columnSet: DEFAULT_SET, tabs: [{ id: "t", location: "/a", history: ["/a"], historyIndex: 0 }] as never, activeTab: "t" });
    useStore.getState().setSort({ key: "modified", asc: false });
    useStore.getState().setFilter({ text: "x" });
    useStore.getState().setColumns({ order: ["name", "size", "accessed"], widths: { size: 90 } }, "here");
    // Nothing leaks into the defaults for other folders…
    expect(useStore.getState().settings!.advanced["files.sortKey"]).toBeUndefined();
    expect(useStore.getState().columnSet).toEqual(DEFAULT_SET);
    useStore.getState().navigate("/b");
    expect(useStore.getState().sort).toEqual({ key: "name", asc: true });
    expect(useStore.getState().filter.text).toBe("");
    // …and the first folder has what it was given when you come back.
    useStore.getState().navigate("/a");
    expect(useStore.getState().sort).toEqual({ key: "modified", asc: false });
    expect(useStore.getState().filter.text).toBe("x");
    expect(useStore.getState().folderViews["/a"]?.columns?.order).toEqual(["name", "size", "accessed"]);
    // "All folders" clears the per-folder columns and sets the shared ones.
    useStore.getState().setColumns({ order: ["name", "type"], widths: {} }, "all");
    expect(useStore.getState().columnSet.order).toEqual(["name", "type"]);
    expect(useStore.getState().folderViews["/a"]?.columns).toBeUndefined();
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

describe("shared or per folder, for each of sort, filter, columns and view", () => {
  const fresh = async (advanced: Record<string, unknown>) => {
    const { useStore } = await import("../store");
    const base = { general: {}, appearance: { viewLevel: 0 }, advanced, profiles: [] } as never;
    useStore.setState({ settings: base, folderViews: {}, folderView: null, ready: true, tabs: [{ id: "t", location: "/a", history: ["/a"], historyIndex: 0 }] as never, activeTab: "t" });
    return useStore;
  };

  it("older options still decide until a choice is made", async () => {
    const { scopeOf } = await import("./folderMemory");
    const s = (advanced: Record<string, unknown>) => ({ advanced }) as never;
    expect(scopeOf(s({}), "sort")).toBe("folder");
    expect(scopeOf(s({}), "view")).toBe("shared");
    expect(scopeOf(s({ "files.perFolderView": false }), "filter")).toBe("shared");
    expect(scopeOf(s({ "files.rememberView": false }), "view")).toBe("folder");
    expect(scopeOf(s({ "files.scope.sort": "shared", "files.perFolderView": true }), "sort")).toBe("shared");
  });

  it("each aspect can be chosen on its own: sort per folder, view shared", async () => {
    const useStore = await fresh({ "files.scope.sort": "folder", "files.scope.view": "shared", "files.scope.filter": "shared" });
    useStore.getState().setSort({ key: "size", asc: false });
    useStore.getState().setViewLevel(4);
    useStore.getState().navigate("/b");
    // /b was never visited: it starts from the default sort (name, ascending), the view is shared.
    expect(useStore.getState().sort).toEqual({ key: "name", asc: true });
    expect(useStore.getState().settings!.appearance.viewLevel).toBe(4);
    useStore.getState().navigate("/a");
    expect(useStore.getState().sort).toEqual({ key: "size", asc: false });
  });

  it("a never-visited folder starts from the chosen default sort, which can still be changed", async () => {
    const useStore = await fresh({ "files.scope.sort": "folder", "files.sortKey": "modified", "files.sortAsc": false });
    useStore.getState().navigate("/new");
    expect(useStore.getState().sort).toEqual({ key: "modified", asc: false });
    useStore.getState().setSort({ key: "type", asc: true });
    useStore.getState().navigate("/other");
    expect(useStore.getState().sort).toEqual({ key: "modified", asc: false });
    useStore.getState().navigate("/new");
    expect(useStore.getState().sort).toEqual({ key: "type", asc: true });
  });

  it("the view is remembered for each folder when chosen, and falls back to the default view", async () => {
    const { viewLevelOf } = await import("./viewLevel");
    const useStore = await fresh({ "files.scope.view": "folder" });
    useStore.getState().setViewLevel(5);
    useStore.getState().navigate("/b");
    expect(viewLevelOf(useStore.getState().settings, useStore.getState().folderView)).toBe(0);
    useStore.getState().setViewLevel(2);
    useStore.getState().navigate("/a");
    expect(viewLevelOf(useStore.getState().settings, useStore.getState().folderView)).toBe(5);
    useStore.getState().navigate("/b");
    expect(viewLevelOf(useStore.getState().settings, useStore.getState().folderView)).toBe(2);
  });

  it("changing a choice takes effect at once for the open folder", async () => {
    const useStore = await fresh({ "files.scope.sort": "folder" });
    useStore.getState().setSort({ key: "size", asc: true });
    useStore.getState().updateSettings((s) => void (s.advanced["files.scope.sort"] = "shared"));
    useStore.getState().setSort({ key: "modified", asc: true });
    useStore.getState().navigate("/b");
    expect(useStore.getState().sort.key).toBe("modified");
  });
});
