import { afterEach, describe, expect, it } from "vitest";
import { mergeHits } from "./searchHits";
import { configureFormat, formatMemory, formatDuration, formatEta, formatRate, formatSize, kindOf, steadyFraction } from "./format";
import { baseName, crumbs, joinPath, parentPath, parseLocation, searchLocation } from "./path";
import { layoutTree, squarify } from "./treemap";
import type { SizeNode } from "../api";

describe("format", () => {
  it("formats sizes", () => {
    expect(formatSize(0)).toBe("0 B");
    expect(formatSize(1500)).toBe("1.50 KB");
    expect(formatSize(5e9)).toBe("5.00 GB");
    expect(formatSize(250e6)).toBe("250 MB");
  });

  it("formats time remaining", () => {
    expect(formatEta(null)).toBe("Calculating…");
    expect(formatEta(12)).toBe("15 seconds left");
    expect(formatEta(150)).toBe("About 3 minutes left");
    expect(formatEta(7500)).toBe("About 2 hours 5 minutes left");
  });

  it("picks the rate unit from what limits the transfer", () => {
    const base = { bytesPerSec: 100e6, filesPerSec: 842 };
    expect(formatRate({ ...base, display: "bytes" })).toBe("100 MB/s");
    expect(formatRate({ ...base, display: "files" })).toBe("842 files/s");
    expect(formatRate({ ...base, display: "mixed" })).toBe("842 files/s · 100 MB/s");
  });

  it("never lets progress go backwards", () => {
    expect(steadyFraction(0.4, { phase: "running", fraction: 0.3 })).toBeCloseTo(0.4);
    expect(steadyFraction(0.4, { phase: "running", fraction: 0.6 })).toBeCloseTo(0.6);
    expect(steadyFraction(0.4, { phase: "running", fraction: 1.2 })).toBeLessThan(1);
    expect(steadyFraction(0.4, { phase: "done", fraction: 0.1 })).toBe(1);
  });

  it("formats durations", () => {
    expect(formatDuration(5)).toBe("5 sec");
    expect(formatDuration(200)).toBe("3 min 20 sec");
    expect(formatDuration(7500)).toBe("2 h 5 min");
  });

  it("classifies files", () => {
    expect(kindOf("a.JPG", false)).toBe("image");
    expect(kindOf("a", true)).toBe("folder");
    expect(kindOf(".bashrc", false)).toBe("file");
  });
});

describe("path", () => {
  it("handles unix paths", () => {
    expect(joinPath("/home/me", "x")).toBe("/home/me/x");
    expect(joinPath("/", "x")).toBe("/x");
    expect(parentPath("/home/me")).toBe("/home");
    expect(parentPath("/home")).toBe("/");
    expect(parentPath("/")).toBeNull();
    expect(baseName("/home/me/")).toBe("me");
    expect(crumbs("/home/me").map((c) => c.path)).toEqual(["/", "/home", "/home/me"]);
  });

  it("handles windows paths", () => {
    expect(joinPath("C:\\", "Users")).toBe("C:\\Users");
    expect(joinPath("C:\\Users", "me")).toBe("C:\\Users\\me");
    expect(parentPath("C:\\Users\\me")).toBe("C:\\Users");
    expect(parentPath("C:\\Users")).toBe("C:\\");
    expect(parentPath("C:\\")).toBeNull();
    expect(crumbs("C:\\Users\\me").map((c) => c.label)).toEqual(["C:\\", "Users", "me"]);
  });

  it("round-trips app locations", () => {
    const loc = searchLocation("a b&c", "C:\\x y");
    expect(parseLocation(loc)).toEqual({ kind: "search", query: "a b&c", scope: "C:\\x y" });
    expect(parseLocation("/tmp")).toEqual({ kind: "folder", path: "/tmp" });
  });
});

describe("treemap", () => {
  it("fills the rectangle with proportional, non-overlapping areas", () => {
    const sizes = [6, 6, 4, 3, 2, 2, 1];
    const rects = squarify(sizes, { x: 0, y: 0, w: 600, h: 400 });
    const total = sizes.reduce((a, b) => a + b, 0);
    rects.forEach((r, i) => expect(r.w * r.h).toBeCloseTo((sizes[i] / total) * 240000, 3));
    for (const r of rects) {
      expect(r.x).toBeGreaterThanOrEqual(-1e-6);
      expect(r.x + r.w).toBeLessThanOrEqual(600 + 1e-6);
      expect(r.y + r.h).toBeLessThanOrEqual(400 + 1e-6);
    }
    // Squarified layouts keep tiles reasonably square.
    expect(Math.max(...rects.map((r) => Math.max(r.w / r.h, r.h / r.w)))).toBeLessThan(4);
  });

  it("handles empty and zero-size input", () => {
    expect(squarify([], { x: 0, y: 0, w: 10, h: 10 })).toEqual([]);
    expect(squarify([0, 0], { x: 0, y: 0, w: 10, h: 10 }).every((r) => r.w === 0)).toBe(true);
  });

  it("lays out nested trees into leaf tiles", () => {
    const leaf = (name: string, size: number): SizeNode => ({ name, path: name, size, isDir: false, children: [], types: [] });
    const tree: SizeNode = {
      name: "root", path: "/", size: 10, isDir: true, types: [],
      children: [{ name: "d", path: "/d", size: 6, isDir: true, types: [], children: [leaf("a", 4), leaf("b", 2)] }, leaf("c", 4)],
    };
    const tiles = layoutTree(tree, { x: 0, y: 0, w: 100, h: 100 });
    expect(tiles.map((t) => t.node.name).sort()).toEqual(["a", "b", "c"]);
    expect(tiles.reduce((s, t) => s + t.w * t.h, 0)).toBeCloseTo(10000, 3);
  });
});

import { driveLetter, driveName, driveRemote, hidesAddress, withoutLetter } from "./drives";
import type { Drive } from "../api";

describe("drive names", () => {
  const drive = { name: "games (\\\\192.168.100.1) (S:)", mount: "S:\\", media: "network", remote: "\\\\192.168.100.1\\games" } as Drive;
  const local = { name: "main (C:)", mount: "C:\\", media: "ssd", remote: null } as Drive;
  const base = { hideNetworkAddress: false, driveNames: {} } as never;

  it("keeps the system name by default", () => {
    expect(driveName(drive, base)).toBe(drive.name);
    expect(driveRemote(drive, base)).toBe(drive.remote);
  });
  it("hides the address globally or per drive", () => {
    const g = { hideNetworkAddress: true, driveNames: {} } as never;
    expect(driveName(drive, g)).toBe("games (S:)");
    expect(driveRemote(drive, g)).toBeNull();
    const per = { hideNetworkAddress: true, driveNames: { "S:\\": { name: "", base: "", hideAddress: false } } } as never;
    expect(hidesAddress(drive, per)).toBe(false);
    expect(hidesAddress(local, g)).toBe(false);
  });
  it("uses a local name only while the system name is unchanged", () => {
    const a = { hideNetworkAddress: false, driveNames: { "S:\\": { name: "Games", base: drive.name, hideAddress: null } } } as never;
    expect(driveName(drive, a)).toBe("Games (S:)");
    expect(driveName({ ...drive, name: "Games renamed (S:)" }, a)).toBe("Games renamed (S:)");
  });
});

import { describeItems, transferNotice } from "./notices";

describe("notices", () => {
  it("words single and multiple items", () => {
    expect(describeItems(["/a/report.pdf"])).toBe('"report.pdf"');
    expect(describeItems(["/a/x", "/a/y", "/a/z"])).toBe("3 items");
    expect(transferNotice("move", ["/a/x"], "/b/Docs")).toEqual({ title: 'Moved "x"', detail: "to Docs" });
  });
});

import { categoryOf, extOf, hideSmall, isolate, matchesFilter, nodeTypes, sortLargest, withoutPaths } from "./analysis";

describe("disk usage helpers", () => {
  const file = (name: string, size: number, path = `/r/${name}`): SizeNode => ({ name, path, size, isDir: false, children: [], types: [] });
  const names = ["(no extension)", "(other)", "png", "txt"];
  const tree: SizeNode = {
    name: "/r", path: "/r", size: 1000, isDir: true, types: [[2, 700], [3, 300]],
    children: [
      file("a.png", 500),
      file("b.txt", 100),
      { name: "sub", path: "/r/sub", size: 400, isDir: true, types: [[2, 200], [3, 200]], children: [file("c.png", 150, "/r/sub/c.png")] },
    ],
  };

  it("reads file types like the scanner", () => {
    expect(extOf("Photo.JPG")).toBe("jpg");
    expect(extOf("Makefile")).toBe("(no extension)");
    expect(extOf(".gitignore")).toBe("(no extension)");
    expect(nodeTypes(tree, names)).toEqual([{ ext: "png", bytes: 700 }, { ext: "txt", bytes: 300 }]);
  });
  it("isolates a type and keeps totals honest", () => {
    const only = isolate(tree, new Set(["png"]), names)!;
    expect(only.size).toBe(700);
    expect(only.children.map((c) => c.name)).toEqual(["a.png", "sub"]);
    expect(only.children[1].children.map((c) => c.name)).toEqual(["c.png", "(other)"]);
    expect(only.children.find((c) => c.name === "sub")!.size).toBe(200);
    expect(isolate(tree, new Set(["zip"]), names)).toBeNull();
  });
  it("folds small files into (other)", () => {
    const t = hideSmall(tree, 200);
    expect(t.children.map((c) => c.name).sort()).toEqual(["(other)", "a.png", "sub"]);
    expect(t.children.find((c) => c.name === "(other)")!.size).toBe(100);
  });
  it("removes deleted items from the sizes above them", () => {
    const t = withoutPaths(tree, new Set(["/r/sub/c.png"]));
    expect(t.size).toBe(850);
    expect(t.children.find((c) => c.name === "sub")!.size).toBe(250);
  });
  it("filters and sorts the file list", () => {
    const list = [
      { path: "/a/x.png", name: "x.png", size: 5, modified: 1 },
      { path: "/a/y.txt", name: "y.txt", size: 9, modified: 3 },
    ];
    expect(sortLargest(list, "size", false)[0].name).toBe("y.txt");
    expect(sortLargest(list, "modified", true)[0].name).toBe("x.png");
    expect(matchesFilter("x.png", 5, { text: "", minBytes: 6, exts: new Set() })).toBe(false);
    expect(matchesFilter("x.png", 5, { text: "X", minBytes: 0, exts: new Set(["png"]) })).toBe(true);
    expect(categoryOf("png")).toBe("Images");
    expect(categoryOf("zzzz")).toBe("Other");
  });
});

describe("size units", () => {
  const set = (units: "decimal" | "binary" | "windows") => configureFormat({ units, dates: "system", clock: "auto" });
  afterEach(() => set("decimal"));
  it("binary mode counts in 1024s and says MiB", () => {
    set("binary");
    expect(formatSize(1536)).toBe("1.50 KiB");
    expect(formatSize(250 * 1024 ** 2)).toBe("250 MiB");
  });
  it("windows mode counts in 1024s but keeps the MB label", () => {
    set("windows");
    expect(formatSize(250 * 1024 ** 2)).toBe("250 MB");
  });
  it("memory rounds to the number on the box", () => {
    expect(formatMemory(16 * 1024 ** 3)).toBe("16 GB");
    set("binary");
    expect(formatMemory(16 * 1024 ** 3)).toBe("16 GiB");
  });
});

describe("streamed search results", () => {
  const hit = (name: string, score: number, path = `/x/${name}`) => ({ name, score, path, isDir: false, size: 0, modified: 0 });
  it("merges batches best-first without duplicates and respects the limit", () => {
    const first = [hit("report.pdf", 70)];
    const merged = mergeHits(first, [hit("annual-report.pdf", 50), hit("report.pdf", 70), hit("report", 100)], 3);
    expect(merged.map((h) => h.name)).toEqual(["report", "report.pdf", "annual-report.pdf"]);
    // Equal scores: the shorter name wins the last slot.
    expect(mergeHits(merged, [hit("zzz-report.txt", 50)], 3).map((h) => h.name)).toEqual(["report", "report.pdf", "zzz-report.txt"]);
    expect(mergeHits(merged, [], 3)).toBe(merged);
  });
});

describe("drive names keep their letter", () => {
  const drive = { mount: "S:\\", name: "games (\\\\192.168.100.1) (S:)", media: "network", remote: "\\\\192.168.100.1\\games" } as unknown as import("../api").Drive;
  const base = { driveNames: {}, hideNetworkAddress: false } as unknown as import("../api").Settings["appearance"];
  it("shows the system name unchanged", () => {
    expect(driveName(drive, base)).toBe("games (\\\\192.168.100.1) (S:)");
  });
  it("re-adds the letter to a renamed drive, and ignores one typed in the name", () => {
    const named = { ...base, driveNames: { "S:\\": { name: "Games (Z:)", base: drive.name, hideAddress: null } } };
    expect(driveName(drive, named)).toBe("Games (S:)");
    expect(driveName(drive, { ...base, hideNetworkAddress: true })).toBe("games (S:)");
    expect(withoutLetter("Games (S:)")).toBe("Games");
  });
  it("leaves drives that have no letter alone", () => {
    const mac = { mount: "/Volumes/Backup", name: "Backup", media: "external", remote: null } as unknown as import("../api").Drive;
    expect(driveName(mac, base)).toBe("Backup");
    expect(driveLetter(mac)).toBeNull();
  });
});


import { resolveFolderDrop, volumeOf } from "../dnd";

describe("dropping onto a folder", () => {
  const d = (mount: string): Drive => ({ name: mount, mount, fs: "ntfs", total: 1, available: 1, media: "ssd", removable: false, readOnly: false, online: true, remote: null } as Drive);
  const drives = [d("C:\\"), d("D:\\")];
  const none = { ctrl: false, shift: false };

  it("moves on the same drive and copies to another", () => {
    expect(resolveFolderDrop(["C:\\a\\x.txt"], "C:\\b", none, drives)).toMatchObject({ ok: true, op: "move" });
    expect(resolveFolderDrop(["C:\\a\\x.txt"], "D:\\b", none, drives)).toMatchObject({ ok: true, op: "copy" });
  });
  it("Ctrl copies and Shift moves, wherever it goes", () => {
    expect(resolveFolderDrop(["C:\\a\\x.txt"], "C:\\b", { ctrl: true, shift: false }, drives).op).toBe("copy");
    expect(resolveFolderDrop(["C:\\a\\x.txt"], "D:\\b", { ctrl: false, shift: true }, drives).op).toBe("move");
    // Archives: moving inside one rearranges it, anything coming from outside copies in.
    const zip = "archive:///h/a.zip!/";
    expect(resolveFolderDrop([zip + "x"], zip + "dir", none, drives)).toMatchObject({ ok: true, op: "move" });
    expect(resolveFolderDrop(["/h/file.txt"], zip + "dir", none, drives)).toMatchObject({ ok: true, op: "copy" });
    expect(resolveFolderDrop([zip + "x"], "/h/out", none, drives)).toMatchObject({ ok: true, op: "copy" });
  });
  it("does nothing when dropped where it already is, unless copying", () => {
    expect(resolveFolderDrop(["C:\\a\\x.txt"], "C:\\a", none, drives).ok).toBe(false);
    expect(resolveFolderDrop(["C:\\a\\x.txt"], "C:\\A\\", none, drives).ok).toBe(false);
    expect(resolveFolderDrop(["C:\\a\\x.txt"], "C:\\a", { ctrl: true, shift: false }, drives)).toMatchObject({ ok: true, op: "copy" });
  });
  it("refuses a folder dropped into itself or something inside it", () => {
    expect(resolveFolderDrop(["C:\\a"], "C:\\a", none, drives).ok).toBe(false);
    expect(resolveFolderDrop(["C:\\a"], "C:\\a\\deep\\er", none, drives).ok).toBe(false);
    expect(resolveFolderDrop(["C:\\a"], "C:\\ab", none, drives).ok).toBe(true);
  });
  it("tells drives apart on unix paths too", () => {
    const unix = [d("/"), d("/mnt/usb")];
    expect(volumeOf("/home/me/x", unix)).toBe("/");
    expect(volumeOf("/mnt/usb/x", unix)).toBe("/mnt/usb");
    expect(resolveFolderDrop(["/home/me/x"], "/mnt/usb/in", none, unix).op).toBe("copy");
  });
});
