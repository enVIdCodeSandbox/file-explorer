// Changing icons: the dialog that picks an app icon or a picture, the right-click entry on any icon, and the list in
// Settings where each icon can be changed or put back.

import { useEffect, useMemo, useRef, useState } from "react";
import { useStore } from "../store";
import { ICONS_KEY, MAX_PICTURE, iconMap, prettyName, safeSvg, withIcon, type IconChoice } from "../util/customIcons";
import type { IconTarget } from "../util/iconTarget";
import { ContextMenu, type MenuItem, type MenuState } from "./ContextMenu";
import { PackManager, PackPicker, usePacks } from "./IconCollections";
import { FileIcon, Icon, iconNames } from "./Icons";

/** Set (or with null, reset) the choice for a slot. */
export function setIcon(slot: string, choice: IconChoice | null, alsoReset: string[] = []) {
  useStore.getState().updateSettings((st) => {
    let raw: unknown = st.advanced?.[ICONS_KEY];
    for (const s of alsoReset) raw = withIcon(raw, s, null);
    st.advanced = { ...st.advanced, [ICONS_KEY]: withIcon(raw, slot, choice) };
  });
}

/** Whether this slot has been changed. */
export function isChanged(slot: string): boolean {
  return !!iconMap(useStore.getState().settings?.advanced?.[ICONS_KEY])[slot];
}

/** The menu entries for changing one icon, to add to any right-click menu. */
export function iconMenuItems(t: IconTarget): MenuItem[] {
  const open = useStore.getState().openIconPicker;
  return [
    { id: "icon-change", label: "Change icon…", icon: "star", run: () => open(t) },
    ...(isChanged(t.slot) ? [{ id: "icon-reset", label: "Use the default icon", run: () => setIcon(t.slot, null) }] : []),
  ];
}

/** A picture shrunk to a small PNG, as a data address. */
export async function shrinkPicture(file: File): Promise<string> {
  const url = URL.createObjectURL(file);
  try {
    const img = new Image();
    img.src = url;
    await img.decode();
    const w = img.naturalWidth || MAX_PICTURE;
    const h = img.naturalHeight || MAX_PICTURE;
    const scale = Math.min(1, MAX_PICTURE / Math.max(w, h));
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(1, Math.round(w * scale));
    canvas.height = Math.max(1, Math.round(h * scale));
    canvas.getContext("2d")!.drawImage(img, 0, 0, canvas.width, canvas.height);
    return canvas.toDataURL("image/png");
  } finally {
    URL.revokeObjectURL(url);
  }
}

export function IconPickerDialog() {
  const target = useStore((s) => s.iconPicker);
  if (!target) return null;
  return <Picker key={target.slot} target={target} />;
}

function Picker({ target }: { target: IconTarget }) {
  const close = () => useStore.setState({ iconPicker: null });
  const [all, setAll] = useState(false);
  const [error, setError] = useState("");
  const [filter, setFilter] = useState("");
  const file = useRef<HTMLInputElement>(null);
  const names = useMemo(() => iconNames().filter((n) => !filter || n.toLowerCase().includes(filter.toLowerCase()) || prettyName(n).toLowerCase().includes(filter.toLowerCase())), [filter]);
  // More icons, from collections that are installed on request (previewed over the network first).
  const packs = usePacks();
  const usePackIcon = (svg: string) => {
    const clean = safeSvg(svg);
    if (clean) apply({ svg: clean });
    else setError("That icon cannot be used.");
  };
  const slot = all && target.group ? target.group.slot : target.slot;
  const apply = (choice: IconChoice | null) => {
    // Changing them all replaces this one's own choice too.
    setIcon(slot, choice, all && target.group ? [target.slot] : []);
    close();
  };
  useEffect(() => {
    const key = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, []);
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="dialog icon-picker" role="dialog" aria-label={`Change the icon: ${target.label}`}>
        <h3>Change the icon: {target.label}</h3>
        {target.group && (
          <div className="icon-scope" role="radiogroup">
            <label><input type="radio" name="scope" checked={!all} onChange={() => setAll(false)} /> Only {target.label}</label>
            <label><input type="radio" name="scope" checked={all} onChange={() => setAll(true)} /> {target.group.label}</label>
          </div>
        )}
        <p className="dialog-message">Choose one of the app's icons, or use a picture of your own. Every place that shows this icon changes with it.</p>
        <div className="icon-picker-bar">
          <input className="text-input" placeholder="Find an icon" value={filter} onChange={(e) => setFilter(e.target.value)} aria-label="Find an icon" />
          <button className="secondary" onClick={() => file.current?.click()}>Use a picture…</button>
          <input
            ref={file}
            type="file"
            accept="image/*"
            hidden
            onChange={(e) => {
              const f = e.target.files?.[0];
              e.target.value = "";
              if (!f) return;
              shrinkPicture(f).then((img) => apply({ img }), () => setError("That file could not be read as a picture."));
            }}
          />
        </div>
        {error && <div className="dialog-error" role="alert">{error}</div>}
        {filter.trim().length >= 2 && <h4 className="icon-section-title">App icons{names.length ? ` · ${names.length} ${names.length === 1 ? "match" : "matches"}` : ""}</h4>}
        <div className="icon-grid" role="listbox" aria-label="App icons">
          {names.map((n) => (
            <button key={n} className="icon-cell" title={prettyName(n)} aria-label={prettyName(n)} onClick={() => apply({ app: n })}>
              <Icon name={n} size={22} plain />
            </button>
          ))}
          {names.length === 0 && <div className="side-empty">No icon matches.</div>}
        </div>
        <PackPicker filter={filter} packs={packs} onPick={usePackIcon} />
        <div className="dialog-buttons">
          <button className="secondary" onClick={() => apply(null)}>Use the default</button>
          <button className="primary" onClick={close}>Cancel</button>
        </div>
      </div>
    </div>
  );
}

/**
 * Right-click on any icon that nothing else claimed: offer to change it. Menus of the app (files, folders, drives…) run
 * first and are left alone; they have their own entry.
 */
export function IconContextMenu() {
  const [menu, setMenu] = useState<MenuState | null>(null);
  useEffect(() => {
    const on = (e: MouseEvent) => {
      if (e.defaultPrevented) return;
      const el = (e.target as Element | null)?.closest?.("[data-icon-slot]") as HTMLElement | null;
      const slot = el?.dataset.iconSlot;
      if (!el || !slot) return;
      e.preventDefault();
      const label = slotLabel(slot);
      setMenu({ x: e.clientX, y: e.clientY, items: iconMenuItems({ slot, label }) });
    };
    document.addEventListener("contextmenu", on);
    return () => document.removeEventListener("contextmenu", on);
  }, []);
  return menu ? <ContextMenu menu={menu} onClose={() => setMenu(null)} /> : null;
}

const PLACES: [string, string][] = [["downloads", "Downloads folder"], ["documents", "Documents folder"], ["pictures", "Pictures folder"], ["music", "Music folder"], ["videos", "Videos folder"], ["desktop", "Desktop folder"]];
const KINDS: [string, string][] = [["folder", "Folders"], ["file", "Other files"], ["image", "Pictures"], ["video", "Videos"], ["audio", "Music and sound"], ["archive", "Archives"], ["code", "Code"], ["doc", "Documents"], ["sheet", "Spreadsheets"], ["slides", "Presentations"], ["exe", "Programs"]];
const NAMED: Record<string, string> = {
  home: "Home",
  pc: "This PC",
  trash: "Recycle Bin and Trash",
  chart: "Disk usage",
  broom: "Smart Clear",
  drive: "Drives",
  network: "Network drives and servers",
  usb: "USB drives and other devices",
  phone: "Phones",
  camera: "Cameras",
  terminal: "Terminal",
  pin: "Pin",
  star: "Star",
  folderOpen: "Open folder",
  archive: "Archive",
  lock: "Lock",
  shield: "Shield",
  search: "Search",
  settings: "Settings",
  clock: "Tasks and clock",
};

/** A name people can read for a slot. */
export function slotLabel(slot: string): string {
  const [type, rest = ""] = [slot.slice(0, slot.indexOf(":")), slot.slice(slot.indexOf(":") + 1)];
  if (type === "icon") return NAMED[rest] ?? prettyName(rest);
  if (type === "kind") return KINDS.find(([k]) => k === rest)?.[1] ?? prettyName(rest);
  if (type === "place") return PLACES.find(([k]) => k === rest)?.[1] ?? prettyName(rest);
  if (type === "drive") return `Drive ${rest.toUpperCase()}`;
  if (type === "server") return `Server ${rest}`;
  if (type === "device") return "A phone or device";
  return slot;
}

/** The list in Settings: every kind of icon, with a button to change it and one to put it back. */
export function IconsPanel() {
  const raw = useStore((s) => s.settings?.advanced?.[ICONS_KEY]);
  const map = iconMap(raw);
  const open = useStore((s) => s.openIconPicker);
  const [filter, setFilter] = useState("");
  const q = filter.trim().toLowerCase();
  const groups: { title: string; rows: { slot: string; label: string; preview: React.ReactNode }[] }[] = [
    {
      title: "Folders and files",
      rows: [
        ...KINDS.map(([k, label]) => ({ slot: `kind:${k}`, label, preview: <FileIcon kind={k} size={22} /> })),
        ...PLACES.map(([k, label]) => ({ slot: `place:${k}`, label, preview: <FileIcon kind="folder" name={k} size={22} /> })),
      ],
    },
    {
      title: "Navigation, drives and devices",
      rows: Object.keys(NAMED).map((n) => ({ slot: `icon:${n}`, label: NAMED[n], preview: <Icon name={n} size={22} /> })),
    },
    {
      title: "Every other icon",
      rows: iconNames().filter((n) => !NAMED[n]).map((n) => ({ slot: `icon:${n}`, label: prettyName(n), preview: <Icon name={n} size={22} /> })),
    },
  ];
  // Drives that were given an icon of their own
  const drives = Object.keys(map).filter((s) => /^(drive|server|device):/.test(s));
  const changed = Object.keys(map).length;
  return (
    <div className="icons-panel">
      <div className="icons-bar">
        <input className="text-input" placeholder="Find an icon" aria-label="Find an icon" value={filter} onChange={(e) => setFilter(e.target.value)} />
        <button className="secondary" disabled={!changed} onClick={() => useStore.getState().updateSettings((st) => void (st.advanced = { ...st.advanced, [ICONS_KEY]: "" }))}>
          Put every icon back{changed ? ` (${changed} changed)` : ""}
        </button>
      </div>
      <p className="muted">You can also right-click a drive, a navigation item or any icon and choose Change icon.</p>
      <details open={!!q && "icon collections".includes(q)}>
        <summary>Icon collections (install more icons)</summary>
        <PackManager />
      </details>
      {groups.map((g) => {
        const rows = [...g.rows, ...(g.title.startsWith("Navigation") ? drives.map((slot) => ({ slot, label: slotLabel(slot), preview: <span className="icon-custom-preview"><Icon name={slot.startsWith("device:") ? "phone" : slot.startsWith("server:") ? "network" : "drive"} slot={slot} size={22} /></span> })) : [])].filter((r) => !q || r.label.toLowerCase().includes(q) || r.slot.includes(q));
        if (!rows.length) return null;
        return (
          <details key={g.title} open={g.title !== "Every other icon" || !!q}>
            <summary>{g.title}</summary>
            <div className="icons-list">
              {rows.map((r) => (
                <div key={r.slot} className="icons-row" data-changed={map[r.slot] ? "" : undefined}>
                  <span className="icons-preview">{r.preview}</span>
                  <span className="icons-name">{r.label}</span>
                  <button className="secondary" onClick={() => open({ slot: r.slot, label: r.label })}>Change…</button>
                  <button className="secondary" disabled={!map[r.slot]} onClick={() => setIcon(r.slot, null)}>Reset</button>
                </div>
              ))}
            </div>
          </details>
        );
      })}
    </div>
  );
}
