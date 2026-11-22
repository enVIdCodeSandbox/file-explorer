import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./util/crashTrail", () => ({ tracedInvoke: async () => undefined }));

import { useStore } from "./store";

const progress = (id: number, phase = "running") =>
  ({ id, phase, op: "copy", destDir: "/d", current: "", errors: [], errorCount: 0, filesDone: 0, filesTotal: 1, bytesDone: 0, bytesTotal: 10, fraction: 0.2, elapsedSecs: 1, filesPerSec: 0, bytesPerSec: 0 }) as never;

describe("tucking the running jobs into the notification button", () => {
  beforeEach(() => {
    useStore.setState({ transfers: {}, deletes: {}, shown: {}, transfersHidden: false });
  });

  it("stays tucked away while the same jobs go on, and comes back when a new one starts", () => {
    useStore.getState().onProgress(progress(1));
    useStore.setState({ transfersHidden: true });
    useStore.getState().onProgress(progress(1));
    expect(useStore.getState().transfersHidden).toBe(true);
    useStore.getState().onProgress(progress(2));
    expect(useStore.getState().transfersHidden).toBe(false);
  });
});
