// Running a backend tool (search in files, compare, duplicates…) with progress and cancel.

import { api, on, type GrepHit, type ToolSpec } from "../api";

let next = 1;

export interface ToolRun<T> {
  result: Promise<T>;
  cancel: () => void;
}

export function runTool<T>(
  spec: ToolSpec,
  handlers: { progress?: (done: number, total: number, note: string) => void; hits?: (hits: GrepHit[]) => void } = {},
): ToolRun<T> {
  const id = next++ + Math.floor(Date.now() % 1_000_000) * 1000;
  const off: (() => void)[] = [];
  const result = (async () => {
    off.push(...(await Promise.all([
      on("tool-progress", (e) => e.id === id && handlers.progress?.(e.done, e.total, e.note)),
      on("tool-hits", (e) => e.id === id && handlers.hits?.(e.hits)),
    ])));
    try {
      return (await api.toolRun(id, spec)) as T;
    } finally {
      off.forEach((f) => f());
    }
  })();
  return { result, cancel: () => void api.toolCancel(id).catch(() => {}) };
}

/** An error from a cancelled run. */
export const wasCancelled = (e: unknown) => String(e).includes("cancelled");
