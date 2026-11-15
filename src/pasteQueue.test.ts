// Pasting while a delete is still working on the same folder waits in line instead of racing it.

import { beforeEach, describe, expect, it, vi } from "vitest";

const gate: { release: () => void } = { release: () => {} };
const calls = { transferStart: 0, deleteStart: 0 };

vi.mock("./api", async (orig) => {
  const real = await orig<typeof import("./api")>();
  return {
    ...real,
    api: {
      ...real.api,
      // The access check is slow, as it can be for a big folder.
      checkAccess: (job: { kind: string }) =>
        job.kind === "delete" ? new Promise((r) => (gate.release = () => r({ needsElevation: false, becomesPermanent: false }))) : Promise.resolve({ needsElevation: false, becomesPermanent: false }),
      deleteStart: async () => ((calls.deleteStart += 1), 7),
      transferStart: async () => ((calls.transferStart += 1), 100 + calls.transferStart),
      clipboardSetFiles: async () => {},
    },
  };
});

const { useStore, deletePaths } = await import("./store");

const done = (id: number) =>
  ({ id, phase: "done", permanent: false, selected: 1, itemsTotal: 1, itemsDone: 1, current: "", errors: [], errorCount: 0, elapsedSecs: 0, firstName: "x", action: "delete", result: "", bytesDone: 0, bytesTotal: 0, skipped: 0 }) as never;
const tick = (ms = 5) => new Promise((r) => setTimeout(r, ms));

beforeEach(() => {
  calls.transferStart = 0;
  calls.deleteStart = 0;
  useStore.setState({
    info: { os: "linux" } as never,
    settings: { appearance: {}, general: { confirmDelete: false, deleteToTrash: true }, transfer: { conflict: "ask" }, advanced: {} } as never,
    queuedJobs: {},
    transfers: {},
    deletes: {},
    notices: [],
  });
});

describe("a paste that meets a delete", () => {
  it("waits for it, even before the delete has started, and then runs by itself", async () => {
    const del = deletePaths(["/docs/report.txt"]); // stuck in its access check
    await tick();
    const paste = useStore.getState().startTransfer(["/backup/report.txt"], "/docs", "copy");
    await tick();
    expect(calls.transferStart, "not started while the delete is pending").toBe(0);
    expect(Object.keys(useStore.getState().queuedJobs)).toHaveLength(1);
    gate.release();
    await del;
    expect(calls.deleteStart).toBe(1);
    await tick();
    expect(calls.transferStart, "still waiting while the delete runs").toBe(0);
    useStore.getState().onDeleteProgress(done(7));
    await paste;
    expect(calls.transferStart).toBe(1);
    expect(useStore.getState().queuedJobs).toEqual({});
  });

  it("turns away the same paste while it waits, and lines up different ones in order", async () => {
    const del = deletePaths(["/docs/a.txt"]);
    await tick();
    const first = useStore.getState().startTransfer(["/backup/a.txt"], "/docs", "copy");
    await tick();
    await useStore.getState().startTransfer(["/backup/a.txt"], "/docs", "copy"); // pasted again: nothing new
    expect(Object.keys(useStore.getState().queuedJobs)).toHaveLength(1);
    const second = useStore.getState().startTransfer(["/other/a.txt"], "/docs", "copy"); // same name, other file
    await tick();
    expect(Object.keys(useStore.getState().queuedJobs)).toHaveLength(2);
    gate.release();
    await del;
    useStore.getState().onDeleteProgress(done(7));
    await first;
    expect(calls.transferStart, "the second one waits for the first").toBe(1);
    useStore.getState().onProgress({ id: 101, phase: "done" } as never);
    await second;
    expect(calls.transferStart).toBe(2);
  });

  it("does not wait for a delete somewhere else", async () => {
    const del = deletePaths(["/music/song.mp3"]);
    await tick();
    await useStore.getState().startTransfer(["/backup/report.txt"], "/docs", "copy");
    expect(calls.transferStart).toBe(1);
    gate.release();
    await del;
  });
});
