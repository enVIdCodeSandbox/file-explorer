// Drive names shown in the app: the system's name, unless the person renamed
// the drive here, and optionally without the server address.

import type { Drive, Settings } from "../api";

type Appearance = Settings["appearance"];

const stripAddress = (name: string): string =>
  name
    .replace(/\s*\((?:\\\\|\/\/)[^()]*\)/g, "") // "(\\192.168.1.5)"
    .replace(/\s*\(\d{1,3}(?:\.\d{1,3}){3}\)/g, "") // "(192.168.1.5)"
    .replace(/\s{2,}/g, " ")
    .trim();

/** Does this drive hide its server address? A per-drive choice beats the global one. */
export function hidesAddress(d: Drive, a: Appearance): boolean {
  if (d.media !== "network") return false;
  return a.driveNames?.[d.mount]?.hideAddress ?? a.hideNetworkAddress;
}

/** The name to show. A local rename only counts while the system name is unchanged. */
export function driveName(d: Drive, a: Appearance): string {
  const alias = a.driveNames?.[d.mount];
  if (alias?.name && alias.base === d.name) return alias.name;
  const base = hidesAddress(d, a) ? stripAddress(d.name) : d.name;
  return base || d.name;
}

/** The share path to show, or null when hidden. */
export function driveRemote(d: Drive, a: Appearance): string | null {
  if (!d.remote || hidesAddress(d, a)) return null;
  return d.remote;
}
