import { describe, expect, it } from "vitest";
import { networkThumbs } from "./network";

const withMode = (v: unknown) => ({ advanced: v === undefined ? {} : { "thumbs.networkDrives": v } }) as never;

describe("thumbnails on network shares", () => {
  it("automatic mode follows the cable", () => {
    expect(networkThumbs(withMode(undefined), "wired")).toBe(true);
    expect(networkThumbs(withMode("auto"), "wireless")).toBe(false);
    expect(networkThumbs(withMode("auto"), "unknown")).toBe(false);
  });
  it("always on and always off ignore the connection", () => {
    expect(networkThumbs(withMode("on"), "wireless")).toBe(true);
    expect(networkThumbs(withMode("off"), "wired")).toBe(false);
  });
  it("an old yes/no setting still means what it did", () => {
    expect(networkThumbs(withMode(true), "wireless")).toBe(true);
    expect(networkThumbs(withMode(false), "wired")).toBe(true); // the old default (off) becomes automatic
    expect(networkThumbs(withMode(false), "wireless")).toBe(false);
  });
});
