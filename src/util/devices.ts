// What to do when a phone, camera or other device is opened.

import { api, type Device, type DeviceKind } from "../api";
import { useStore } from "../store";

export const KIND_LABEL: Record<DeviceKind, string> = {
  phone: "Phone or tablet",
  camera: "Camera",
  printer: "Printer",
  audio: "Audio device",
  input: "Keyboard, mouse or receiver",
  bluetooth: "Bluetooth adapter",
  other: "USB device",
};

export const isMainDevice = (d: Device) => d.kind === "phone" || d.kind === "camera";

export const deviceIcon = (d: Device) => (d.kind === "phone" ? "phone" : d.kind === "camera" ? "camera" : "usb");

/** One line saying what state the device is in. */
export function deviceStatus(d: Device): string {
  if (d.path) return "Ready · open to browse files";
  if (d.connectable) return "Open to connect";
  if (d.open) return "Opens in the system's file tool";
  return KIND_LABEL[d.kind];
}

/** Open a device the best way available: in the app, after connecting it, or in the system's tool. */
export async function openDevice(d: Device) {
  const { navigate, showToast, refreshDrives } = useStore.getState();
  if (d.path) return navigate(d.path);
  if (d.connectable) {
    showToast(`Opening ${d.name}… unlock it if it asks.`);
    try {
      const path = await api.connectDevice(d.id);
      refreshDrives();
      if (path) navigate(path);
      else showToast(`${d.name} is connected but has not shown its files yet. Try again in a moment.`);
    } catch (e) {
      showToast(String(e));
    }
    return;
  }
  if (d.open) {
    try {
      await api.openDeviceInSystem(d.open);
    } catch (e) {
      showToast(String(e));
    }
    return;
  }
  showToast(d.note || `There are no files to open on ${d.name}.`);
}
