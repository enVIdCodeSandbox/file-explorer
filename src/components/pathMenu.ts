// The right-click menu for one folder or file shown outside a folder window (a sidebar row, a recent, a frequent
// folder): the same entries as in a folder, then the system's own entries once they arrive.

import { api } from "../api";
import { compressPaths, deletePaths, extractArchive, isArchiveName, openFile, promptText, renamePath, useStore } from "../store";
import { analyzeLocation, baseName, isArchiveLoc, isRemote, parentPath } from "../util/path";
import { taskFromSelection } from "./tasksUtil";
import type { MenuItem, MenuState } from "./ContextMenu";
import { useShellItems } from "./shellMenu";
import { useRef } from "react";

export interface MenuTarget {
  path: string;
  isDir: boolean;
}

/** The entries for one path. `shell` are the system's entries (empty until they have arrived); `extra` come first. */
export function pathMenuItems(t: MenuTarget, shell: MenuItem[] = [], extra: MenuItem[] = []): MenuItem[] {
  const { pins, places, drives, settings, navigate, newTab, togglePin, setClipboard, pasteInto, showToast, startTransfer } = useStore.getState();
  const paths = [t.path];
  const name = baseName(t.path);
  const pinned = pins.some((x) => x.path === t.path);
  const remote = isRemote(t.path);
  const here = parentPath(t.path);
  const sendTo: MenuItem[] = [
    ...places.filter((x) => ["Desktop", "Documents", "Downloads"].includes(x.label)).map((x) => ({ label: x.label, path: x.path, icon: "folderOpen" })),
    ...drives.filter((d) => d.online && !d.readOnly && d.mount !== "/").map((d) => ({ label: d.name, path: d.mount, icon: d.media === "network" ? "network" : "drive" })),
  ]
    .filter((d) => d.path !== here)
    .map((d) => ({ id: `sendto-${d.path}`, label: d.label, icon: d.icon, run: () => void startTransfer(paths, d.path, "copy") }));
  const terminal = (mode: "here" | "admin") => api.openTerminal(t.isDir ? t.path : (here ?? t.path), mode).catch((e) => showToast(String(e)));
  const items: (MenuItem | false | null | undefined)[] = [
    { id: "open", label: "Open", icon: t.isDir ? "folderOpen" : undefined, run: () => (t.isDir ? navigate(t.path) : void openFile(t.path)) },
    t.isDir && { id: "openNewTab", label: "Open in new tab", run: () => newTab(t.path) },
    t.isDir && !remote && { id: "openNewWindow", label: "Open in new window", run: () => void api.openWindow(t.path).catch((e) => showToast(String(e))) },
    !t.isDir && { id: "openWith", label: "Open with…", run: () => useStore.getState().chooseApp(t.path) },
    !t.isDir && !!here && { id: "reveal", label: "Show in folder", icon: "folderOpen", run: () => navigate(here) },
    ...shell.map((m, i) => (i === 0 ? { ...m, separatorBefore: true } : m)),
    { id: "pin", label: pinned ? "Unpin from sidebar" : "Pin to sidebar", icon: "pin", separatorBefore: shell.length === 0, run: () => togglePin(t.path, !t.isDir) },
    ...extra,
    { id: "copyPath", label: "Copy as path", icon: "copy", run: () => void navigator.clipboard.writeText(t.path) },
    {
      id: "share",
      label: "Share",
      icon: "share",
      children: [
        { id: "share-link", label: "Temporary download link…", icon: "share", run: () => useStore.setState({ shareFor: paths }) },
        { id: "share-path", separatorBefore: true, label: "Copy as path", run: () => void navigator.clipboard.writeText(t.path) },
        { id: "share-name", label: "Copy name", run: () => void navigator.clipboard.writeText(name) },
      ],
    },
    sendTo.length > 0 && { id: "sendTo", label: "Send to", icon: "forward", children: sendTo },
    { id: "archive-zip", label: "Compress to ZIP", icon: "archive", run: () => void compressPaths(paths) },
    !t.isDir && !isArchiveLoc(t.path) && isArchiveName(name) && { id: "archive-here", label: "Extract here", icon: "archive", run: () => void extractArchive(t.path, true) },
    !t.isDir && !isArchiveLoc(t.path) && isArchiveName(name) && { id: "archive-to", label: `Extract to "${name.replace(/(\.tar)?\.[^.]+$/i, "")}"`, icon: "archive", run: () => void extractArchive(t.path, false) },
    { id: "cut", label: "Cut", icon: "cut", separatorBefore: true, run: () => setClipboard({ paths, op: "move" }) },
    { id: "copy", label: "Copy", icon: "copy", run: () => setClipboard({ paths, op: "copy" }) },
    t.isDir && { id: "paste", label: "Paste into folder", icon: "paste", run: () => void pasteInto(t.path) },
    { id: "createShortcut", label: "Create shortcut", separatorBefore: true, run: () => void api.createShortcut(t.path).then(() => showToast("Shortcut created.")).catch((e) => showToast(String(e))) },
    { id: "delete", label: "Delete", icon: "trash", run: () => void deletePaths(paths) },
    { id: "deletePermanent", label: "Delete permanently", danger: true, run: () => void deletePaths(paths, true) },
    {
      id: "rename",
      label: "Rename",
      icon: "rename",
      run: () =>
        void (async () => {
          const next = await promptText("Rename", name);
          if (next && next !== name) await renamePath(t.path, next);
        })(),
    },
    !remote && { id: "terminal", label: "Open terminal here", icon: "terminal", separatorBefore: true, run: () => void terminal("here") },
    !remote && { id: "terminalAdmin", label: "Open terminal as administrator", run: () => void terminal("admin") },
    t.isDir && { id: "analyze", label: "Analyze disk usage", icon: "chart", run: () => navigate(analyzeLocation(t.path)) },
    !remote && { id: "automate", label: "Automate with a task…", icon: "clock", separatorBefore: true, run: () => useStore.getState().openTasks(taskFromSelection(paths, t.isDir ? t.path : here)) },
    { id: "properties", label: "Properties", icon: "info", separatorBefore: remote, run: () => useStore.setState({ properties: paths }) },
    t.isDir && !remote && { id: "lock", label: "Lock with passcode…", icon: "lock", separatorBefore: true, run: () => useStore.getState().openLockDialog(t.path) },
  ];
  const hidden = new Set(settings?.contextMenu.hidden ?? []);
  const localOnly = new Set(["pin", "share", "sendTo", "archive", "createShortcut", "deletePermanent", "terminal", "terminalAdmin", "analyze", "properties", "lock"]);
  return items.filter((i): i is MenuItem => !!i && !hidden.has(i.id) && !hidden.has(i.id.replace(/-.*/, "")) && !(remote && localOnly.has(i.id.replace(/-.*/, ""))));
}

/**
 * A function that shows the menu for a path in the host's own menu state: at once with the built-in entries, then again
 * with the system's entries when they arrive (it never waits for them).
 */
export function usePathMenu(setMenu: (m: MenuState | null) => void) {
  const { fetchFor } = useShellItems();
  const ticket = useRef(0);
  return (e: { preventDefault: () => void; clientX: number; clientY: number }, t: MenuTarget, extra: MenuItem[] = []) => {
    e.preventDefault();
    const [x, y] = [e.clientX, e.clientY];
    const mine = ++ticket.current;
    setMenu({ x, y, items: pathMenuItems(t, [], extra) });
    const settings = useStore.getState().settings;
    if (!settings?.general.shellExtras || isRemote(t.path)) return;
    void fetchFor([t.path], false).then((shell) => {
      if (mine !== ticket.current || shell.length === 0) return;
      setMenu({ x, y, items: pathMenuItems(t, shell, extra) });
    });
  };
}
