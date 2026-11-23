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
import { QUALITY, VIEW_SETTING, thumbKind, type ThumbView } from "../util/thumbKinds";
import { extensionOf, kindOf } from "../util/format";
import { SNIFFED_LOOKS } from "../util/fileTypes";
import { isAmbiguousName, useSniffedLook } from "../util/sniff";
import { isArchiveLoc, isRemote } from "../util/path";
import { isDevicePath, useThriftyBattery } from "../util/battery";
import { networkThumbs } from "../util/network";
import { FileIcon } from "./Icons";
import { askFill, cachedFill, looksFull, type Fill } from "../util/folderFill";

let maxCached = 600;
const MAX_FAILED = 2000;
/** A failed thumbnail is tried again after this long (a busy moment is not a verdict). */
const RETRY_MS = 20_000;
const cache = new Map<string, string>();
const failed = new Map<string, number>();
/** Pictures on screen right now: never thrown out of the cache, or the tile would go blank. */
const showing = new Map<string, number>();

/** How many pictures to keep: more on a powerful machine. */
export function setThumbCacheSize(n: number) {
  maxCached = Math.max(300, Math.min(6000, n));
}

function remember(key: string, url: string) {
  cache.set(key, url);
  if (cache.size > maxCached) {
    for (const k of cache.keys()) {
      if (k === key || showing.has(k)) continue;
      URL.revokeObjectURL(cache.get(k)!);
      cache.delete(k);
      if (cache.size <= maxCached) break;
    }
  }
}

/** Let go of every picture that is not on screen (the window is out of sight, so none is). */
export function dropThumbCache() {
  for (const [k, url] of cache) {
    if (showing.has(k)) continue;
    URL.revokeObjectURL(url);
    cache.delete(k);
  }
  failed.clear();
}

function giveUp(key: string) {
  failed.set(key, Date.now());
  if (failed.size > MAX_FAILED) failed.delete(failed.keys().next().value as string);
}

const hasFailed = (key: string) => {
  const at = failed.get(key);
  if (at === undefined) return false;
  if (Date.now() - at < RETRY_MS) return true;
  failed.delete(key);
  return false;
};

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
  limit = Math.max(1, Math.min(64, n));
}

/** Thumbnail priority 1-5 (Settings > Thumbnails): how soon work starts, and how long it leaves gaps. */
let gapMs = 16;
let settleScale = 1;
const PRIORITY = {
  1: { gap: 120, settle: 2.5, extra: -99 },
  2: { gap: 50, settle: 1.5, extra: -1 },
  3: { gap: 16, settle: 1, extra: 1 },
  4: { gap: 8, settle: 0.6, extra: 3 },
  5: { gap: 0, settle: 0, extra: 8 },
} as const;
/** How many thumbnails to make at once for a priority, given the number of worker threads. */
export function priorityParallelism(priority: number, threads: number): number {
  const p = PRIORITY[Math.max(1, Math.min(5, Math.round(priority))) as 1 | 2 | 3 | 4 | 5];
  const cap = Math.max(8, Math.min(64, threads));
  return Math.max(1, Math.min(cap, p.extra === -99 ? 1 : threads + p.extra));
}
export function setThumbPriority(priority: number) {
  const p = PRIORITY[Math.max(1, Math.min(5, Math.round(priority))) as 1 | 2 | 3 | 4 | 5];
  gapMs = p.gap;
  settleScale = p.settle;
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
    setTimeout(step, gapMs);
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

/** Is this file on a network drive or share? */
function onNetwork(path: string, drives: { mount: string; media: string }[]): boolean {
  if (path.startsWith("\\\\")) return true;
  const p = path.toLowerCase();
  return drives.some((d) => d.media === "network" && p.startsWith(d.mount.toLowerCase().replace(/[\\/]+$/, "")));
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
  view,
}: {
  path: string;
  name: string;
  isDir: boolean;
  modified: number | null;
  size: number;
  /** Size of the fallback type icon when there is no thumbnail. */
  iconSize?: number;
  /** Which view this is shown in; each view can be switched on or off in Settings > Thumbnails. */
  view: ThumbView;
}) {
  const enabled = useStore((s) => s.budget?.thumbnails ?? false);
  const settings = useStore((s) => s.settings);
  const drives = useStore((s) => s.drives);
  const settle = adv<number>(settings, "perf.thumbDelayMs");
  const types = useStore((s) => s.info?.thumbnailTypes);
  const threads = useStore((s) => s.budget?.thumbThreads ?? 1);
  const link = useStore((s) => s.link);
  const priority = Number(adv<string>(settings, "thumbs.priority")) || 3;
  setThumbPriority(priority);
  setThumbParallelism(adv<number>(settings, "thumbs.parallel") || priorityParallelism(priority, threads));
  const ext = extensionOf(name);
  const device = isDevicePath(path);
  // A name several kinds of file use (".ts": TypeScript, video, a Qt translation): the start of the file says which.
  // Only a video gets a thumbnail, and each kind has its own icon.
  const ambiguous = !isDir && isAmbiguousName(name);
  const look = useSniffedLook(path, modified, size, ambiguous && !isRemote(path) && !device && !isArchiveLoc(path));
  const kind = ambiguous ? (look?.endsWith(":video") ? "videos" : null) : thumbKind(ext);
  // On a powerful computer (the Full or Manual performance level) the medium, large and extra large views always show
  // thumbnails, whatever the per-view switches say; otherwise those switches decide.
  const strong = useStore((s) => s.budget?.tier === "full" || s.budget?.tier === "manual");
  const cacheMb = useStore((s) => s.budget?.cacheMb ?? 256);
  const intensity = useStore((s) => s.budget?.intensity ?? 50);
  const always = strong && adv<boolean>(settings, "thumbs.highPerfAlways") && (view === "medium" || view === "large" || view === "xl");
  setThumbCacheSize(intensity >= 95 ? 25000 : cacheMb >= 2048 ? 10000 : strong ? 3000 : 600);
  const wanted =
    !!kind && adv<boolean>(settings, `thumbs.${kind}`) && (always || adv<boolean>(settings, VIEW_SETTING[view])) && (device || networkThumbs(settings, link) || !onNetwork(path, drives));
  // A phone or camera on a cable follows the same thumbnail settings as any folder, except
  // when its own switch is off or the computer is running low on battery.
  const thrifty = useThriftyBattery(device && adv<boolean>(settings, "thumbs.deviceSaveBattery"));
  const deviceOk = !device || (adv<boolean>(settings, "thumbs.onDevices") && !thrifty);
  // A file on an FTP/SFTP/WebDAV server is fetched to make its picture, so it follows the network-drive setting.
  const serverOk = !isRemote(path) || device || networkThumbs(settings, link);
  const supported = enabled && wanted && deviceOk && serverOk && !isDir && !isArchiveLoc(path) && !!types?.includes(ext);
  // Ask for more or fewer pixels than are shown, depending on the sharpness setting.
  const px = Math.max(16, Math.min(512, Math.round(size * (QUALITY[adv<string>(settings, "thumbs.quality")] ?? 1))));
  const key = `${path}|${modified}|${px}|${adv<number>(settings, "thumbs.videoPercent")}`;
  const [url, setUrl] = useState<string | undefined>(() => cache.get(key));
  const [again, setAgain] = useState(0);
  // While this tile shows a picture it cannot be thrown out of the cache.
  useEffect(() => {
    if (!url) return;
    showing.set(key, (showing.get(key) ?? 0) + 1);
    return () => {
      const n = (showing.get(key) ?? 1) - 1;
      if (n <= 0) showing.delete(key);
      else showing.set(key, n);
    };
  }, [key, url]);

  useEffect(() => {
    if (!supported || hasFailed(key)) return;
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
    let retry: ReturnType<typeof setTimeout> | undefined;
    const timer = setTimeout(() => {
      dequeue = enqueue({
        key,
        run: async () => {
          if (cache.has(key)) return;
          try {
            const bytes = await api.thumbnail(path, px);
            const made = URL.createObjectURL(new Blob([bytes], { type: "image/png" }));
            remember(key, made);
            if (alive) setUrl(made);
          } catch (e) {
            // A drive that a copy is using is left alone for now: try again a little later instead of giving up.
            if (String(e).includes("drive-busy")) {
              if (alive) retry = setTimeout(() => setAgain((n) => n + 1), 5000);
            } else giveUp(key);
          }
        },
      });
    }, Math.round((settle ?? SETTLE_MS) * settleScale));
    return () => {
      alive = false;
      clearTimeout(timer);
      clearTimeout(retry);
      dequeue?.();
    };
  }, [key, path, px, supported, settle, again]);

  if (supported && url) {
    return <img className="thumb" src={url} width={size} height={size} alt="" draggable={false} decoding="async" loading="lazy" />;
  }
  return <FolderOrFile path={path} name={name} isDir={isDir} size={iconSize ?? size} look={look} />;
}

/** The type icon; a folder shows papers when it has files in it (Settings > Advanced > Files and lists). */
function FolderOrFile({ path, name, isDir, size, look }: { path: string; name: string; isDir: boolean; size: number; look?: string }) {
  const mode = useStore((s) => adv<string>(s.settings, "files.folderFill"));
  const watch = isDir && mode !== "off" && !isRemote(path) && !isArchiveLoc(path) && !isDevicePath(path);
  const [fill, setFill] = useState<Fill | undefined>(() => (watch ? cachedFill(path) : undefined));
  useEffect(() => {
    if (!watch) return setFill(undefined);
    const hit = cachedFill(path);
    if (hit) return setFill(hit);
    setFill(undefined);
    return askFill(path, setFill);
  }, [path, watch]);
  const sniffed = look ? SNIFFED_LOOKS[look] : undefined;
  return <FileIcon kind={sniffed?.category ?? kindOf(name, isDir)} name={name} look={look} size={size} full={watch && looksFull(fill, mode)} />;
}
