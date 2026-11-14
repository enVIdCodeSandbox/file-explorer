import { describe, expect, it } from "vitest";
import type { Device } from "../api";
import { deviceIcon, deviceStatus, isMainDevice } from "./devices";

const base: Device = { id: "usb:1-2", name: "Pixel 8", maker: "Google", kind: "phone", path: null, connectable: false, open: null, note: "" };

describe("devices", () => {
  it("says what state a device is in", () => {
    expect(deviceStatus({ ...base, path: "/run/user/1000/gvfs/mtp:host=x" })).toMatch(/open to browse/);
    expect(deviceStatus({ ...base, connectable: true })).toMatch(/connect/i);
    expect(deviceStatus({ ...base, open: "Image Capture" })).toMatch(/system/);
    expect(deviceStatus({ ...base, kind: "input" })).toBe("Keyboard, mouse or receiver");
  });
  it("lists phones and cameras as the main devices", () => {
    expect(isMainDevice(base)).toBe(true);
    expect(isMainDevice({ ...base, kind: "camera" })).toBe(true);
    expect(isMainDevice({ ...base, kind: "printer" })).toBe(false);
    expect(deviceIcon({ ...base, kind: "camera" })).toBe("camera");
    expect(deviceIcon({ ...base, kind: "other" })).toBe("usb");
  });
});
