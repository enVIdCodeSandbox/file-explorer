import { describe, expect, it } from "vitest";
import { handleTerminalToggleKey, windowsScreen } from "./Terminal";

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
  it("prevents default browser/WebView handling when toggling with the terminal shortcut", () => {
    let prevented = false;
    let stopped = false;
    let toggled = 0;
    const ok = handleTerminalToggleKey(
      {
        key: "`",
        code: "Backquote",
        ctrlKey: true,
        metaKey: false,
        shiftKey: false,
        altKey: false,
        preventDefault: () => (prevented = true),
        stopPropagation: () => (stopped = true),
      },
      null,
      () => {
        toggled++;
      },
    );
    expect(ok).toBe(true);
    expect(prevented).toBe(true);
    expect(stopped).toBe(true);
    expect(toggled).toBe(1);
  });
});
