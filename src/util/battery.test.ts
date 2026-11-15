import { describe, expect, it } from "vitest";
import { isDevicePath, isLowBattery } from "./battery";

describe("device thumbnails", () => {
  it("knows device paths", () => {
    expect(isDevicePath("mtp://abc/DCIM")).toBe(true);
    expect(isDevicePath("MTP://abc/")).toBe(true);
    expect(isDevicePath("sftp://h/x")).toBe(false);
  });
  it("pauses only on battery below 20%", () => {
    expect(isLowBattery({ charging: false, level: 0.1 })).toBe(true);
    expect(isLowBattery({ charging: true, level: 0.1 })).toBe(false);
    expect(isLowBattery({ charging: false, level: 0.5 })).toBe(false);
    expect(isLowBattery(null)).toBe(false);
  });
});
