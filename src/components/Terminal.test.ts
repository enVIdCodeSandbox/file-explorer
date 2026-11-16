import { describe, expect, it } from "vitest";
import { windowsScreen } from "./Terminal";

describe("the terminal screen on Windows", () => {
  it("is told a ConPTY is behind it, with the real build", () => {
    expect(windowsScreen({ os: "windows", windowsBuild: 22621 })).toEqual({ windowsPty: { backend: "conpty", buildNumber: 22621 } });
  });
  it("assumes a recent Windows 10 when the build is unknown", () => {
    expect(windowsScreen({ os: "windows" }).windowsPty?.buildNumber).toBe(19045);
  });
  it("is left alone on other systems", () => {
    expect(windowsScreen({ os: "linux" })).toEqual({});
    expect(windowsScreen(null)).toEqual({});
  });
});
