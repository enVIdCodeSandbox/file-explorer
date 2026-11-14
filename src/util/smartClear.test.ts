import { describe, expect, it } from "vitest";
import type { ClearItem, ClearKind } from "../api";
import { NO_FILTER, idleText, initialSelection, matching, pathsToDelete, percentOf, pickPercent, totalBytes } from "./smartClear";

const NOW = 1_800_000_000;
const item = (name: string, kind: ClearKind, size: number, daysAgo: number, also: string[] = []): ClearItem => ({
  path: `/home/me/${name}`,
  also,
  name,
  size,
  used: daysAgo < 0 ? 0 : NOW - daysAgo * 86_400,
  kind,
  isDir: false,
  group: null,
  note: "",
});

const found = [
  item("thesis-backup.zip", "largeOld", 600, 400),
  item("cache-a", "cache", 100, 3),
  item("gone.txt", "trash", 50, 20, ["/home/me/gone.txt.trashinfo"]),
  item("setup.exe", "installers", 150, 90),
  item("session.tmp", "temp", 100, 10),
];

describe("what is ticked to begin with", () => {
  it("is only what can be recreated or was already thrown away", () => {
    expect([...initialSelection(found)].sort()).toEqual(["/home/me/cache-a", "/home/me/gone.txt", "/home/me/session.tmp"]);
  });
});

describe("clearing a percentage", () => {
  it("takes the safest kinds first and stops once the share is covered", () => {
    // 1000 bytes on offer; 20% is 200: the temporary file, then the cache.
    expect([...pickPercent(found, 20)]).toEqual(["/home/me/session.tmp", "/home/me/cache-a"]);
    // 25% needs the Trash as well, and still leaves the person's own files alone.
    expect(pickPercent(found, 25).has("/home/me/gone.txt")).toBe(true);
    expect(pickPercent(found, 25).has("/home/me/setup.exe")).toBe(false);
    // Past what is safe, it moves on to what wants a look, in order of caution.
    expect([...pickPercent(found, 30)].at(-1)).toBe("/home/me/setup.exe");
  });

  it("picks nothing at 0 and everything at 100", () => {
    expect(pickPercent(found, 0).size).toBe(0);
    expect(pickPercent(found, 100).size).toBe(found.length);
    expect(pickPercent([], 50).size).toBe(0);
  });

  it("within a kind, takes what was left alone the longest first", () => {
    const logs = [item("new.log", "logs", 100, 40), item("ancient.log", "logs", 100, 900), item("old.log", "logs", 100, 200)];
    expect([...pickPercent(logs, 50)]).toEqual(["/home/me/ancient.log", "/home/me/old.log"]);
  });

  it("reports the share that is ticked", () => {
    expect(percentOf(found, new Set(["/home/me/thesis-backup.zip"]))).toBe(60);
    expect(percentOf(found, new Set())).toBe(0);
    expect(percentOf([], new Set())).toBe(0);
  });
});

describe("filters", () => {
  it("narrow by kind, age, size and text", () => {
    const names = (f: Partial<typeof NO_FILTER>) => matching(found, { ...NO_FILTER, ...f }, NOW).map((i) => i.name);
    expect(names({})).toHaveLength(5);
    expect(names({ kinds: ["cache", "trash"] })).toEqual(["cache-a", "gone.txt"]);
    expect(names({ olderDays: 60 })).toEqual(["thesis-backup.zip", "setup.exe"]);
    expect(names({ minBytes: 150 })).toEqual(["thesis-backup.zip", "setup.exe"]);
    expect(names({ text: "SETUP" })).toEqual(["setup.exe"]);
    expect(names({ olderDays: 60, minBytes: 200 })).toEqual(["thesis-backup.zip"]);
  });

  it("never treat something of unknown age as old", () => {
    expect(matching([item("mystery", "largeOld", 10, -1)], { ...NO_FILTER, olderDays: 1 }, NOW)).toEqual([]);
  });
});

describe("what is handed to the delete", () => {
  it("includes the notes the Trash keeps beside its items", () => {
    expect(pathsToDelete([found[2]])).toEqual(["/home/me/gone.txt", "/home/me/gone.txt.trashinfo"]);
    expect(totalBytes(found)).toBe(1000);
  });
});

describe("idle time in words", () => {
  it("reads naturally at every scale", () => {
    expect(idleText(NOW - 3600, NOW)).toBe("used today");
    expect(idleText(NOW - 86_400, NOW)).toBe("1 day ago");
    expect(idleText(NOW - 86_400 * 200, NOW)).toBe("6 months ago");
    expect(idleText(NOW - 86_400 * 800, NOW)).toBe("2 years ago");
    expect(idleText(0, NOW)).toBe("");
  });
});
