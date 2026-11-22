// What "pause all", "resume all" and "stop all" in the list of recent actions act on.

import type { Progress } from "../api";

/** Copies and moves that can be paused or resumed right now. A job still counting files or waiting in line has nothing to pause yet. */
export function pausableTransfers(transfers: Record<number, Progress>, pause: boolean): Progress[] {
  return Object.values(transfers).filter((p) => (pause ? p.phase === "running" : p.phase === "paused"));
}

/** Which of the three buttons to offer: Pause when something is running, Resume when something is paused, Stop when anything is live. */
export function bulkChoices(transfers: Record<number, Progress>, liveCount: number): { pause: number; resume: number; stop: number } {
  return { pause: pausableTransfers(transfers, true).length, resume: pausableTransfers(transfers, false).length, stop: liveCount };
}
