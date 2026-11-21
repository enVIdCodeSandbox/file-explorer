// Joining a window to another by dragging it over: what the window underneath is offered, and what becomes of it.

import { create } from "zustand";
import { api } from "../api";
import { isAppLocation, isArchiveLoc, isRemote } from "./path";
import { MAX_PANES } from "./splitLayout";

export interface OfferedTab {
  id: string;
  location: string;
}

export type MergeMode = "tabs" | "panes";

interface MergeState {
  /** A window is being held over this one. */
  hover: boolean;
  /** The window that was dropped here, and its tabs. */
  offer: { source: string; tabs: OfferedTab[] } | null;
}

export const useMerge = create<MergeState>(() => ({ hover: false, offer: null }));

/** A folder a split pane can show (not a server, phone, archive or the app's own places). */
export const fitsPane = (location: string) => !!location && !isAppLocation(location) && !isRemote(location) && !isArchiveLoc(location);

/**
 * What to do with the ticked tabs. As panes, only the folders that fit a pane become panes, up to the room there is;
 * everything else is added as a tab, so nothing ticked is left behind.
 */
export function planMerge(ticked: OfferedTab[], mode: MergeMode, paneRoom: number): { tabs: OfferedTab[]; panes: OfferedTab[] } {
  if (mode === "tabs") return { tabs: ticked, panes: [] };
  const room = Math.max(0, Math.min(paneRoom, MAX_PANES - 1));
  const panes: OfferedTab[] = [];
  const tabs: OfferedTab[] = [];
  for (const t of ticked) (panes.length < room && fitsPane(t.location) ? panes : tabs).push(t);
  return { tabs, panes };
}

/** Who is told what, so the two windows agree: `source` is the dragged window, `target` the one it was dropped on. */
export const handOver = (target: string, tabs: OfferedTab[]) => api.mergeOffer(target, tabs);
