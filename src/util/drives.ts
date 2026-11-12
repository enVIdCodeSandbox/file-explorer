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

/** The drive letter ("S:") of a Windows drive, or null where drives have none. */
export const driveLetter = (d: Drive): string | null => {
  if (/^[A-Za-z]:/.test(d.mount)) return d.mount.slice(0, 2).toUpperCase();
  // An elevated process cannot see the user's mapped letters, so those shares
  // are mounted by their UNC path and keep the letter in their name.
  const m = d.media === "network" ? /\(\s*([A-Za-z]:)\s*\)\s*$/.exec(d.name) : null;
  return m ? m[1].toUpperCase() : null;
};

const LETTER_TAIL = /\s*\(\s*[A-Za-z]:\s*\)\s*$/;

/** A name without a trailing "(S:)". The letter is the drive's identity, so it is never part of an editable name. */
export const withoutLetter = (name: string): string => name.replace(LETTER_TAIL, "").trim();

/**
 * The name to show. A local rename only counts while the system name is
 * unchanged. The drive letter is always appended from the drive itself, so it
 * cannot be renamed or removed, only the name before it.
 */
export function driveName(d: Drive, a: Appearance): string {
  const letter = driveLetter(d);
  const alias = a.driveNames?.[d.mount];
  const custom = alias?.name && alias.base === d.name ? withoutLetter(alias.name) : "";
  const base = custom || withoutLetter(hidesAddress(d, a) ? stripAddress(d.name) : d.name) || withoutLetter(d.name);
  return letter ? `${base || "Drive"} (${letter})` : base || d.name;
}

/** The share path to show, or null when hidden. */
export function driveRemote(d: Drive, a: Appearance): string | null {
  if (!d.remote || hidesAddress(d, a)) return null;
  return d.remote;
}
