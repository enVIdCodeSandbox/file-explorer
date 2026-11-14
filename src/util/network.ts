// Thumbnails of files on network shares: always, never, or only while on a cable.

import { adv } from "../settings/advanced";

export type Link = "wired" | "wireless" | "unknown";

/** Whether thumbnails of network shares are wanted now. */
export function networkThumbs(settings: Parameters<typeof adv>[0], link: Link): boolean {
  // Older versions saved a yes/no here.
  const saved = settings?.advanced?.["thumbs.networkDrives"];
  const mode = saved === true ? "on" : saved === false ? "auto" : adv<string>(settings, "thumbs.networkDrives");
  return mode === "on" || (mode === "auto" && link === "wired");
}
