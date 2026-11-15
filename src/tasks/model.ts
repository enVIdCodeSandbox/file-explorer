// Automated tasks: the shapes the backend stores, and the words used to describe them.
// The Rust side is crates/fast-core/src/tasks.rs; names match its JSON.

import { formatDate, formatDuration, formatSize } from "../util/format";

export type Conflict = "skip" | "overwrite" | "overwriteIfNewer" | "keepBoth";
export type SleepPolicy = "resume" | "cancel";
export type Notify = "never" | "failure" | "always";
export type Outcome = "success" | "partial" | "failed" | "cancelled" | "interrupted" | "skipped";

export interface Filter {
  include: string;
  exclude: string;
  olderThanDays: number | null;
  newerThanDays: number | null;
  minBytes: number | null;
  maxBytes: number | null;
  hidden: boolean;
  recursive: boolean;
}

export type Trigger =
  | { type: "manual" }
  | { type: "once"; at: number }
  | { type: "every"; minutes: number; anchor: number }
  | { type: "daily"; time: string; days: number[] }
  | { type: "monthly"; day: number; time: string }
  | { type: "startup" }
  | { type: "onChange"; path: string; quietSecs: number }
  | { type: "driveConnected"; name: string };

export type Action =
  | { type: "copy"; from: string[]; to: string; filter: Filter; conflict: Conflict; keepStructure: boolean }
  | { type: "move"; from: string[]; to: string; filter: Filter; conflict: Conflict; keepStructure: boolean }
  | { type: "delete"; paths: string[]; filter: Filter; permanent: boolean; removeEmptyFolders: boolean; removeFolder: boolean }
  | { type: "compress"; from: string[]; to: string; filter: Filter }
  | { type: "extract"; archive: string; to: string }
  | { type: "sync"; from: string; to: string; mirror: boolean; filter: Filter }
  | { type: "run"; program: string; args: string[]; cwd: string; timeoutSecs: number };

export interface Options {
  onSleep: SleepPolicy;
  catchUp: boolean;
  retries: number;
  retryDelaySecs: number;
  timeoutMinutes: number;
  stopOnError: boolean;
  onlyOnAc: boolean;
  runAsAdmin: boolean;
  notify: Notify;
  skipIfRunning: boolean;
  lowPriority: boolean;
}

export interface Task {
  id: string;
  name: string;
  notes: string;
  enabled: boolean;
  triggers: Trigger[];
  actions: Action[];
  options: Options;
  created: number;
  updated: number;
  resumePending: boolean;
  needsAdmin: boolean;
}

export interface Progress {
  step: number;
  steps: number;
  itemsDone: number;
  itemsTotal: number;
  bytesDone: number;
  bytesTotal: number;
  current: string;
  elapsedSecs: number;
}

export interface RunRecord {
  taskId: string;
  started: number;
  ended: number;
  outcome: Outcome;
  reason: string;
  items: number;
  bytes: number;
  errors: string[];
  attempts: number;
  interruptedBy: string | null;
  elevated: boolean;
  name: string;
}

export interface RunningInfo {
  id: string;
  started: number;
  reason: string;
  elevated: boolean;
  progress: Progress;
}

export interface TasksListing {
  tasks: Task[];
  running: RunningInfo[];
  next: Record<string, number>;
  last: Record<string, RunRecord>;
  enabled: boolean;
}

export interface Stats {
  runs: number;
  successes: number;
  partial: number;
  failures: number;
  cancelled: number;
  interrupted: number;
  skipped: number;
  successRate: number;
  avgSecs: number;
  medianSecs: number;
  p90Secs: number;
  minSecs: number;
  maxSecs: number;
  avgItems: number;
  avgBytes: number;
  avgBytesPerSec: number;
  totalItems: number;
  totalBytes: number;
  lastRun: number | null;
  lastSuccess: number | null;
  lastFailure: number | null;
  lastError: string | null;
  speedTrend: number | null;
}

export interface Estimate {
  secs: number;
  samples: number;
  basis: "speed" | "duration" | "mixed";
}

export interface StatsReply {
  stats: Stats;
  estimate: Estimate | null;
}

export interface Planned {
  action: number;
  kind: string;
  from: string;
  to: string;
  bytes: number;
}

export interface Preview {
  ops: Planned[];
  files: number;
  bytes: number;
  truncated: boolean;
  problems: string[];
}

// ---- Starting points ---------------------------------------------------------------

export const emptyFilter = (): Filter => ({ include: "", exclude: "", olderThanDays: null, newerThanDays: null, minBytes: null, maxBytes: null, hidden: false, recursive: true });

export const defaultOptions = (): Options => ({
  onSleep: "resume",
  catchUp: true,
  retries: 0,
  retryDelaySecs: 5,
  timeoutMinutes: 0,
  stopOnError: true,
  onlyOnAc: false,
  runAsAdmin: false,
  notify: "failure",
  skipIfRunning: true,
  lowPriority: true,
});

export const newTask = (): Task => ({
  id: "",
  name: "",
  notes: "",
  enabled: true,
  triggers: [{ type: "daily", time: "03:00", days: [] }],
  actions: [newAction("copy")],
  options: defaultOptions(),
  created: 0,
  updated: 0,
  resumePending: false,
  needsAdmin: false,
});

export type ActionType = Action["type"];
export type TriggerType = Trigger["type"];

export const ACTION_TYPES: { type: ActionType; label: string; hint: string }[] = [
  { type: "copy", label: "Copy", hint: "Copy files and folders somewhere else." },
  { type: "move", label: "Move", hint: "Move files and folders somewhere else." },
  { type: "delete", label: "Delete", hint: "Delete what matches (to the trash unless told otherwise)." },
  { type: "compress", label: "Compress to ZIP", hint: "Pack files into a ZIP archive." },
  { type: "extract", label: "Extract archive", hint: "Unpack an archive into a folder." },
  { type: "sync", label: "Sync folders", hint: "Make one folder match another." },
  { type: "run", label: "Run a program", hint: "Start a program or script." },
];

export const TRIGGER_TYPES: { type: TriggerType; label: string }[] = [
  { type: "daily", label: "Every day (or chosen days)" },
  { type: "every", label: "Every so often" },
  { type: "monthly", label: "Once a month" },
  { type: "once", label: "Once, at a set time" },
  { type: "startup", label: "When the app starts" },
  { type: "onChange", label: "When a folder changes" },
  { type: "driveConnected", label: "When a drive is connected" },
  { type: "manual", label: "Only when I start it" },
];

export function newAction(type: ActionType): Action {
  switch (type) {
    case "copy":
    case "move":
      return { type, from: [], to: "", filter: emptyFilter(), conflict: "overwrite", keepStructure: true };
    case "delete":
      return { type, paths: [], filter: emptyFilter(), permanent: false, removeEmptyFolders: false, removeFolder: false };
    case "compress":
      return { type, from: [], to: "", filter: emptyFilter() };
    case "extract":
      return { type, archive: "", to: "" };
    case "sync":
      return { type, from: "", to: "", mirror: false, filter: emptyFilter() };
    case "run":
      return { type, program: "", args: [], cwd: "", timeoutSecs: 0 };
  }
}

export function newTrigger(type: TriggerType): Trigger {
  switch (type) {
    case "manual":
      return { type };
    case "once":
      return { type, at: Date.now() + 3600_000 };
    case "every":
      return { type, minutes: 60, anchor: Date.now() };
    case "daily":
      return { type, time: "03:00", days: [] };
    case "monthly":
      return { type, day: 1, time: "03:00" };
    case "startup":
      return { type };
    case "onChange":
      return { type, path: "", quietSecs: 10 };
    case "driveConnected":
      return { type, name: "" };
  }
}

// ---- Words -------------------------------------------------------------------------

export const WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

export function describeEvery(minutes: number): string {
  if (minutes % 1440 === 0) return minutes === 1440 ? "day" : `${minutes / 1440} days`;
  if (minutes % 60 === 0) return minutes === 60 ? "hour" : `${minutes / 60} hours`;
  return minutes === 1 ? "minute" : `${minutes} minutes`;
}

export function describeTrigger(t: Trigger): string {
  switch (t.type) {
    case "manual":
      return "Only when started by hand";
    case "once":
      return `Once, ${formatDate(t.at)}`;
    case "every":
      return `Every ${describeEvery(t.minutes)}`;
    case "daily":
      return t.days.length === 0 || t.days.length === 7 ? `Every day at ${t.time}` : `${[...t.days].sort().map((d) => WEEKDAYS[d]).join(", ")} at ${t.time}`;
    case "monthly":
      return `Day ${t.day} of each month at ${t.time}`;
    case "startup":
      return "When the app starts";
    case "onChange":
      return `When ${t.path || "a folder"} changes`;
    case "driveConnected":
      return t.name ? `When a drive named "${t.name}" is connected` : "When any drive is connected";
  }
}

const lastPart = (p: string) => p.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || p;
const list = (a: string[]) => (a.length === 0 ? "(nothing chosen)" : a.length === 1 ? lastPart(a[0]) : `${lastPart(a[0])} and ${a.length - 1} more`);

export function describeAction(a: Action): string {
  switch (a.type) {
    case "copy":
      return `Copy ${list(a.from)} to ${a.to ? lastPart(a.to) : "…"}`;
    case "move":
      return `Move ${list(a.from)} to ${a.to ? lastPart(a.to) : "…"}`;
    case "delete":
      return `${a.permanent ? "Delete for good" : "Move to the trash"}: ${list(a.paths)}`;
    case "compress":
      return `Compress ${list(a.from)} into ${a.to ? lastPart(a.to) : "…"}`;
    case "extract":
      return `Extract ${a.archive ? lastPart(a.archive) : "…"} into ${a.to ? lastPart(a.to) : "…"}`;
    case "sync":
      return `${a.mirror ? "Mirror" : "Sync"} ${a.from ? lastPart(a.from) : "…"} to ${a.to ? lastPart(a.to) : "…"}`;
    case "run":
      return `Run ${a.program ? lastPart(a.program) : "…"}`;
  }
}

export const OUTCOME_LABEL: Record<Outcome, string> = {
  success: "Done",
  partial: "Done, with problems",
  failed: "Failed",
  cancelled: "Stopped",
  interrupted: "Interrupted",
  skipped: "Not run",
};

export function describeRun(r: RunRecord): string {
  const parts = [OUTCOME_LABEL[r.outcome]];
  if (r.outcome !== "skipped") {
    parts.push(formatDuration(Math.max(0, (r.ended - r.started) / 1000)));
    if (r.items) parts.push(`${r.items} item${r.items === 1 ? "" : "s"}`);
    if (r.bytes) parts.push(formatSize(r.bytes));
  }
  return parts.join(" · ");
}

/** The first problem with a task, in words, or null when it can be saved. */
export function validate(t: Task): string | null {
  if (!t.name.trim()) return "Give the task a name.";
  if (t.triggers.length === 0) return "Choose when it runs (or \"only when I start it\").";
  if (t.actions.length === 0) return "Add at least one step.";
  for (const [i, a] of t.actions.entries()) {
    const n = `Step ${i + 1}`;
    switch (a.type) {
      case "copy":
      case "move":
      case "compress":
        if (a.from.length === 0) return `${n}: choose what to ${a.type === "compress" ? "compress" : a.type}.`;
        if (!a.to.trim()) return `${n}: choose where it goes.`;
        break;
      case "delete":
        if (a.paths.length === 0) return `${n}: choose what to delete.`;
        break;
      case "extract":
        if (!a.archive.trim() || !a.to.trim()) return `${n}: choose the archive and the folder to unpack into.`;
        break;
      case "sync":
        if (!a.from.trim() || !a.to.trim()) return `${n}: choose both folders.`;
        break;
      case "run":
        if (!a.program.trim()) return `${n}: choose a program.`;
        break;
    }
  }
  for (const tr of t.triggers) {
    if (tr.type === "onChange" && !tr.path.trim()) return "Choose the folder to watch.";
    if (tr.type === "every" && tr.minutes < 1) return "\"Every\" needs at least one minute.";
  }
  return null;
}
