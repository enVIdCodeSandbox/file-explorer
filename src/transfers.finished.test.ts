import { beforeEach, describe, expect, it } from "vitest";
import type { DeleteProgress, Progress } from "./api";
import { FINISHED_KEPT, useStore } from "./store";

const job = (id: number) => ({ id, phase: "done", errorCount: 0, elapsedSecs: 12, filesDone: 3, filesTotal: 3 }) as unknown as Progress;

beforeEach(() => useStore.setState({ transfers: {}, deletes: {}, finished: {}, finishedOpen: false, transfersOpen: true, series: {} }));

describe("a finished job", () => {
  it("leaves the screen at once and is kept to look at again", () => {
    useStore.setState({ transfers: { 1: job(1) } });
    useStore.getState().finishJob("transfer", job(1));
    const s = useStore.getState();
    expect(s.transfers[1]).toBeUndefined();
    expect(s.finished["transfer-1"].p.id).toBe(1);
    expect(s.transfersOpen).toBe(false);
  });

  it("keeps the window while something else is still going", () => {
    useStore.setState({ transfers: { 1: job(1), 2: { ...job(2), phase: "running" } as Progress } });
    useStore.getState().finishJob("transfer", job(1));
    expect(useStore.getState().transfersOpen).toBe(true);
  });

  it("does the same for deletes", () => {
    const d = { id: 5, phase: "done", errorCount: 0, action: "delete" } as unknown as DeleteProgress;
    useStore.setState({ deletes: { 5: d } });
    useStore.getState().finishJob("delete", d);
    expect(useStore.getState().deletes[5]).toBeUndefined();
    expect(useStore.getState().finished["delete-5"]).toBeDefined();
  });

  it("can be forgotten one by one or all together", () => {
    const st = useStore.getState();
    st.finishJob("transfer", job(1));
    st.finishJob("transfer", job(2));
    useStore.setState({ finishedOpen: true });
    st.forgetFinished("transfer-1");
    expect(Object.keys(useStore.getState().finished)).toEqual(["transfer-2"]);
    st.forgetFinished();
    expect(useStore.getState().finished).toEqual({});
    expect(useStore.getState().finishedOpen).toBe(false);
  });

  it("keeps only the latest few, with their graphs", () => {
    const st = useStore.getState();
    for (let i = 1; i <= FINISHED_KEPT + 5; i++) {
      useStore.setState({ series: { ...useStore.getState().series, [i]: [] } });
      st.finishJob("transfer", job(i));
    }
    const keys = Object.keys(useStore.getState().finished);
    expect(keys.length).toBe(FINISHED_KEPT);
    expect(keys).not.toContain("transfer-1");
    expect(useStore.getState().series[1]).toBeUndefined();
  });

  it("is shown when the progress report says it is done", () => {
    useStore.getState().onProgress({ ...job(9), destDir: "/x", series: undefined, errors: [], prediction: null } as unknown as Progress);
    const s = useStore.getState();
    expect(s.transfers[9]).toBeUndefined();
    expect(s.finished["transfer-9"]).toBeDefined();
  });

  it("stays on screen when it had errors", () => {
    useStore.getState().onProgress({ ...job(10), errorCount: 2, destDir: "/x", errors: [] } as unknown as Progress);
    expect(useStore.getState().transfers[10]).toBeDefined();
    expect(useStore.getState().finished["transfer-10"]).toBeUndefined();
  });

  it("opens on the job a notification points at, even over the running jobs' window, and again later", () => {
    const st = useStore.getState();
    st.finishJob("transfer", job(1));
    useStore.setState({ transfersOpen: true });
    st.showFinished("transfer-1");
    expect(useStore.getState()).toMatchObject({ finishedOpen: true, finishedPick: "transfer-1", transfersOpen: false });
    // Still open when another job ends; its notification points at it.
    st.finishJob("transfer", job(2));
    expect(useStore.getState().finishedOpen).toBe(true);
    st.showFinished("transfer-2");
    expect(useStore.getState().finishedPick).toBe("transfer-2");
    // Closed and asked again.
    useStore.setState({ finishedOpen: false });
    st.showFinished("transfer-1");
    expect(useStore.getState()).toMatchObject({ finishedOpen: true, finishedPick: "transfer-1" });
  });
});
