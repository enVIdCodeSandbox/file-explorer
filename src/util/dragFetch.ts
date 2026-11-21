// Dragging items that live on a phone or a server out of the window. Other programs can only receive files that are
// on this computer, so these are fetched first, and the system drag starts when they are here.

import { api } from "../api";
import { adv } from "../settings/advanced";
import type { Settings } from "../api";
import { awaitStage } from "../store";
import { isTooBig, tooBigMb } from "./stageRemote";

export interface DragFetch {
  /** The job that is fetching, to stop it. */
  id: number;
  /** The files on this computer, one per dragged item. Rejects with an Error whose message can be shown as it is. */
  done: Promise<string[]>;
}

export const dragLimitMb = (settings: Settings | null | undefined): number => {
  const mb = Number(adv<number>(settings, "drag.remoteMb"));
  return Number.isFinite(mb) && mb > 0 ? mb : 2048;
};

/** The words for a file that is over the limit. */
export const tooBigMessage = (mb: number | null, limit: number) =>
  `${mb !== null ? `That is about ${mb.toLocaleString()} MB, ` : "That is "}more than the ${limit.toLocaleString()} MB that can be fetched for a drag out of the window. To move it somewhere else, copy it and paste it inside the app (it goes straight from the source to the destination), or raise the limit in Settings → Advanced → Files and lists.`;

export async function startDragFetch(paths: string[], limitMb: number): Promise<DragFetch> {
  let id: number;
  try {
    id = await api.remoteStageStart(paths, limitMb, true);
  } catch (e) {
    throw new Error(isTooBig(e) ? tooBigMessage(tooBigMb(e), limitMb) : String(e).replace(/^Error: /, ""));
  }
  const done = awaitStage(id).then((p) => {
    if (p.phase === "cancelled") throw new Error("Stopped.");
    const files = (p.result ?? "").split("\n").filter(Boolean);
    if (p.phase === "failed" || !files.length) throw new Error(p.errors[0]?.message || "The files could not be fetched.");
    return files;
  });
  return { id, done };
}

export const stopDragFetch = (id: number) => api.deleteControl(id, "cancel").catch(() => {});
