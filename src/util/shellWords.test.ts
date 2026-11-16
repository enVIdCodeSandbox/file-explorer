import { describe, expect, it } from "vitest";
import { shellFor } from "./shellWords";

describe("shells typed in the address bar", () => {
  it("opens Command Prompt for cmd on Windows, however it is spelled", () => {
    for (const t of ["cmd", "CMD", " cmd ", "cmd.exe"]) expect(shellFor(t, "windows")).toEqual({ shell: "cmd", name: "Command Prompt" });
  });

  it("knows the other Windows shells", () => {
    expect(shellFor("powershell", "windows")?.shell).toBe("powershell");
    expect(shellFor("pwsh", "windows")?.shell).toBe("pwsh");
    expect(shellFor("wt", "windows")?.shell).toBe("wt");
    expect(shellFor("terminal", "windows")?.shell).toBe("wt");
  });

  it("leaves paths and unknown words alone", () => {
    expect(shellFor("C:\\Windows\\cmd", "windows")).toBeNull();
    expect(shellFor("cmd/x", "windows")).toBeNull();
    expect(shellFor("Downloads", "windows")).toBeNull();
    expect(shellFor("", "windows")).toBeNull();
    expect(shellFor("~", "linux")).toBeNull();
  });

  it("uses the configured terminal elsewhere, with the shell asked for", () => {
    expect(shellFor("terminal", "linux")).toEqual({ name: "Terminal" });
    expect(shellFor("zsh", "macos")).toEqual({ loginShell: "zsh", name: "zsh" });
    expect(shellFor("cmd", "linux")).toBeNull();
  });
});
