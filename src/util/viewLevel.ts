// Which view (Details, List, icons…) the open folder uses.

import { adv } from "../settings/advanced";
import type { Settings } from "../api";

/**
 * The view for the open folder. Normally the one saved in the settings. With "Use the same view in
 * every folder" off, a view picked from the View menu (`folderView`) only applies until another folder is opened.
 */
export function viewLevelOf(settings: Settings | null | undefined, folderView: number | null): number {
  const saved = settings?.appearance.viewLevel ?? 0;
  if (!settings || adv<boolean>(settings, "files.rememberView")) return saved;
  return folderView ?? saved;
}
