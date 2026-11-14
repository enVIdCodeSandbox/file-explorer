// Wording for the action notices ("Copied 3 items").

import { baseName } from "./path";

export type NoticeKind = "copy" | "cut" | "move" | "delete" | "rename" | "create" | "undo" | "info" | "error";

export interface Notice {
  id: number;
  kind: NoticeKind;
  title: string;
  detail?: string;
  at: number;
  /** What the expanded view lists, as label and value (the same layout a transfer's card uses). */
  facts?: [string, string][];
  /** Names of the items concerned, shown in the expanded view (the first few). */
  names?: string[];
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

/** What an expanded notice can say about a set of items; `extra` adds action-specific rows. */
export function itemFacts(action: string, paths: string[], extra: [string, string][] = []): { facts: [string, string][]; names: string[] } {
  const places = [...new Set(paths.map((p) => p.replace(/[\\/][^\\/]*$/, "") || p))];
  const facts: [string, string][] = [["Action", action], ["Items", String(paths.length)]];
  if (places.length === 1) facts.push(["Location", places[0]]);
  else if (places.length > 1) facts.push(["Locations", `${places.length} folders`]);
  facts.push(...extra);
  return { facts, names: paths.map((p) => baseName(p)) };
}
