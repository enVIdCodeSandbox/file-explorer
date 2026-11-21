// Copying from a server or phone: whether the files are also fetched for other programs at that moment.
// Pasting inside the app never needs this (it goes straight from the source to the destination).

import type { Settings } from "../api";
import { adv } from "../settings/advanced";

export type StageRemote = { mode: "off" } | { mode: "any" } | { mode: "small"; mb: number };

/** The answer `remote_stage_start` gives when the items are over the limit it was handed. */
export const TOO_BIG_TO_STAGE = "too-big-to-stage";

export function stageRemote(settings: Settings | null | undefined): StageRemote {
  // This used to be an on/off switch; a saved "on" means the careful choice, "off" is still off.
  // Read as stored: the settings reader would discard a true/false from the old version.
  const raw: unknown = settings?.advanced?.["clipboard.stageRemote"];
  const mode = raw === false ? "off" : raw === true ? "small" : raw;
  if (mode === "off") return { mode: "off" };
  if (mode === "any") return { mode: "any" };
  const mb = Number(adv<string | number | boolean>(settings, "clipboard.stageRemoteMb"));
  return { mode: "small", mb: Number.isFinite(mb) && mb > 0 ? mb : 50 };
}

export const isTooBig = (e: unknown) => String(e).includes(TOO_BIG_TO_STAGE);

/** The size, in MB, that a "too big" answer carries (`too-big-to-stage:1234`). */
export const tooBigMb = (e: unknown): number | null => {
  const m = /too-big-to-stage:(\d+)/.exec(String(e));
  return m ? Number(m[1]) : null;
};
