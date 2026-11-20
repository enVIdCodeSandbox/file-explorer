import { describe, it, expect, vi, beforeEach } from "vitest";
import type { Opener, Settings } from "../api";

const app = (name: string): Opener => ({ kind: "app", name, id: `/apps/${name}`, iconUrl: null });

describe("which program a file opens with", () => {
  beforeEach(() => vi.resetModules());

  const setup = async () => {
    const calls: [string, boolean][] = [];
    vi.doMock("../api", async (orig) => {
      const mod = await orig<typeof import("../api")>();
      return { ...mod, api: { defaultApp: (p: string, icon: boolean) => (calls.push([p, icon]), Promise.resolve(app(p.endsWith(".mp4") ? "VLC" : "Editor"))) } };
    });
    return { calls, o: await import("./opener") };
  };

  it("files of one kind share one question, and an icon asks again once", async () => {
    const { calls, o } = await setup();
    expect(o.peekOpener("/a/one.txt")).toBeNull();
    await Promise.all([o.loadOpener("/a/one.txt"), o.loadOpener("/b/two.TXT")]);
    expect(calls.length).toBe(1);
    expect(o.peekOpener("/c/three.txt")?.name).toBe("Editor");
    expect((await o.loadOpener("/c/clip.mp4")).name).toBe("VLC");
    expect(calls.length).toBe(2);
    // The remembered answer has no picture: asking with one goes back to the system, then it is kept.
    expect(o.peekOpener("/a/one.txt", true)).toBeNull();
    await o.loadOpener("/a/one.txt", true);
    expect(calls[2]).toEqual(["/a/one.txt", true]);
    expect(o.peekOpener("/z/other.txt", true)).not.toBeNull();
    o.forgetOpeners();
    expect(o.peekOpener("/a/one.txt")).toBeNull();
  });

  it("files without an extension are looked at one by one (they may be marked runnable)", async () => {
    const { o } = await setup();
    expect(o.openerKey("/bin/tool")).not.toBe(o.openerKey("/bin/other"));
    expect(o.openerKey("/home/me/.bashrc")).toBe("p:/home/me/.bashrc");
    expect(o.openerKey("/a/b.tar.gz")).toBe(".gz");
  });

  it("is only for files on this computer, and can be turned off", async () => {
    const { o } = await setup();
    expect(o.openerApplies("/home/me/a.txt")).toBe(true);
    expect(o.openerApplies("sftp://srv/a.txt")).toBe(false);
    expect(o.showOpener({ advanced: {} } as unknown as Settings)).toBe(true);
    expect(o.showOpener({ advanced: { "files.showOpensWith": false } } as unknown as Settings)).toBe(false);
  });

  it("menu words: the program, or that it runs itself, and nothing otherwise", async () => {
    const { o } = await setup();
    expect(o.openerHint(app("VLC"))).toBe("VLC");
    expect(o.openerHint({ kind: "runs", name: "", id: "", iconUrl: null })).toBe("runs itself");
    expect(o.openerHint({ kind: "none", name: "", id: "", iconUrl: null })).toBeUndefined();
    expect(o.openerHint(null)).toBeUndefined();
  });
});
