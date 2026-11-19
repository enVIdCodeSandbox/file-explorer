// Which view (Details, List, icons…) the open folder uses.

import { isPerFolder } from "./folderMemory";
import type { Settings } from "../api";

/**
 * The view for the open folder. Normally the one saved in the settings. With "Use the same view in
 * every folder" off, each folder has its own view (`folderView`, put in place when the folder is opened).
 */
export function viewLevelOf(settings: Settings | null | undefined, folderView: number | null): number {
  const saved = settings?.appearance.viewLevel ?? 0;
  if (!settings || !isPerFolder(settings, "view")) return saved;
  return folderView ?? saved;
}
