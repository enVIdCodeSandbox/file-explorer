import { beforeEach, describe, expect, it } from "vitest";
import { useStore } from "../store";
import { closeAllPanes, dropOnPanes, openChooser, openTabs, paneCount, splitCapacity, splitTabs, useSplit } from "./state";
import { leaves } from "../util/splitLayout";

const tab = (id: string, location: string) => ({ id, location, groupId: null, history: [location], historyIndex: 0 });

beforeEach(() => {
  closeAllPanes();
  useSplit.setState({ allowed: true, chooser: null, drag: null });
  useStore.setState({ tabs: [tab("a", "/a"), tab("b", "/b"), tab("c", "/c"), tab("d", "/d")] as never, activeTab: "a", info: { home: "/home" } as never });
});

describe("choosing the tabs to split", () => {
  it("lists every open tab, the window's first and then those of the extra panes", () => {
    splitTabs([{ slot: 0, tabId: "d" }]);
    const list = openTabs();
    expect(list.map((t) => [t.slot, t.path])).toEqual([[0, "/a"], [0, "/b"], [0, "/c"], [1, "/d"]]);
  });

  it("makes a pane of each ticked tab, side by side, and takes them out of the tab strip", () => {
    const made = splitTabs([{ slot: 0, tabId: "b" }, { slot: 0, tabId: "d" }]);
    expect(made).toBe(2);
    expect(useStore.getState().tabs.map((t) => t.id)).toEqual(["a", "c"]);
    const { tree, panes } = useSplit.getState();
    expect(leaves(tree)).toEqual([0, 1, 2]);
    expect(panes[1].tabs[0].path).toBe("/b");
    expect(panes[2].tabs[0].path).toBe("/d");
    expect(typeof tree === "object" && tree.dir).toBe("row");
  });

  it("uses the layout that was chosen", () => {
    splitTabs([{ slot: 0, tabId: "b" }, { slot: 0, tabId: "c" }], "3-stack");
    const { tree } = useSplit.getState();
    expect(typeof tree === "object" && tree.dir).toBe("col");
  });

  it("counts a tab twice ticked once, and the window's last tab stays (its pane gets a copy)", () => {
    useStore.setState({ tabs: [tab("a", "/a")] as never });
    expect(splitTabs([{ slot: 0, tabId: "a" }, { slot: 0, tabId: "a" }])).toBe(1);
    expect(useStore.getState().tabs).toHaveLength(1);
    expect(useSplit.getState().panes[1].tabs[0].path).toBe("/a");
  });

  it("stops at the most panes there can be", () => {
    const many = Array.from({ length: 12 }, (_, i) => tab(`t${i}`, `/t${i}`));
    useStore.setState({ tabs: many as never });
    const all = many.map((t) => ({ slot: 0, tabId: t.id }));
    expect(splitCapacity(all)).toBe(7);
    expect(splitTabs(all)).toBe(7);
    expect(paneCount()).toBe(8);
  });

  it("splitting a pane's only tab again moves it rather than adding a pane", () => {
    splitTabs([{ slot: 0, tabId: "b" }]);
    expect(paneCount()).toBe(2);
    splitTabs([{ slot: 1, tabId: useSplit.getState().panes[1].tabs[0].id }, { slot: 0, tabId: "c" }]);
    expect(paneCount()).toBe(3);
    expect(openTabs().map((t) => t.path).sort()).toEqual(["/a", "/b", "/c", "/d"]);
  });

  it("opens the list when a tab is let go over the button, and does nothing to the tabs yet", () => {
    useSplit.setState({ drag: { hover: { choose: true }, label: "b", empties: null } });
    expect(dropOnPanes({ slot: 0, tabId: "b" })).toBe(true);
    expect(useSplit.getState().chooser?.first).toEqual({ slot: 0, tabId: "b" });
    expect(useStore.getState().tabs).toHaveLength(4);
    expect(paneCount()).toBe(1);
  });

  it("does not open where the place cannot be split", () => {
    useSplit.setState({ allowed: false });
    openChooser(null);
    expect(useSplit.getState().chooser).toBeNull();
  });
});
