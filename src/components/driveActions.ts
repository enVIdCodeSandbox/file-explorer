// Actions on drives and network locations, shared by the sidebar, This PC and
// the command bar so every right-click menu offers the same things.

import { api, type Drive, type SavedServer } from "../api";
import { ask, useStore } from "../store";
import { analyzeLocation, clearLocation, savedServerUrl } from "../util/path";
import { driveName, hidesAddress } from "../util/drives";
import { ICONS_KEY, iconMap, withIcon } from "../util/customIcons";
import { serverSlot, serverTarget } from "../util/iconTarget";
import type { MenuItem } from "./ContextMenu";
import { iconMenuItems } from "./IconPicker";

/** Pick a drive letter that is not in use, for the Map network drive dialog. */
function freeLetters(drives: Drive[]): string[] {
  const used = new Set(drives.map((d) => d.mount.slice(0, 1).toUpperCase()));
  return "ZYXWVUTSRQPONMLKJIHGFED".split("").filter((l) => !used.has(l));
}

/** Ask for a share and connect it (`auto`: no drive letter, like "Add a network location"). */
export async function mapNetworkDrive(drives: Drive[], auto: boolean, os: string | undefined) {
  const { showToast, refreshDrives } = useStore.getState();
  const windows = os === "windows";
  const r = await ask({
    title: auto || !windows ? "Add a network location" : "Map network drive",
    message: windows ? "Folder or computer to connect to, like \\\\server\\share" : "Server and share to connect to, like smb://server/share",
    input: windows ? "\\\\" : "smb://",
    select: auto || !windows ? undefined : { label: "Drive letter", options: freeLetters(drives).map((l) => ({ value: `${l}:`, label: `${l}:` })) },
    buttons: [
      { id: "ok", label: "Connect", primary: true },
      { id: "cancel", label: "Cancel" },
    ],
  });
  if (!r || r.button !== "ok" || !r.input.trim()) return;
  const remote = r.input.trim();
  const letter = auto || !windows ? null : r.selected || null;
  const done = (mount: string | null) => {
    showToast("Connected.");
    refreshDrives();
    if (mount) useStore.getState().navigate(mount);
  };
  try {
    done(await api.networkMap(remote, letter));
    return;
  } catch (e) {
    if (!windows || !needsSignIn(String(e))) return void showToast(explainNetError(String(e)));
  }
  // Windows would have asked for a name and password itself, but not with no console to ask on: ask here and try again.
  let note = "That location asks you to sign in.";
  for (let tries = 0; tries < 3; tries++) {
    const who = await ask({
      title: "Sign in to connect",
      message: `${note} Enter a user name and password for ${remote}. For a computer in a domain, use DOMAIN\\name.`,
      input: "",
      inputLabel: "User name",
      secret: { label: "Password" },
      buttons: [
        { id: "ok", label: "Connect", primary: true },
        { id: "cancel", label: "Cancel" },
      ],
    });
    if (!who || who.button !== "ok" || !who.input.trim()) return;
    try {
      return done(await api.networkMap(remote, letter, who.input.trim(), who.secret));
    } catch (e) {
      if (!needsSignIn(String(e))) return void showToast(explainNetError(String(e)));
      note = "That name or password was not accepted.";
    }
  }
  showToast("That name or password was not accepted.");
}

/** Windows' own error numbers for "who are you?": not signed in, wrong name or password, the sign-in was cancelled. */
export function needsSignIn(message: string): boolean {
  if (/system error 1219\b/i.test(message)) return false; // already connected under another name: signing in again cannot help
  return /system error (5|86|1203|1223|1244|1312|1326|1331|1909|2220)\b/i.test(message) || /logon failure|password is not correct|access is denied/i.test(message);
}

/** Say what a `net use` error means. */
export function explainNetError(message: string): string {
  if (/system error 1219\b/i.test(message)) return "Windows already has a connection to that computer under another user name. Disconnect the other connection first, then try again.";
  if (/system error 53\b/i.test(message)) return "Windows could not find that computer or share. Check the name, and that the computer is on and reachable.";
  if (/system error 67\b/i.test(message)) return "That computer was found, but it has no share with that name.";
  return message;
}

export function disconnectDrive(d: Drive) {
  const { showToast, refreshDrives } = useStore.getState();
  return api
    .networkDisconnect(d.mount)
    .then(() => (showToast("Disconnected."), refreshDrives()))
    .catch((err) => showToast(String(err)));
}

export function ejectDrive(d: Drive) {
  const { showToast, refreshDrives } = useStore.getState();
  return api
    .driveEject(d.mount)
    .then(() => (showToast("It is safe to remove the drive."), refreshDrives()))
    .catch((err) => showToast(String(err)));
}

/** Remember a local name, or a show/hide-address choice, for one drive. */
export function setDriveAlias(d: Drive, patch: { name?: string; hideAddress?: boolean | null }) {
  useStore.getState().updateSettings((s) => {
    const cur = s.appearance.driveNames?.[d.mount] ?? { name: "", base: d.name, hideAddress: null };
    const next = { ...cur, ...patch };
    if (patch.name !== undefined) next.base = d.name;
    const names = { ...(s.appearance.driveNames ?? {}) };
    if (!next.name && next.hideAddress == null) delete names[d.mount];
    else names[d.mount] = next;
    s.appearance.driveNames = names;
  });
}

/** True for drives that can be detached (USB sticks, SD cards, external disks). */
export const isDetachable = (d: Drive) => d.removable || d.media === "removable";

/**
 * What a drive's right-click menu offers beyond Open: disk usage, detaching a
 * removable drive, and connecting or disconnecting network locations.
 */
export function driveMenuItems(d: Drive, extra: { os: string | undefined; drives: Drive[]; appearance: Parameters<typeof hidesAddress>[1] }): MenuItem[] {
  const { os, drives, appearance } = extra;
  const windows = os === "windows";
  const items: MenuItem[] = [
    { id: "analyze", label: "Analyze disk usage", icon: "chart", run: () => useStore.getState().navigate(analyzeLocation(d.mount)) },
  ];
  if (d.online && !d.readOnly && d.media !== "network") {
    items.push({ id: "smartClear", label: "Smart Clear: free up space", icon: "broom", run: () => useStore.getState().navigate(clearLocation(d.mount)) });
  }
  if (isDetachable(d)) {
    items.push({ id: "eject", label: windows ? "Eject" : "Safely remove", icon: "drive", separatorBefore: true, disabled: !d.online, run: () => void ejectDrive(d) });
  }
  if (d.media === "network") {
    items.push({
      id: "disconnect",
      label: windows ? "Disconnect network drive" : "Disconnect",
      icon: "network",
      separatorBefore: true,
      run: () => void disconnectDrive(d),
    });
    items.push({
      id: "address",
      label: hidesAddress(d, appearance) ? "Show server address" : "Hide server address",
      run: () => setDriveAlias(d, { hideAddress: !hidesAddress(d, appearance) }),
    });
  }
  items.push({ id: "addNetwork", label: "Add a network location…", icon: "network", separatorBefore: true, run: () => void mapNetworkDrive(drives, true, os) });
  if (windows && drives.some((x) => x.media === "network")) {
    items.push({ id: "signInAgain", label: d.media === "network" ? "Sign in again…" : "Sign in again to network drives…", icon: "network", run: () => useStore.getState().openSignIn(d.media === "network" ? [d.mount] : []) });
  }
  if (windows) items.push({ id: "mapDrive", label: "Map network drive…", icon: "network", run: () => void mapNetworkDrive(drives, false, os) });
  items.push({
    id: "disconnectOther",
    label: windows ? "Disconnect a network drive" : "Disconnect a network location",
    icon: "network",
    disabled: !drives.some((x) => x.media === "network"),
    children: drives.filter((x) => x.media === "network").map((x) => ({ id: `disc-${x.mount}`, label: driveName(x, appearance), run: () => void disconnectDrive(x) })),
  });
  // Last, and apart from the everyday entries.
  if (d.media !== "network" || !d.mount.includes("://")) {
    if (d.online) {
      items.push({ id: "permissions", label: "Permissions", icon: "shield", separatorBefore: true, run: () => useStore.getState().showPermissions(d.mount) });
    }
    items.push({ id: "lock", label: "Lock with passcode…", icon: "lock", separatorBefore: !d.online, run: () => useStore.getState().openLockDialog(d.mount) });
  }
  return items;
}

/** Menu for the empty part of This PC. */
export function networkMenuItems(drives: Drive[], os: string | undefined, appearance: Parameters<typeof hidesAddress>[1]): MenuItem[] {
  const windows = os === "windows";
  const network = drives.filter((d) => d.media === "network");
  return [
    { id: "connectServer", label: "Connect to server…", icon: "network", run: () => useStore.getState().openConnect() },
    { id: "addNetwork", label: "Add a network location…", icon: "network", run: () => void mapNetworkDrive(drives, true, os) },
    ...(windows ? [{ id: "mapDrive", label: "Map network drive…", icon: "network", run: () => void mapNetworkDrive(drives, false, os) } as MenuItem] : []),
    ...(windows && network.length > 0 ? [{ id: "signInAgain", label: "Sign in again to network drives…", icon: "network", run: () => useStore.getState().openSignIn() } as MenuItem] : []),
    {
      id: "disconnect",
      label: windows ? "Disconnect network drive" : "Disconnect network location",
      icon: "network",
      disabled: network.length === 0,
      children: network.map((d) => ({ id: `disc-${d.mount}`, label: driveName(d, appearance), run: () => void disconnectDrive(d) })),
    },
  ];
}

/** Move any custom icon override when a saved server is renamed. */
export function migrateServerIcon(oldName: string, newName: string) {
  const oldSlot = serverSlot(oldName);
  const newSlot = serverSlot(newName);
  if (oldSlot === newSlot) return;
  useStore.getState().updateSettings((s) => {
    const map = iconMap(s.advanced[ICONS_KEY]);
    const choice = map[oldSlot];
    if (!choice) return;
    let raw = withIcon(s.advanced[ICONS_KEY], oldSlot, null);
    raw = withIcon(raw, newSlot, choice);
    s.advanced[ICONS_KEY] = raw;
  });
}

/** Prompt to rename a saved server and persist the change. */
export async function renameServer(sv: SavedServer) {
  const { setServers, showToast } = useStore.getState();
  const r = await ask({
    title: "Rename server",
    message: "Change the name shown for this server.",
    input: sv.name,
    buttons: [
      { id: "ok", label: "Rename", primary: true },
      { id: "cancel", label: "Cancel" },
    ],
  });
  const next = r?.button === "ok" ? r.input.trim() : "";
  if (!next || next === sv.name) return;
  try {
    setServers(await api.serversRename(sv.name, next));
    migrateServerIcon(sv.name, next);
  } catch (e) {
    showToast(String(e));
  }
}

/** Shared context menu for saved servers in both the sidebar and This PC. */
export function serverMenuItems(sv: SavedServer, opts?: { includeOpen?: boolean }): MenuItem[] {
  const url = savedServerUrl(sv);
  const { navigate, openConnect, setServers, showToast } = useStore.getState();
  return [
    ...(opts?.includeOpen ? [{ id: "open", label: "Open", run: () => navigate(url) }] : []),
    { id: "server-rename", label: "Rename…", icon: "rename", shortcut: "F2", separatorBefore: !!opts?.includeOpen, run: () => void renameServer(sv) },
    { id: "server-login", label: "Log in again…", separatorBefore: true, run: () => openConnect({ saved: sv }) },
    { id: "server-logout", label: "Log out", run: () => void api.remoteDisconnect(url).then(() => showToast(`Logged out of ${sv.name}.`)).catch((e) => showToast(String(e))) },
    { id: "server-forget", label: "Remove from the list", separatorBefore: true, run: () => void api.serversRemove(sv.name).then(setServers).catch((e) => showToast(String(e))) },
    ...iconMenuItems(serverTarget(sv.name)).map((m, i) => (i === 0 ? { ...m, separatorBefore: true } : m)),
  ];
}
