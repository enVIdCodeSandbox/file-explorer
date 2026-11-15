// The copy method readout in Advanced settings.

import { describe, expect, it } from "vitest";
import { ADVANCED } from "./settings/advanced";
import { SETTINGS } from "./settings/schema";
import { ALL_METHODS, METHOD_NAME, jobGroup } from "./util/methods";
import { useStore } from "./store";

describe("naming the work", () => {
  it("has a name for every method", () => {
    for (const m of ALL_METHODS) expect(METHOD_NAME[m].length).toBeGreaterThan(3);
  });
  it("groups every kind of job", () => {
    expect(jobGroup("delete", true)).toBe("Delete");
    expect(jobGroup("delete", false)).toContain("Recycle Bin");
    expect(jobGroup("upload", false)).toBe("Servers and phones");
    expect(jobGroup("remote-delete", false)).toBe("Servers and phones");
    expect(jobGroup("compress", false)).toBe("Compress");
    expect(jobGroup("unpack", false)).toContain("archive");
    expect(jobGroup("update", false)).toBe("Update");
  });
});

describe("the readout in Advanced > Transfers", () => {
  it("is listed with the other Transfers options", () => {
    const def = ADVANCED.find((a) => a.key === "transfers.methods");
    expect(def?.group).toBe("Transfers");
    expect(SETTINGS.some((s) => s.key === "advanced.transfers.methods" && s.control.type === "methods")).toBe(true);
  });
  it("remembers the method the latest job of each kind used", () => {
    const p = { id: 71, op: "copy", phase: "running", method: "robocopy", destDir: "D:\\x", elapsedSecs: 1, filesPerSec: 1, bytesPerSec: 1, fraction: 0.1, errors: [], errorCount: 0 } as never;
    useStore.getState().onProgress(p);
    expect(useStore.getState().lastMethods["Copy and move"]).toMatchObject({ method: "Robocopy", what: "to D:\\x" });
    const job = { id: 72, phase: "running", permanent: false, selected: 1, itemsTotal: 5, itemsDone: 1, current: "", errors: [], errorCount: 0, elapsedSecs: 1, firstName: "a.txt", action: "upload", result: "", bytesDone: 1, bytesTotal: 10, method: "SFTP (over SSH)" } as never;
    useStore.getState().onDeleteProgress(job);
    expect(useStore.getState().lastMethods["Servers and phones"]).toEqual({ method: "SFTP (over SSH)", what: "a.txt" });
  });
});
