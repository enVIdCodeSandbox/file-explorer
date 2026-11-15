// Names for the ways a copy can be carried out.

import type { CopyMethod } from "../api";

export const METHOD_NAME: Record<CopyMethod, string> = {
  native: "Built-in engine",
  robocopy: "Robocopy",
  rsync: "rsync",
  rclone: "rclone",
  smb: "SMB-tuned (Robocopy with compression)",
  nfs: "NFS-tuned (rsync, whole file)",
};

export const METHOD_NOTE: Record<CopyMethod, string> = {
  native: "Always available. Chooses its own streams and buffers for each job.",
  robocopy: "Windows only.",
  rsync: "macOS and Linux, when rsync is installed.",
  rclone: "Any system, when rclone is installed.",
  smb: "Windows shares with Robocopy; macOS and Linux with rsync.",
  nfs: "macOS and Linux, for NFS shares, when rsync is installed.",
};

export const ALL_METHODS: CopyMethod[] = ["native", "robocopy", "rsync", "rclone", "smb", "nfs"];

/** The kind of work a job is, as Settings > Advanced > Transfers groups them. */
export function jobGroup(action: string, permanent: boolean): string {
  switch (action) {
    case "delete":
      return permanent ? "Delete" : "Move to the Recycle Bin / Trash";
    case "compress":
      return "Compress";
    case "extract":
      return "Unpack";
    case "pack":
    case "unpack":
      return "Copy into or out of an archive";
    case "update":
      return "Update";
    default:
      return "Servers and phones";
  }
}

/** Remember the method the latest job of a kind used (only when it changed). */
export function noteMethod(
  get: () => { lastMethods: Record<string, { method: string; what: string }> },
  set: (p: { lastMethods: Record<string, { method: string; what: string }> }) => void,
  group: string,
  method: string,
  what: string,
): void {
  const cur = get().lastMethods[group];
  if (cur?.method === method && cur.what === what) return;
  set({ lastMethods: { ...get().lastMethods, [group]: { method, what } } });
}
