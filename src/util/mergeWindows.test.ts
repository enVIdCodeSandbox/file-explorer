import { beforeEach, describe, expect, it } from "vitest";
import { useStore } from "../store";
import { closeAllPanes, paneCount, splitPaths, useSplit } from "../split/state";
import { fitsPane, planMerge } from "./mergeWindows";

const t = (id: string, location: string) => ({ id, location });

describe("taking a window's tabs", () => {
  it("adds everything as tabs when asked for tabs", () => {
    const all = [t("1", "/a"), t("2", "ftp://x/y"), t("3", "thispc://")];
    const plan = planMerge(all, "tabs", 5);
    expect(plan.tabs).toEqual(all);
    expect(plan.panes).toEqual([]);
  });

  it("makes panes of the folders that fit, up to the room, and tabs of the rest", () => {
    const plan = planMerge([t("1", "/a"), t("2", "/b"), t("3", "/c"), t("4", "sftp://h/p")], "panes", 2);
    expect(plan.panes.map((x) => x.id)).toEqual(["1", "2"]);
    expect(plan.tabs.map((x) => x.id)).toEqual(["3", "4"]);
  });

  it("makes no panes when there is no room", () => {
    const plan = planMerge([t("1", "/a")], "panes", 0);
    expect(plan.panes).toEqual([]);
    expect(plan.tabs).toHaveLength(1);
  });

  it("knows which places a pane can show", () => {
    expect(fitsPane("/home/me")).toBe(true);
    expect(fitsPane("")).toBe(false);
    expect(fitsPane("sftp://h/p")).toBe(false);
  });
});

describe("what the window that gave its tabs does", () => {
  const tab = (id: string, location: string) => ({ id, location, groupId: null, history: [location], historyIndex: 0 });
  beforeEach(() => {
    closeAllPanes();
    useSplit.setState({ allowed: true });
    useStore.setState({ tabs: [tab("a", "/a"), tab("b", "/b"), tab("c", "/c")] as never, activeTab: "a", closedTabs: [] });
  });

  it("lets go of the tabs that were taken, keeps the rest, and does not remember them as closed", () => {
    expect(useStore.getState().releaseTabs(["a", "c"])).toBe(1);
    expect(useStore.getState().tabs.map((x) => x.id)).toEqual(["b"]);
    expect(useStore.getState().closedTabs).toHaveLength(0);
  });

  it("reports that nothing is left when every tab was taken", () => {
    expect(useStore.getState().releaseTabs(["a", "b", "c"])).toBe(0);
  });
});

describe("panes made from another window's tabs", () => {
  beforeEach(() => {
    closeAllPanes();
    useSplit.setState({ allowed: true });
  });
  it("makes a pane per folder, side by side or as chosen, and stops at the most panes", () => {
    expect(splitPaths(["/a", "/b"])).toBe(2);
    expect(paneCount()).toBe(3);
    closeAllPanes();
    expect(splitPaths(Array.from({ length: 12 }, (_, i) => `/p${i}`))).toBe(7);
    expect(paneCount()).toBe(8);
  });
  it("uses the chosen layout", () => {
    splitPaths(["/a", "/b"], "3-stack");
    const { tree } = useSplit.getState();
    expect(typeof tree === "object" && tree.dir).toBe("col");
  });
});
