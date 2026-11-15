// What to tell the person while a folder is still being read.

export interface LoadProgress {
  path: string;
  /** Items the system has handed over so far (counting ones left out, such as hidden files). */
  done: number;
  /** Items the folder has, when the system said (0 = not known). */
  total: number;
  startedAt: number;
}

/** Seconds until everything is in, from the pace so far; null when it can't be told yet. */
export function etaSeconds(p: Pick<LoadProgress, "done" | "total" | "startedAt">, now: number): number | null {
  const elapsed = (now - p.startedAt) / 1000;
  if (p.total <= 0 || p.done <= 0 || p.done >= p.total || elapsed < 0.4) return null;
  return ((p.total - p.done) * elapsed) / p.done;
}

/** One-line wording for the details pane and status bar. */
export function loadingNotice(p: LoadProgress, now: number, fmt: (secs: number) => string): { title: string; detail: string } {
  const eta = etaSeconds(p, now);
  const count = p.total > 0 ? `${p.done.toLocaleString()} of ${p.total.toLocaleString()} items` : `${p.done.toLocaleString()} items so far`;
  const when = eta == null ? "" : eta < 1 ? " Almost done." : ` About ${fmt(Math.ceil(eta))} left.`;
  return {
    title: "Still loading this folder",
    detail: `Not every file is shown yet (${count}). The rest appear as they are read.${when}`,
  };
}
