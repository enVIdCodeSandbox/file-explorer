import { describe, expect, it } from "vitest";
import { phoneCrumbs, phoneParent, phoneTitle, rootOf } from "./phonePath";

const roots = [
  { name: "Internal storage", mount: "/storage/emulated/0" },
  { name: "SD card", mount: "/storage/1A2B-3C4D" },
];

describe("paths on a phone", () => {
  it("finds the storage a path is on", () => {
    expect(rootOf("/storage/emulated/0/Download/a.txt", roots)?.name).toBe("Internal storage");
    expect(rootOf("/storage/1A2B-3C4D", roots)?.name).toBe("SD card");
    expect(rootOf("/storage/emulated/01", roots)).toBeNull(); // a different folder that only starts the same
    expect(rootOf("/data/local", roots)).toBeNull();
  });

  it("picks the deepest storage when one sits inside another", () => {
    const nested = [{ name: "Device", mount: "/" }, ...roots];
    expect(rootOf("/storage/emulated/0/DCIM", nested)?.name).toBe("Internal storage");
    expect(rootOf("/etc", nested)?.name).toBe("Device");
  });

  it("stops going up at the top of a storage", () => {
    expect(phoneParent("/storage/emulated/0/Download", roots)).toBe("/storage/emulated/0");
    expect(phoneParent("/storage/emulated/0", roots)).toBeNull();
    expect(phoneParent("/storage/emulated/0/", roots)).toBeNull();
    // Outside every storage it behaves like any path.
    expect(phoneParent("/data/local", roots)).toBe("/data");
  });

  it("starts breadcrumbs at the storage's name", () => {
    expect(phoneCrumbs("/storage/emulated/0/Pictures/Screenshots", roots)).toEqual([
      { label: "Internal storage", path: "/storage/emulated/0" },
      { label: "Pictures", path: "/storage/emulated/0/Pictures" },
      { label: "Screenshots", path: "/storage/emulated/0/Pictures/Screenshots" },
    ]);
    expect(phoneTitle("/storage/1A2B-3C4D", roots)).toBe("SD card");
    expect(phoneTitle("/storage/1A2B-3C4D/Music", roots)).toBe("Music");
  });

  it("leaves servers and archives as they are", () => {
    const server = phoneCrumbs("sftp://me@host/home/me", roots);
    expect(server[server.length - 1].path).toBe("sftp://me@host/home/me");
    expect(server.length).toBeGreaterThan(1);
    expect(phoneParent("sftp://me@host/", roots)).toBeNull();
  });

  it("works with Windows drives when the phone layout is shown on a computer", () => {
    const win = [{ name: "Local Disk (C:)", mount: "C:\\" }];
    expect(phoneParent("C:\\Users", win)).toBe("C:\\");
    expect(phoneParent("C:\\", win)).toBeNull();
    expect(phoneCrumbs("C:\\Users\\me", win).map((c) => c.label)).toEqual(["Local Disk (C:)", "Users", "me"]);
  });
});
