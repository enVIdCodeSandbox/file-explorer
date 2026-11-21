import { describe, it, expect, vi, beforeEach } from "vitest";
import type { Settings } from "../api";
import { stageRemote, isTooBig, tooBigMb } from "./stageRemote";

const st = (advanced: Record<string, unknown> = {}) => ({ advanced }) as unknown as Settings;

describe("copying from a server or phone, for other programs", () => {
  it("small files by default, with a limit; an old on/off value still means something", () => {
    expect(stageRemote(st())).toEqual({ mode: "small", mb: 50 });
    expect(stageRemote(st({ "clipboard.stageRemote": "any" }))).toEqual({ mode: "any" });
    expect(stageRemote(st({ "clipboard.stageRemote": "off" }))).toEqual({ mode: "off" });
    expect(stageRemote(st({ "clipboard.stageRemote": false }))).toEqual({ mode: "off" });
    expect(stageRemote(st({ "clipboard.stageRemote": true, "clipboard.stageRemoteMb": 200 }))).toEqual({ mode: "small", mb: 200 });
    expect(stageRemote(st({ "clipboard.stageRemoteMb": "nonsense" }))).toEqual({ mode: "small", mb: 50 });
  });

  it("recognises the answer for something too big", () => {
    expect(isTooBig("Error: too-big-to-stage:61440")).toBe(true);
    expect(tooBigMb("too-big-to-stage:61440")).toBe(61440);
    expect(isTooBig("The server said no")).toBe(false);
    expect(tooBigMb("too-big-to-stage")).toBeNull();
  });
});

describe("dragging phone and server items out of the window", () => {
  beforeEach(() => vi.resetModules());

  const setup = async (start: (...a: unknown[]) => Promise<number>, finished: Record<string, unknown>) => {
    const calls: unknown[][] = [];
    vi.doMock("../api", async (orig) => {
      const mod = await orig<typeof import("../api")>();
      return { ...mod, api: { remoteStageStart: (...a: unknown[]) => (calls.push(a), start(...a)), deleteControl: (...a: unknown[]) => (calls.push(["cancel", ...a]), Promise.resolve()) } };
    });
    vi.doMock("../store", () => ({ awaitStage: () => Promise.resolve(finished), useStore: { getState: () => ({}) } }));
    return { calls, m: await import("./dragFetch") };
  };

  it("fetches with the size limit, accepting folders, and hands back the local files", async () => {
    const { calls, m } = await setup(async () => 7, { phase: "done", result: "/tmp/a/IMG1.jpg\n/tmp/b/IMG2.jpg", errors: [] });
    const f = await m.startDragFetch(["mtp://k/Internal/DCIM/IMG1.jpg", "mtp://k/Internal/DCIM/IMG2.jpg"], 2048);
    expect(calls[0]).toEqual([["mtp://k/Internal/DCIM/IMG1.jpg", "mtp://k/Internal/DCIM/IMG2.jpg"], 2048, true]);
    expect(f.id).toBe(7);
    await expect(f.done).resolves.toEqual(["/tmp/a/IMG1.jpg", "/tmp/b/IMG2.jpg"]);
  });

  it("explains a file that is over the limit in words a person can act on", async () => {
    const { m } = await setup(() => Promise.reject(new Error("too-big-to-stage:61440")), {});
    await expect(m.startDragFetch(["mtp://k/a.mp4"], 2048)).rejects.toThrow(/61,440 MB.*2,048 MB.*copy it and paste it inside the app/s);
  });

  it("a failed or stopped fetch is an error, not an empty drag", async () => {
    const failed = await setup(async () => 1, { phase: "failed", result: "", errors: [{ message: "The phone was unplugged." }] });
    await expect((await failed.m.startDragFetch(["mtp://k/a"], 10)).done).rejects.toThrow("The phone was unplugged.");
    vi.resetModules();
    const stopped = await setup(async () => 2, { phase: "cancelled", result: "", errors: [] });
    await expect((await stopped.m.startDragFetch(["mtp://k/a"], 10)).done).rejects.toThrow("Stopped.");
  });

  it("the limit comes from the setting", async () => {
    const { m } = await setup(async () => 1, {});
    expect(m.dragLimitMb(st())).toBe(2048);
    expect(m.dragLimitMb(st({ "drag.remoteMb": 500 }))).toBe(500);
  });
});
