import { describe, expect, it } from "vitest";
import type { Drive } from "../api";
import { useStore } from "../store";
import { driveMenuItems, networkMenuItems } from "./driveActions";

const drive = (mount: string, media: string) => ({ mount, media, name: mount, total: 1, available: 1, online: true, removable: false, readOnly: false, fs: "smb" }) as unknown as Drive;
const appearance = { driveNames: {}, hideNetworkAddress: true } as never;

describe("signing in again to network drives", () => {
  const drives = [drive("C:\\", "fixed"), drive("S:\\", "network"), drive("T:\\", "network")];

  it("a network drive's menu offers it for that drive, and starts with it ticked", () => {
    const items = driveMenuItems(drives[1], { os: "windows", drives, appearance });
    const item = items.find((i) => i.id === "signInAgain")!;
    expect(item.label).toBe("Sign in again…");
    item.run?.();
    expect(useStore.getState().signIn).toEqual({ preselect: ["S:\\"] });
  });

  it("the This PC menu offers it for all of them", () => {
    const item = networkMenuItems(drives, "windows", appearance).find((i) => i.id === "signInAgain")!;
    item.run?.();
    expect(useStore.getState().signIn).toEqual({ preselect: [] });
  });

  it("is not offered with no network drives, or off Windows", () => {
    expect(networkMenuItems([drive("C:\\", "fixed")], "windows", appearance).some((i) => i.id === "signInAgain")).toBe(false);
    expect(networkMenuItems(drives, "linux", appearance).some((i) => i.id === "signInAgain")).toBe(false);
  });
});
