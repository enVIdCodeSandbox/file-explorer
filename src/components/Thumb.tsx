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
import { isArchiveLoc, isRemote } from "../util/path";
import { isDevicePath, useThriftyBattery } from "../util/battery";
import { networkThumbs } from "../util/network";
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
  limit = Math.max(1, Math.min(8, n));
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
  setThumbParallelism(adv<number>(settings, "thumbs.parallel") || threads + 1);
  const ext = extensionOf(name);
  const kind = thumbKind(ext);
  const device = isDevicePath(path);
  const wanted =
    !!kind && adv<boolean>(settings, `thumbs.${kind}`) && adv<boolean>(settings, VIEW_SETTING[view]) && (device || networkThumbs(settings, link) || !onNetwork(path, drives));
  // A phone or camera on a cable follows the same thumbnail settings as any folder, except
  // when its own switch is off or the computer is running low on battery.
  const thrifty = useThriftyBattery(device && adv<boolean>(settings, "thumbs.deviceSaveBattery"));
  const deviceOk = !device || (adv<boolean>(settings, "thumbs.onDevices") && !thrifty);
  const supported = enabled && wanted && deviceOk && !isDir && (device || !isRemote(path)) && !isArchiveLoc(path) && !!types?.includes(ext);
  // Ask for more or fewer pixels than are shown, depending on the sharpness setting.
  const px = Math.max(16, Math.min(512, Math.round(size * (QUALITY[adv<string>(settings, "thumbs.quality")] ?? 1))));
  const key = `${path}|${modified}|${px}|${adv<number>(settings, "thumbs.videoPercent")}`;
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
            const bytes = await api.thumbnail(path, px);
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
  }, [key, path, px, supported, settle]);

  if (supported && url) {
    return <img className="thumb" src={url} width={size} height={size} alt="" draggable={false} decoding="async" loading="lazy" />;
  }
  return <FileIcon kind={kindOf(name, isDir)} name={name} size={iconSize ?? size} />;
}
