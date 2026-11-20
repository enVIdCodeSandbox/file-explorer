import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

describe("settings undo", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  const fresh = async () => {
    const { useStore } = await import("../store");
    const base = { general: { foldersFirst: true }, appearance: { viewLevel: 0 }, advanced: { "ui.autoSave": true }, profiles: [] } as never;
    useStore.setState({ settings: base, ready: true });
    useStore.getState().beginSettingsSession();
    return useStore;
  };
  const change = (useStore: Awaited<ReturnType<typeof fresh>>, v: number) => {
    useStore.getState().updateSettings((s) => void (s.appearance.viewLevel = v));
    vi.advanceTimersByTime(1000);
  };

  it("undoes and redoes changes one step at a time", async () => {
    const st = await fresh();
    change(st, 1);
    change(st, 2);
    change(st, 3);
    expect(st.getState().settingsUndoDepth).toBe(3);
    st.getState().undoSettings();
    expect(st.getState().settings!.appearance.viewLevel).toBe(2);
    st.getState().undoSettings();
    expect(st.getState().settings!.appearance.viewLevel).toBe(1);
    st.getState().redoSettings();
    expect(st.getState().settings!.appearance.viewLevel).toBe(2);
    expect(st.getState().settingsRedoDepth).toBe(1);
  });

  it("a new change after an undo drops what could have been redone", async () => {
    const st = await fresh();
    change(st, 1);
    change(st, 2);
    st.getState().undoSettings();
    change(st, 5);
    expect(st.getState().settingsRedoDepth).toBe(0);
    expect(st.getState().settings!.appearance.viewLevel).toBe(5);
  });

  it("changes made in quick succession (typing a number) are one step", async () => {
    const st = await fresh();
    for (const v of [1, 12, 120]) st.getState().updateSettings((s) => void (s.appearance.viewLevel = v));
    expect(st.getState().settingsUndoDepth).toBe(1);
    st.getState().undoSettings();
    expect(st.getState().settings!.appearance.viewLevel).toBe(0);
  });

  it("undo all goes back to how Settings was opened", async () => {
    const st = await fresh();
    change(st, 1);
    change(st, 2);
    st.getState().undoAllSettings();
    expect(st.getState().settings!.appearance.viewLevel).toBe(0);
    expect(st.getState().settingsUndoDepth).toBe(0);
  });

  it("closing Settings ends the history: the changes are final", async () => {
    const st = await fresh();
    change(st, 4);
    st.getState().endSettingsSession();
    expect(st.getState().settingsUndoDepth).toBe(0);
    st.getState().undoSettings();
    expect(st.getState().settings!.appearance.viewLevel).toBe(4);
    // and nothing is recorded while Settings is closed
    st.getState().updateSettings((s) => void (s.appearance.viewLevel = 9));
    expect(st.getState().settingsUndoDepth).toBe(0);
  });
});
