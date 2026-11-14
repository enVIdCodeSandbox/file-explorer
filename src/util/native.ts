// The few things only a phone's own system can do: opening a file in another
// app, the share sheet, asking for access to storage, and coloring the system
// bars. On Android they go through a small object the app's activity adds to
// the page (src-tauri/gen/android/.../MainActivity.kt). Everywhere else the
// object is absent and each function says so. Nothing here depends on the rest
// of the app, so anything may import it.

/** What the Android activity offers the page. Every method is synchronous and takes and returns plain values. */
interface NativeBridge {
  maker(): string;
  model(): string;
  hasAllFilesAccess(): boolean;
  requestAllFilesAccess(): void;
  /** Opens a file in another app. Returns "" when an app took it, else why not. */
  openFile(path: string): string;
  /** Shares files (paths separated by line breaks) through the system's share sheet. Returns "" or why not. */
  shareFiles(paths: string): string;
  /** Colors the strips behind the status and navigation bars, and picks dark or light icons for them. */
  setBars(color: string, lightBackground: boolean): void;
}

const bridge = (): NativeBridge | undefined => (globalThis as unknown as { FastNative?: NativeBridge }).FastNative;

export const isMobileOs = (os: string | undefined): boolean => os === "android" || os === "ios";

/** The maker of an Android device from the text its browser identifies itself with. Galaxy model numbers start with SM-. */
export function makerFromUserAgent(ua: string): string {
  if (/\bSM-[A-Z]\d|SAMSUNG|\bGT-[A-Z]\d/i.test(ua)) return "samsung";
  if (/\bPixel\b/i.test(ua)) return "google";
  if (/iPhone|iPad|iPod/i.test(ua)) return "apple";
  return "";
}

/** Who made this device, lower case, or "" when unknown. */
export function deviceMaker(): string {
  try {
    const told = bridge()?.maker();
    if (told) return told.toLowerCase();
  } catch {
    /* an older build of the activity: fall through to the browser's own description */
  }
  return makerFromUserAgent(typeof navigator === "undefined" ? "" : navigator.userAgent);
}

/** Android only: whether the app may read all of shared storage. Null where the question does not apply. */
export function storageAccess(): boolean | null {
  try {
    const b = bridge();
    return b ? b.hasAllFilesAccess() : null;
  } catch {
    return null;
  }
}

/** Android only: open the system page where the person allows access to all files. */
export function askStorageAccess(): void {
  try {
    bridge()?.requestAllFilesAccess();
  } catch {
    /* nothing to ask on this system */
  }
}

/** Open a file in another app. Null: this system has no bridge (use the usual way). "": opened. Otherwise why it could not be. */
export function openOutside(path: string): string | null {
  try {
    const b = bridge();
    return b ? b.openFile(path) : null;
  } catch (e) {
    return String(e);
  }
}

/** Share files through the system's share sheet. Null: not available here. "": shown. Otherwise why not. */
export function shareOutside(paths: string[]): string | null {
  try {
    const b = bridge();
    return b ? b.shareFiles(paths.join("\n")) : null;
  } catch (e) {
    return String(e);
  }
}

/** Android only: make the strips behind the system bars match the app's background. */
export function colorSystemBars(color: string, lightBackground: boolean): void {
  try {
    bridge()?.setBars(color, lightBackground);
  } catch {
    /* an older build of the activity, or not Android */
  }
}
