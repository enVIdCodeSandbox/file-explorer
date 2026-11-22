import { describe, expect, it } from "vitest";
import type { Drive } from "../api";
import { driveSpace, visibleDrives } from "./drives";

const d = (mount: string, media: string, total = 1000, available = 250, online = true) => ({ mount, media, total, available, online, name: mount }) as unknown as Drive;
const list = [d("C:\\", "fixed"), d("D:\\", "removable"), d("S:\\", "network")];

describe("which drives show", () => {
  it("shows both kinds, only local ones, only network ones, or none", () => {
    expect(visibleDrives(list, { showLocalDrives: true, showNetworkDrives: true }).map((x) => x.mount)).toEqual(["C:\\", "D:\\", "S:\\"]);
    expect(visibleDrives(list, { showLocalDrives: true, showNetworkDrives: false }).map((x) => x.mount)).toEqual(["C:\\", "D:\\"]);
    expect(visibleDrives(list, { showLocalDrives: false, showNetworkDrives: true }).map((x) => x.mount)).toEqual(["S:\\"]);
    expect(visibleDrives(list, { showLocalDrives: false, showNetworkDrives: false })).toEqual([]);
  });
  it("shows everything for settings saved before these switches existed", () => {
    expect(visibleDrives(list, {} as never)).toHaveLength(3);
  });
});

describe("the bar under a drive", () => {
  it("fills with the used share or the free share, as chosen on This PC", () => {
    const used = driveSpace(d("C:\\", "fixed", 1000, 250), "used");
    expect(used.fraction).toBeCloseTo(0.75);
    expect(used.text).toMatch(/used$/);
    const free = driveSpace(d("C:\\", "fixed", 1000, 250), "free");
    expect(free.fraction).toBeCloseTo(0.25);
    expect(free.text).toMatch(/free$/);
  });
  it("marks a nearly full drive and has nothing for one that is not there", () => {
    expect(driveSpace(d("C:\\", "fixed", 1000, 50), "used").nearlyFull).toBe(true);
    expect(driveSpace(d("C:\\", "fixed", 1000, 600), "used").nearlyFull).toBe(false);
    expect(driveSpace(d("E:\\", "removable", 0, 0, false), "used").known).toBe(false);
  });
});
