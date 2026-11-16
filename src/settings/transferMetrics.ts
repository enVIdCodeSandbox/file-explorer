// What the copy window can show. Each line has its own switch under Settings > Advanced > "Copy window";
// a line that is switched off is not drawn and is not kept up to date.

export interface TransferMetric {
  id: string;
  label: string;
  /** On unless the person turns it off. The extra ones that are off by default are there to be switched on. */
  on: boolean;
  description: string;
}

export const TRANSFER_METRICS: TransferMetric[] = [
  { id: "graph", label: "Speed graph", on: true, description: "The graph of files and megabytes per second. Off, it is not drawn and its history is not collected." },
  { id: "controls", label: "Priority, speed limit and time limit", on: true, description: "The boxes for changing a running transfer." },
  { id: "name", label: "Name of the file being copied", on: true, description: "The name of the file the copy is at." },
  { id: "activeFiles", label: "Every file being copied right now", on: true, description: "One line for each file that is in flight, with its size. How many, and how often they refresh, are set below." },
  { id: "method", label: "How it is transferred (Robocopy, SMB, rsync, rclone, built-in…)", on: true, description: "The program or engine that moves the bytes for this transfer." },
  { id: "timeRemaining", label: "Time remaining", on: true, description: "With a note on how far to trust it." },
  { id: "copied", label: "Data copied so far", on: true, description: "Bytes done of the total." },
  { id: "itemsRemaining", label: "Items remaining", on: true, description: "Files left, and their size." },
  { id: "speed", label: "Speed", on: true, description: "Megabytes and files per second right now." },
  { id: "algorithm", label: "Algorithm", on: true, description: "The plan the engine chose for this kind of job." },
  { id: "streams", label: "Streams and buffer", on: true, description: "How many files are copied at once and the buffer size." },
  { id: "from", label: "From (source drive)", on: true, description: "The drive the files come from and what kind it is." },
  { id: "to", label: "To (destination drive)", on: true, description: "The drive the files go to and what kind it is." },
  { id: "typicalTime", label: "Typical time", on: true, description: "How long similar earlier transfers took." },
  { id: "averageSpeed", label: "Average speed", on: true, description: "Over the whole transfer so far." },
  { id: "peakSpeed", label: "Peak speed", on: true, description: "The fastest moment so far." },
  { id: "timeSplit", label: "Time spent per file and on moving data", on: true, description: "How much of the time goes on opening and closing files and how much on the data itself." },
  { id: "elapsed", label: "Time elapsed", on: true, description: "How long the transfer has been running." },
  { id: "nowAt", label: "Full path of the current file", on: true, description: "Where the file being copied lives." },
  // Not shown unless switched on.
  { id: "stage", label: "Stage (counting, copying, paused)", on: false, description: "What the transfer is doing right now." },
  { id: "filesDone", label: "Files done of the total", on: false, description: "A count of files rather than what is left." },
  { id: "totalSize", label: "Total size of the transfer", on: false, description: "All the data this transfer will move." },
  { id: "avgFileSize", label: "Average file size", on: false, description: "Total size over the number of files." },
  { id: "finishAt", label: "Finishes at (clock time)", on: false, description: "The time of day the estimate says it will be done." },
  { id: "startedAt", label: "Started at (clock time)", on: false, description: "The time of day the transfer began." },
  { id: "skippedErrors", label: "Skipped files and errors so far", on: false, description: "A running count of both." },
  { id: "confidence", label: "How sure the estimate is (percent)", on: false, description: "The number behind \"rough estimate\" and \"getting more accurate\"." },
  { id: "destFolder", label: "Destination folder (full path)", on: false, description: "Where the files are being put." },
  { id: "progressByFiles", label: "Progress by files and by data", on: false, description: "Two percentages: the share of files done and the share of bytes done." },
];

const BY_ID = new Map(TRANSFER_METRICS.map((m) => [m.id, m]));

/** Whether a line of the copy window is switched on. */
export function metricOn(settings: { advanced?: Record<string, unknown> } | null | undefined, id: string): boolean {
  const v = settings?.advanced?.[`transfer.show.${id}`];
  return typeof v === "boolean" ? v : (BY_ID.get(id)?.on ?? true);
}
