// What the "Change icon" dialog is asked to change.

export interface IconTarget {
  /** The icon's place in the settings (see customIcons.ts). */
  slot: string;
  /** What to call it: "Drives", "Home"… */
  label: string;
  /** A bigger group the same choice can be applied to instead, e.g. all drives rather than this one. */
  group?: { slot: string; label: string };
}

/** The icon for one drive, or for all of them. */
export function driveTarget(mount: string, name: string, media?: string): IconTarget {
  return {
    slot: `drive:${mount.toLowerCase()}`,
    label: name,
    group: { slot: media === "network" ? "icon:network" : "icon:drive", label: media === "network" ? "All network drives" : "All drives" },
  };
}

/** The icon for one of the app's named icons. */
export const iconTarget = (name: string, label: string): IconTarget => ({ slot: `icon:${name}`, label });

const KIND_NAMES: Record<string, string> = { phone: "All phones", camera: "All cameras", usb: "All other devices" };

/** The icon for one phone, camera or other device, or for every device of its kind. */
export function deviceTarget(id: string, name: string, icon: string): IconTarget {
  return { slot: deviceSlot(id), label: name, group: { slot: `icon:${icon}`, label: KIND_NAMES[icon] ?? "All of these" } };
}

/** The icon for one saved server, or for all servers and network drives. */
export function serverTarget(name: string): IconTarget {
  return { slot: serverSlot(name), label: name, group: { slot: "icon:network", label: "All servers and network drives" } };
}

export const deviceSlot = (id: string) => `device:${id}`;
export const serverSlot = (name: string) => `server:${name.toLowerCase()}`;
