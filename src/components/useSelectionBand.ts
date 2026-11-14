// The selection box: drag over empty space in a list to select what the box touches, like the
// blue box in Windows Explorer. Drawn from where the press began to the pointer, anchored to the
// list's content so it keeps growing while the list scrolls under it (the list scrolls by itself
// when the pointer is near or past its top or bottom edge). Ctrl toggles what the box touches and
// Shift adds it, both relative to what was chosen before the press; Escape puts that back.

import { useRef, useState } from "react";
import type React from "react";
import { adv } from "../settings/advanced";
import { useStore } from "../store";
import { touchedIndexes, type BandGeometry } from "../util/band";

interface Options {
  scroller: React.RefObject<HTMLDivElement | null>;
  geometry: BandGeometry;
  /** A key for each item in list order (names, or paths). */
  keys: string[];
  selected: Set<string>;
  commit: (next: Set<string>) => void;
  /** The last item the box touched, so the keyboard carries on from there. */
  onLast: (index: number) => void;
}

export function useSelectionBand(o: Options) {
  const [band, setBand] = useState<{ l: number; t: number; w: number; h: number } | null>(null);
  // The drag outlives renders, so it reads the latest layout and items from here.
  const live = useRef(o);
  live.current = o;

  const startBand = (ev: React.MouseEvent) => {
    const el = o.scroller.current;
    if (!el || ev.button !== 0) return;
    const settings = useStore.getState().settings;
    const on = !settings || adv<boolean>(settings, "files.selectionBox");
    const autoScroll = !settings || adv<boolean>(settings, "band.autoScroll");
    const zone = settings ? adv<number>(settings, "band.edgeZone") : 28;
    const fastest = settings ? adv<number>(settings, "band.scrollSpeed") : 48;
    if (!on) {
      // No box: a press on empty space only clears the selection (with Ctrl or Shift it keeps it).
      if (!ev.ctrlKey && !ev.shiftKey && !ev.metaKey) o.commit(new Set());
      return;
    }
    const rect = el.getBoundingClientRect();
    // Not on the scrollbars.
    if (ev.clientX >= rect.left + el.clientWidth || ev.clientY >= rect.top + el.clientHeight) return;
    ev.preventDefault();
    el.focus();
    const ctrl = ev.ctrlKey || ev.metaKey;
    const add = ev.shiftKey;
    const before = new Set(o.selected);
    if (!ctrl && !add) o.commit(new Set());
    const startX = ev.clientX - rect.left + el.scrollLeft;
    const startY = ev.clientY - rect.top + el.scrollTop;
    let pointer = { x: ev.clientX, y: ev.clientY };
    let moved = false;
    let raf = 0;
    let shown = new Set<string>(ctrl || add ? before : []);
    const clamp = (v: number, hi: number) => Math.max(0, Math.min(v, hi));

    const update = () => {
      const r0 = el.getBoundingClientRect();
      const x = clamp(pointer.x - r0.left + el.scrollLeft, Math.max(el.scrollWidth, el.clientWidth));
      const y = clamp(pointer.y - r0.top + el.scrollTop, Math.max(el.scrollHeight, el.clientHeight));
      const l = Math.min(startX, x), r = Math.max(startX, x), t = Math.min(startY, y), b = Math.max(startY, y);
      if (!moved && Math.hypot(pointer.x - ev.clientX, pointer.y - ev.clientY) < 4) return;
      moved = true;
      setBand({ l, t, w: r - l, h: b - t });
      const hits = touchedIndexes(live.current.geometry, l, t, r, b);
      const keys = live.current.keys;
      const touched = new Set(hits.map((i) => keys[i]));
      let next: Set<string>;
      if (ctrl) {
        next = new Set(before);
        for (const k of touched) next.has(k) ? next.delete(k) : next.add(k);
      } else if (add) next = new Set([...before, ...touched]);
      else next = touched;
      if (next.size !== shown.size || [...next].some((k) => !shown.has(k))) {
        shown = next;
        live.current.commit(next);
      }
      if (hits.length) live.current.onLast(hits[hits.length - 1]);
    };

    const tick = () => {
      // Near or past the top or bottom edge, the list scrolls, faster the further out.
      const r0 = el.getBoundingClientRect();
      const top = r0.top + live.current.geometry.headerHeight;
      const dy = pointer.y < top + zone ? -(top + zone - pointer.y) : pointer.y > r0.bottom - zone ? pointer.y - (r0.bottom - zone) : 0;
      const dx = pointer.x < r0.left + zone ? -(r0.left + zone - pointer.x) : pointer.x > r0.right - zone ? pointer.x - (r0.right - zone) : 0;
      if (autoScroll && moved && (dy || dx)) {
        el.scrollTop += Math.sign(dy) * Math.min(fastest, 4 + Math.abs(dy) / 2);
        el.scrollLeft += Math.sign(dx) * Math.min(fastest, 4 + Math.abs(dx) / 2);
      }
      update();
      raf = requestAnimationFrame(tick);
    };

    const move = (m: MouseEvent) => {
      pointer = { x: m.clientX, y: m.clientY };
      update();
    };
    const stop = (restore: boolean) => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      window.removeEventListener("keydown", key, true);
      cancelAnimationFrame(raf);
      setBand(null);
      if (restore) live.current.commit(before);
    };
    const up = () => stop(false);
    const key = (k: KeyboardEvent) => {
      if (k.key === "Escape") {
        k.preventDefault();
        k.stopPropagation();
        stop(true);
      }
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
    window.addEventListener("keydown", key, true);
    raf = requestAnimationFrame(tick);
  };

  return { band, startBand };
}
