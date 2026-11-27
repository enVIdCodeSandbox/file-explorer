import { describe, expect, it } from "vitest";
import { baseName, crumbs, isRemote, isSshLoc, joinPath, maskAddresses, parentPath, remoteLabel, savedServerUrl, setAddressPrivacy, setDeviceNames, splitRemote } from "./path";
import { fieldsFromUrl } from "../components/ConnectServer";

describe("server addresses", () => {
  it("are recognized", () => {
    for (const u of ["sftp://u@h/", "ssh://u@h/", "ftp://h", "dav://h/x", "davs://h:8443/x", "SFTP://h/", "SSH://h/"]) expect(isRemote(u)).toBe(true);
    for (const u of ["/home/me", "C:\\Users", "fast://home", "ftpfolder/x", "http://h/"]) expect(isRemote(u)).toBe(false);
    expect(isSshLoc("ssh://u@h/var/www")).toBe(true);
    expect(isSshLoc("sftp://u@h/var/www")).toBe(false);
  });

  it("split into server and folder", () => {
    expect(splitRemote("sftp://u@h:22/a/b/")).toEqual({ origin: "sftp://u@h:22", path: "/a/b" });
    expect(splitRemote("ftp://h")).toEqual({ origin: "ftp://h", path: "/" });
    expect(splitRemote("ftp://h//")).toEqual({ origin: "ftp://h", path: "/" });
    expect(remoteLabel("sftp://u@h:2222/x")).toBe("u@h:2222");
  });

  it("have a parent, a name and children", () => {
    expect(parentPath("sftp://u@h/a/b")).toBe("sftp://u@h/a");
    expect(parentPath("sftp://u@h/a")).toBe("sftp://u@h/");
    expect(parentPath("sftp://u@h/")).toBeNull();
    expect(parentPath("sftp://u@h")).toBeNull();
    expect(baseName("sftp://u@h/a/b c.txt")).toBe("b c.txt");
    expect(baseName("sftp://u@h/")).toBe("u@h");
    expect(joinPath("sftp://u@h/", "x")).toBe("sftp://u@h/x");
    expect(joinPath("sftp://u@h/a", "x y")).toBe("sftp://u@h/a/x y");
  });

  it("make a trail from the server down", () => {
    expect(crumbs("dav://u@h/docs/2026").map((c) => [c.label, c.path])).toEqual([
      ["u@h", "dav://u@h/"],
      ["docs", "dav://u@h/docs"],
      ["2026", "dav://u@h/docs/2026"],
    ]);
  });

  it("fill the login form", () => {
    expect(fieldsFromUrl("sftp://anna@example.com:2222/home/anna")).toMatchObject({ protocol: "sftp", user: "anna", host: "example.com", port: "2222", path: "/home/anna", tls: false });
    expect(fieldsFromUrl("ssh://anna@example.com:2200/var/www")).toMatchObject({ protocol: "ssh", user: "anna", host: "example.com", port: "2200", path: "/var/www", tls: false });
    expect(fieldsFromUrl("davs://cloud.example.com/remote.php/dav")).toMatchObject({ protocol: "dav", tls: true, host: "cloud.example.com", port: "", user: "" });
    expect(fieldsFromUrl("ftp://files.example.com/")).toMatchObject({ protocol: "ftp", host: "files.example.com", path: "" });
    expect(savedServerUrl({ protocol: "ssh", host: "box", port: 22, user: "me", tls: false, path: "srv" })).toBe("ssh://me@box/srv");
  });
});

describe("address privacy", () => {
  const reset = () => setAddressPrivacy(false, []);
  it("leaves text alone when addresses are shown", () => {
    reset();
    expect(maskAddresses("sftp://me@10.0.0.5:22/docs")).toBe("sftp://me@10.0.0.5:22/docs");
    expect(remoteLabel("sftp://u@h:2222/x")).toBe("u@h:2222");
  });
  it("hides hosts in server addresses, keeping the user and path", () => {
    setAddressPrivacy(true, []);
    expect(maskAddresses("sftp://me@10.0.0.5:22/docs/a")).toBe("sftp://me@•••/docs/a");
    expect(maskAddresses("dav://nas.local/x")).toBe("dav://•••/x");
    reset();
  });
  it("uses the name the person gave a saved server", () => {
    setAddressPrivacy(true, [{ name: "Home NAS", host: "10.0.0.5" }]);
    expect(maskAddresses("sftp://me@10.0.0.5/docs")).toBe("sftp://me@Home NAS/docs");
    expect(remoteLabel("sftp://me@10.0.0.5:22/")).toBe("me@Home NAS");
    reset();
  });
  it("hides UNC hosts, NFS exports and GVFS names", () => {
    setAddressPrivacy(true, []);
    expect(maskAddresses("\\\\192.168.1.5\\games")).toBe("\\\\•••\\games");
    expect(maskAddresses("\\\\nas")).toBe("\\\\•••");
    expect(maskAddresses("nas:/export/data")).toBe("•••:/export/data");
    expect(maskAddresses("/run/user/1000/gvfs/smb-share:server=192.168.1.5,share=x")).toBe("/run/user/1000/gvfs/smb-share:server=•••,share=x");
    reset();
  });
  it("does not touch local paths or drive letters", () => {
    setAddressPrivacy(true, []);
    expect(maskAddresses("C:\\Users\\me\\a.txt")).toBe("C:\\Users\\me\\a.txt");
    expect(maskAddresses("/home/me/notes:/x")).toBe("/home/me/notes:/x");
    reset();
  });
  it("force overrides the global choice", () => {
    reset();
    expect(maskAddresses("\\\\nas\\s", true)).toBe("\\\\•••\\s");
  });
});

describe("plugged-in devices", () => {
  it("are addresses like servers, labelled with the device's name", () => {
    expect(isRemote("mtp://3f9a/Internal storage/DCIM")).toBe(true);
    setDeviceNames([{ name: "Chau's S24+", path: "mtp://3f9a/" }]);
    expect(remoteLabel("mtp://3f9a/Internal storage")).toBe("Chau's S24+");
    expect(baseName("mtp://3f9a/")).toBe("Chau's S24+");
    expect(baseName("mtp://3f9a/Internal storage/DCIM")).toBe("DCIM");
    expect(parentPath("mtp://3f9a/Internal storage")).toBe("mtp://3f9a/");
    expect(parentPath("mtp://3f9a/")).toBeNull();
    expect(joinPath("mtp://3f9a/Internal storage", "DCIM")).toBe("mtp://3f9a/Internal storage/DCIM");
    expect(remoteLabel("mtp://unknown/")).toBe("Device");
  });
});
