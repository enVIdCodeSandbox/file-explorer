// Work that nobody is waiting for runs once the first screen is up and the browser has a free moment.

type IdleWindow = Window & { requestIdleCallback?: (fn: () => void, opts?: { timeout: number }) => number; cancelIdleCallback?: (id: number) => void };

/** Run `fn` when the page is idle (at the latest after `within` ms). Returns a function that cancels it. */
export function whenIdle(fn: () => void, within = 1500): () => void {
  const w = window as IdleWindow;
  if (w.requestIdleCallback) {
    const id = w.requestIdleCallback(fn, { timeout: within });
    return () => w.cancelIdleCallback?.(id);
  }
  const id = setTimeout(fn, Math.min(within, 200));
  return () => clearTimeout(id);
}
