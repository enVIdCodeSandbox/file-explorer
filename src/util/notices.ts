// Wording for the action notices ("Copied 3 items").

import { baseName } from "./path";

export type NoticeKind = "copy" | "cut" | "move" | "delete" | "rename" | "create" | "undo" | "info" | "error";

export interface Notice {
  id: number;
  kind: NoticeKind;
  title: string;
  detail?: string;
  at: number;
  /** Still showing as a pop-up. */
  popup: boolean;
}

/** "report.pdf" for one item (quoted), "3 items" for several. */
export function describeItems(paths: string[]): string {
  return paths.length === 1 ? `"${baseName(paths[0])}"` : `${paths.length} items`;
}

const VERB: Record<"copy" | "move", [string, string]> = { copy: ["Copied", "copy"], move: ["Moved", "move"] };

export function transferNotice(op: "copy" | "move", sources: string[], destDir: string): { title: string; detail: string } {
  return { title: `${VERB[op][0]} ${describeItems(sources)}`, detail: `to ${baseName(destDir) || destDir}` };
}
