import { describe, expect, it, vi } from "vitest";

const calls: string[] = [];
vi.mock("./util/crashTrail", () => ({
  tracedInvoke: async (cmd: string, args?: { id?: number }) => {
    calls.push(cmd);
    if (cmd === "transfer_start") return 41;
    if (cmd === "delete_start") return 42;
    return args?.id;
  },
}));

import { adoptJobs, api, onJobClaimed, ownsJob } from "./api";

describe("which window owns a job", () => {
  it("is the window that started it, and only that one", async () => {
    expect(ownsJob(41)).toBe(false);
    const claimed: number[] = [];
    onJobClaimed((id) => claimed.push(id));
    const id = await api.transferStart({ sources: ["a"], destDir: "b", op: "copy", conflict: "ask", skipIdentical: false } as never);
    expect(id).toBe(41);
    expect(ownsJob(41)).toBe(true);
    expect(ownsJob(7)).toBe(false);
    expect(claimed).toContain(41);
    expect(calls).toContain("claim_job");
  });

  it("also covers deletes, and a job taken over from a window that was closed", async () => {
    await api.deleteStart(["x"], false);
    expect(ownsJob(42)).toBe(true);
    const claimed: number[] = [];
    onJobClaimed((id) => claimed.push(id));
    adoptJobs([77]);
    expect(ownsJob(77)).toBe(true);
    expect(claimed).toEqual([77]);
  });
});
