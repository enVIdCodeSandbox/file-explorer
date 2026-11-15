// A terminal cannot open inside an archive, so the archive is unpacked first.

import { describe, expect, it, vi } from "vitest";

const opened: [string, string][] = [];
const started: { paths: string[]; here: boolean }[] = [];

vi.mock("./api", async (orig) => {
  const real = await orig<typeof import("./api")>();
  return {
    ...real,
    api: {
      ...real.api,
      archiveStart: async (_a: string, paths: string[], here: boolean) => (started.push({ paths, here }), 41 + started.length),
      openTerminal: async (dir: string, mode: string) => void opened.push([dir, mode]),
    },
  };
});

const { useStore, extractAndOpenTerminal } = await import("./store");
const { archiveUrl } = await import("./util/path");

const finished = (id: number, folder: string) =>
  ({ id, phase: "done", permanent: false, selected: 1, itemsTotal: 3, itemsDone: 3, current: "", errors: [], errorCount: 0, elapsedSecs: 0, firstName: "bundle.zip", action: "extract", result: folder, bytesDone: 0, bytesTotal: 0, skipped: 0 }) as unknown as import("./api").DeleteProgress;

describe("a terminal for a folder inside an archive", () => {
  useStore.setState({ settings: { appearance: {}, general: {}, advanced: {}, transfer: {} } as never, info: { os: "windows" } as never });

  it("unpacks first and opens in the same folder of the unpacked copy", async () => {
    await extractAndOpenTerminal(archiveUrl("F:\\Downloads\\build.zip", "release/bundle/msi"), "here");
    expect(started.at(-1)).toEqual({ paths: ["F:\\Downloads\\build.zip"], here: false });
    expect(opened, "nothing opens before it is unpacked").toEqual([]);
    useStore.getState().onDeleteProgress(finished(42, "F:\\Downloads\\build"));
    await Promise.resolve();
    expect(opened).toEqual([["F:\\Downloads\\build\\release\\bundle\\msi", "here"]]);
  });

  it("can unpack into the folder the archive is in, and open as administrator", async () => {
    await extractAndOpenTerminal(archiveUrl("F:\\Downloads\\build.zip"), "admin", true);
    expect(started.at(-1)?.here).toBe(true);
    useStore.getState().onDeleteProgress(finished(43, "F:\\Downloads"));
    await Promise.resolve();
    expect(opened.at(-1)).toEqual(["F:\\Downloads", "admin"]);
  });

  it("opens nothing if the unpacking fails", async () => {
    const before = opened.length;
    await extractAndOpenTerminal(archiveUrl("F:\\x.zip"), "here");
    useStore.getState().onDeleteProgress({ ...finished(44, ""), phase: "failed", errors: [{ path: "", message: "bad zip", needsElevation: false }], errorCount: 1 });
    await Promise.resolve();
    expect(opened.length).toBe(before);
  });
});
