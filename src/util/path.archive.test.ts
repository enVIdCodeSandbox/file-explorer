import { describe, it, expect } from "vitest";
import { archiveDisplay, archiveFromTyped, archiveUrl, baseName, crumbs, insideArchive, isArchiveFile, joinPath, parentPath, splitArchive } from "./path";

describe("archive locations", () => {
  const top = archiveUrl("/home/me/photos.zip");
  it("recognizes archive names", () => {
    for (const n of ["a.zip", "a.7z", "a.rar", "a.tar.gz", "A.TGZ", "a.tar.bz2", "a.txz", "x.iso"]) expect(isArchiveFile(n)).toBe(true);
    for (const n of ["a.txt", ".zip", "zip", "a.zipx.txt", "a.docx"]) expect(isArchiveFile(n)).toBe(false);
  });
  it("splits and joins", () => {
    expect(top).toBe("archive:///home/me/photos.zip!/");
    expect(splitArchive(top)).toEqual({ archive: "/home/me/photos.zip", inner: "" });
    const inner = joinPath(joinPath(top, "2024"), "trip");
    expect(inner).toBe("archive:///home/me/photos.zip!/2024/trip");
    expect(splitArchive(inner)).toEqual({ archive: "/home/me/photos.zip", inner: "2024/trip" });
    expect(insideArchive(inner)).toBe("/home/me/photos.zip");
    expect(insideArchive("/home/me")).toBeNull();
  });
  it("walks up through the archive to its own folder", () => {
    expect(baseName(top)).toBe("photos.zip");
    expect(baseName(joinPath(top, "a"))).toBe("a");
    expect(parentPath(joinPath(joinPath(top, "a"), "b"))).toBe("archive:///home/me/photos.zip!/a");
    expect(parentPath(joinPath(top, "a"))).toBe(top.replace(/\/$/, "/"));
    expect(parentPath(top)).toBe("/home/me");
    expect(crumbs(joinPath(top, "a")).map((c) => c.label)).toEqual(["/", "home", "me", "photos.zip", "a"]);
  });
  it("handles Windows archives", () => {
    const win = archiveUrl("C:\\Users\\me\\a.7z", "x/y");
    expect(splitArchive(win)).toEqual({ archive: "C:\\Users\\me\\a.7z", inner: "x/y" });
    expect(parentPath(archiveUrl("C:\\Users\\me\\a.7z"))).toBe("C:\\Users\\me");
  });
  it("is not fooled by a bang in a folder name", () => {
    expect(splitArchive("archive:///x/wow!/a.zip!/in!side")).toEqual({ archive: "/x/wow!/a.zip", inner: "in!side" });
  });
  it("turns typed paths through an archive into locations and back", () => {
    expect(archiveFromTyped("/home/me/a.zip/docs/x")).toBe("archive:///home/me/a.zip!/docs/x");
    expect(archiveFromTyped("/home/me/a.zip")).toBe("archive:///home/me/a.zip!/");
    expect(archiveFromTyped("C:\\Users\\me\\a.7z\\docs")).toBe("archive://C:\\Users\\me\\a.7z!/docs");
    expect(archiveFromTyped("/home/me/docs")).toBeNull();
    expect(archiveDisplay("archive:///home/me/a.zip!/docs/x")).toBe("/home/me/a.zip/docs/x");
    expect(archiveDisplay("archive://C:\\Users\\me\\a.7z!/docs/x")).toBe("C:\\Users\\me\\a.7z\\docs\\x");
    expect(archiveDisplay("/plain/path")).toBe("/plain/path");
  });
});
