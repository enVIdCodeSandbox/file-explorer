import { describe, expect, it } from "vitest";
import { DEFAULT_LAYOUT, resolveLayout, syncDetailsPane } from "./layout";

const withDock = (dock: string, show: boolean) => ({ appearance: { showDetailsPane: show, layout: { ...DEFAULT_LAYOUT, details: { ...DEFAULT_LAYOUT.details, dock } } } });

describe("details pane and layout agree", () => {
  it("docking the pane in the layout turns it on", () => {
    const out = syncDetailsPane(withDock("hidden", false), withDock("right", false));
    expect(out.appearance.showDetailsPane).toBe(true);
  });
  it("hiding it in the layout turns it off", () => {
    expect(syncDetailsPane(withDock("right", true), withDock("hidden", true)).appearance.showDetailsPane).toBe(false);
  });
  it("turning it on from the button brings it back from hidden", () => {
    const out = syncDetailsPane(withDock("hidden", false), withDock("hidden", true));
    expect(resolveLayout(out.appearance.layout).details.dock).toBe("right");
    expect(out.appearance.showDetailsPane).toBe(true);
  });
  it("turning it off from the button leaves the dock alone", () => {
    const out = syncDetailsPane(withDock("bottom", true), withDock("bottom", false));
    expect(resolveLayout(out.appearance.layout).details.dock).toBe("bottom");
    expect(out.appearance.showDetailsPane).toBe(false);
  });
});
