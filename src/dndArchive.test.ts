import { describe, expect, it } from "vitest";
import { resolveFolderDrop } from "./dnd";
import { archiveUrl } from "./util/path";

const none = { ctrl: false, shift: false };

describe("dropping onto an archive", () => {
  const zip = "/home/me/photos.zip";
  it("adds files from disk", () => {
    const v = resolveFolderDrop(["/home/me/a.jpg"], archiveUrl(zip), none, []);
    expect(v).toMatchObject({ ok: true, op: "copy", text: "Add to photos.zip" });
  });
  it("refuses to put an archive inside itself", () => {
    expect(resolveFolderDrop([zip], archiveUrl(zip), none, []).ok).toBe(false);
  });
  it("moves within the same archive", () => {
    const v = resolveFolderDrop([archiveUrl(zip, "a.jpg")], archiveUrl(zip, "sub"), none, []);
    expect(v).toMatchObject({ ok: true, op: "move" });
  });
});
