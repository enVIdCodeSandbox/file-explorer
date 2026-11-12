// Thumbnail with viewport-driven loading.
//
// The file list is virtualized, so a <Thumb> only exists for rows on or near
// the screen. On top of that, the request is delayed briefly: when the user
// scrolls fast, rows unmount before the timer fires and no work is queued.
// Results are kept in a small LRU of blob URLs.

import { useEffect, useState } from "react";
import { api } from "../api";
import { useStore } from "../store";
import { adv } from "../settings/advanced";
import { extensionOf, kindOf } from "../util/format";
import { FileIcon } from "./Icons";

const MAX_CACHED = 600;
const MAX_FAILED = 2000;
const cache = new Map<string, string>();
const failed = new Set<string>();

function remember(key: string, url: string) {
  cache.set(key, url);
  if (cache.size > MAX_CACHED) {
    const oldest = cache.keys().next().value as string;
    URL.revokeObjectURL(cache.get(oldest)!);
    cache.delete(oldest);
  }
}

function giveUp(key: string) {
  failed.add(key);
  if (failed.size > MAX_FAILED) failed.delete(failed.values().next().value as string);
}

/**
 * Thumbnail scheduler. Requests wait here instead of going straight to the
 * backend, so work for tiles that scrolled away is dropped before it starts,
 * only a few run at once, and the newest (nearest the scroll position) go
 * first. Starts are spaced across timer ticks so a screenful of tiles never
 * lands on the UI thread in one burst.
 */
interface Job {
  key: string;
  run: () => Promise<void>;
}
const waiting: Job[] = [];
let running = 0;
let pumping = false;
let limit = 2;

export function setThumbParallelism(n: number) {
  limit = Math.max(1, Math.min(4, n));
}

function pump() {
  if (pumping) return;
  pumping = true;
  const step = () => {
    const job = running < limit ? waiting.pop() : undefined;
    if (!job) {
      pumping = false;
      return;
    }
    running++;
    void job.run().finally(() => {
      running--;
      if (waiting.length) pump();
    });
    // Leave the main thread a gap between starts.
    setTimeout(step, 16);
  };
  step();
}

function enqueue(job: Job): () => void {
  waiting.push(job);
  pump();
  return () => {
    const i = waiting.indexOf(job);
    if (i >= 0) waiting.splice(i, 1);
  };
}

/** How long a row must stay mounted before its thumbnail is requested. */
const SETTLE_MS = 90;

export function Thumb({
  path,
  name,
  isDir,
  modified,
  size,
  iconSize,
}: {
  path: string;
  name: string;
  isDir: boolean;
  modified: number | null;
  size: number;
  /** Size of the fallback type icon when there is no thumbnail. */
  iconSize?: number;
}) {
  const enabled = useStore((s) => s.budget?.thumbnails ?? false);
  const settle = useStore((s) => adv<number>(s.settings, "perf.thumbDelayMs"));
  const types = useStore((s) => s.info?.thumbnailTypes);
  const threads = useStore((s) => s.budget?.thumbThreads ?? 1);
  setThumbParallelism(threads + 1);
  const supported = enabled && !isDir && !!types?.includes(extensionOf(name));
  const key = `${path}|${modified}|${size}`;
  const [url, setUrl] = useState<string | undefined>(() => cache.get(key));

  useEffect(() => {
    if (!supported || failed.has(key)) return;
    const hit = cache.get(key);
    if (hit) {
      // Refresh LRU position.
      cache.delete(key);
      cache.set(key, hit);
      setUrl(hit);
      return;
    }
    setUrl(undefined);
    let alive = true;
    let dequeue: (() => void) | undefined;
    const timer = setTimeout(() => {
      dequeue = enqueue({
        key,
        run: async () => {
          if (cache.has(key)) return;
          try {
            const bytes = await api.thumbnail(path, size);
            const made = URL.createObjectURL(new Blob([bytes], { type: "image/png" }));
            remember(key, made);
            if (alive) setUrl(made);
          } catch {
            giveUp(key);
          }
        },
      });
    }, settle ?? SETTLE_MS);
    return () => {
      alive = false;
      clearTimeout(timer);
      dequeue?.();
    };
  }, [key, path, size, supported, settle]);

  if (supported && url) {
    return <img className="thumb" src={url} width={size} height={size} alt="" draggable={false} decoding="async" loading="lazy" />;
  }
  return <FileIcon kind={kindOf(name, isDir)} name={name} size={iconSize ?? size} />;
}
