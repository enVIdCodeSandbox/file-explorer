import { describe, expect, it } from "vitest";
import { classifyRemoteFile, posixShellQuote, sshNanoCommand } from "./remoteFileKind";

describe("classifyRemoteFile", () => {
  it("recognizes text configs/backups vs byte-based archives/binaries and leaves ambiguous names unknown", () => {
    expect(classifyRemoteFile("shadow.bak")).toBe("text");
    expect(classifyRemoteFile("gshadow.bak")).toBe("text");
    expect(classifyRemoteFile("config.yaml")).toBe("text");
    expect(classifyRemoteFile("Makefile")).toBe("text");
    expect(classifyRemoteFile("alternatives.tar.0")).toBe("binary");
    expect(classifyRemoteFile("apt.extended_states.1.gz")).toBe("binary");
    expect(classifyRemoteFile("photo.png")).toBe("binary");
    expect(classifyRemoteFile("mystery")).toBe("unknown");
  });
});

describe("sshNanoCommand", () => {
  it("quotes paths with spaces/apostrophes and falls back to sudo when unreadable", () => {
    expect(posixShellQuote("/tmp/my 'file'.txt")).toBe("'/tmp/my '\\''file'\\''.txt'");
    const cmd = sshNanoCommand("/var/backups/shadow.bak");
    expect(cmd.startsWith(" ")).toBe(true);
    expect(cmd.endsWith("\r")).toBe(true);
    expect(cmd).toContain("if [ -r '/var/backups/shadow.bak' ]");
    expect(cmd).toContain("nano -- '/var/backups/shadow.bak'");
    expect(cmd).toContain("sudo nano -- '/var/backups/shadow.bak'");
    expect(sshNanoCommand("/etc/hosts", true)).toBe(" sudo nano -- '/etc/hosts'\r");
  });
});
