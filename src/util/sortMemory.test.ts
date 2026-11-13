import { describe, it, expect, vi } from "vitest";

vi.mock("../api", async (orig) => {
  const mod = await orig<typeof import("../api")>();
  return { ...mod, api: new Proxy({}, { get: () => () => Promise.resolve(undefined) }) };
});

describe("sort memory", () => {
  it("keeps the order across navigation and saves it, and filters reset unless asked to stay", async () => {
    const { useStore } = await import("../store");
    const base = { general: {}, appearance: {}, advanced: {}, profiles: [] } as never;
    useStore.setState({ settings: base, tabs: [{ id: "t", location: "/a", history: ["/a"], historyIndex: 0 }] as never, activeTab: "t" });
    useStore.getState().setSort({ key: "modified", asc: false });
    expect(useStore.getState().settings!.advanced["files.sortKey"]).toBe("modified");
    expect(useStore.getState().settings!.advanced["files.sortAsc"]).toBe(false);
    useStore.getState().setFilter({ text: "x" });
    useStore.getState().navigate("/b");
    expect(useStore.getState().sort).toEqual({ key: "modified", asc: false });
    expect(useStore.getState().filter.text).toBe("");
    useStore.getState().updateSettings((s) => void (s.advanced["files.keepFilter"] = true));
    useStore.getState().setFilter({ text: "y" });
    useStore.getState().navigate("/c");
    expect(useStore.getState().filter.text).toBe("y");
    // Changed in Settings -> applied live.
    useStore.getState().updateSettings((s) => void (s.advanced["files.sortKey"] = "size"));
    expect(useStore.getState().sort.key).toBe("size");
  });
});
