import { beforeEach, describe, expect, it, vi } from "vitest";

const calls: { cmd: string; id?: number }[] = [];
vi.mock("./util/crashTrail", () => ({
  tracedInvoke: async (cmd: string, args?: { id?: number }) => {
    calls.push({ cmd, id: args?.id });
    return undefined;
  },
}));

import { useStore } from "./store";

const job = (id: number, phase: string) => ({ id, phase, errors: [], errorCount: 0, destDir: "/d", current: "" }) as never;

describe("stopping every job", () => {
  beforeEach(() => {
    calls.length = 0;
    useStore.setState({
      transfers: { 1: job(1, "running"), 2: job(2, "scanning"), 3: job(3, "done") },
      deletes: { 4: job(4, "running") },
      queuedJobs: { 9: { title: "Copy a", detail: "waiting" } },
    });
  });

  it("lets go of the running jobs and the pastes waiting in line, and leaves finished ones alone", () => {
    const n = useStore.getState().stopAllJobs();
    expect(n).toBe(4);
    const s = useStore.getState();
    expect(Object.keys(s.transfers)).toEqual(["3"]);
    expect(s.deletes).toEqual({});
    expect(s.queuedJobs).toEqual({});
    expect(calls.filter((c) => c.cmd === "transfer_force_stop").map((c) => c.id).sort()).toEqual([1, 2]);
    expect(calls.some((c) => c.cmd === "delete_control" && c.id === 4)).toBe(true);
  });

  it("can keep the one job that matters", () => {
    useStore.getState().stopAllJobs("transfer-2");
    const s = useStore.getState();
    expect(Object.keys(s.transfers).sort()).toEqual(["2", "3"]);
    expect(s.deletes).toEqual({});
    expect(calls.some((c) => c.cmd === "transfer_force_stop" && c.id === 2)).toBe(false);
  });
});
