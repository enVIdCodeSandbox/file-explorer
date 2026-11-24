// Dragging files and folders around the window, the way Windows Explorer does it.
//
// Pointer events rather than the browser's drag and drop, so it behaves the
// same on every system and can show what will happen before it does:
//
//  * onto a folder (in the list, sidebar, a tab or the address bar): move it
//    there, or copy it if that folder is on another drive. Ctrl forces a copy,
//    Shift forces a move.
//  * onto the folder it is already in: nothing (with Ctrl: a duplicate).
//  * a folder onto itself or something inside it: not allowed.
//  * a folder onto the top or bottom edge of a Pinned row, or onto the Pinned
//    heading: pin it there. Dragging a pin reorders the pins.
//  * a section heading onto another: reorder the sections.
//  * a folder onto the Frequently used section: add it there (it counts as a few opens to begin with).
//  * Home, This PC, Recycle Bin, Disk usage or Smart Clear onto any section, or next to another of them: it moves
//    there and stays there.

import { crumb, flushTrail } from "./util/crashTrail";
import { create } from "zustand";
import { api, type Drive } from "./api";
import { useStore } from "./store";
import { baseName, insideArchive, isArchiveLoc, isInside, isRemote, parentPath } from "./util/path";
import { dragLimitMb, startDragFetch, stopDragFetch, type DragFetch } from "./util/dragFetch";
import { isBetween, isNavId, isZone, moveNav, parseLayout, serializeLayout, SETTING as LAYOUT_KEY, type NavId, type Zone } from "./util/sideLayout";

export interface Mods {
  ctrl: boolean;
  shift: boolean;
}

export interface DragPayload {
  /** What is being dragged, as full paths (empty for a section). */
  paths: string[];
  /** Whether each path is a folder. */
  dirs: boolean[];
  /** Set when a pinned row is being dragged: its position. */
  pinIndex?: number;
  /** Set when a sidebar section heading is being dragged. */
  section?: string;
  /** Set when one of the fixed navigation entries (Home, This PC…) is being dragged. */
  navItem?: NavId;
  /** Short text for the floating label. */
  label: string;
}

export interface Verdict {
  ok: boolean;
  op?: "copy" | "move";
  /** What the floating label says, e.g. "Move to Documents". */
  text: string;
}

const same = (a: string, b: string) => {
  const win = /^[A-Za-z]:|^\\\\/.test(a);
  const t = (s: string) => (win ? s.toLowerCase() : s).replace(/[\\/]+$/, "");
  return t(a) === t(b);
};

/** The drive or share a path lives on. */
export function volumeOf(path: string, drives: Drive[]): string {
  // Inside an archive counts as a place of its own: moving within it rearranges it, anything
  // else copies in (as with a zip folder in Windows).
  const arc = insideArchive(path);
  if (arc) return `archive:${arc}`;
  let best = "";
  for (const d of drives) if (d.mount.length > best.length && isInside(path.toLowerCase(), d.mount.toLowerCase())) best = d.mount;
  if (best) return best.toLowerCase();
  const unc = /^\\\\[^\\]+\\[^\\]+/.exec(path);
  if (unc) return unc[0].toLowerCase();
  const letter = /^[A-Za-z]:/.exec(path);
  return letter ? letter[0].toLowerCase() : "/";
}

/** What dropping `paths` onto the folder `dest` would do. */
export function resolveFolderDrop(paths: string[], dest: string, mods: Mods, drives: Drive[]): Verdict {
  if (!paths.length) return { ok: false, text: "" };
  const name = baseName(dest) || dest;
  const fold = (x: string) => (/^[A-Za-z]:|^\\\\/.test(x) ? x.toLowerCase() : x).replace(/[\\/]+$/, "");
  if (paths.some((p) => isInside(fold(dest), fold(p)))) {
    return { ok: false, text: "Can't put a folder inside itself" };
  }
  if (paths.every((p) => { const up = parentPath(p); return up !== null && same(up, dest); })) {
    return mods.ctrl ? { ok: true, op: "copy", text: `Copy here (make a duplicate)` } : { ok: false, text: `Already in ${name}` };
  }
  const arc = insideArchive(dest);
  if (arc) {
    if (paths.some((p) => same(p, arc))) return { ok: false, text: "Can't put an archive inside itself" };
    const within = paths.every((p) => insideArchive(p) === arc);
    const op = within ? (mods.ctrl ? "copy" : "move") : "copy";
    const where = baseName(arc);
    return { ok: true, op, text: `${op === "move" ? "Move" : "Add"} to ${where}` };
  }
  const onSameDrive = paths.every((p) => volumeOf(p, drives) === volumeOf(dest, drives));
  const op = mods.ctrl ? "copy" : mods.shift ? "move" : onSameDrive ? "move" : "copy";
  return { ok: true, op, text: `${op === "move" ? "Move" : "Copy"} to ${name}` };
}

// ---------------------------------------------------------------------------
// What the floating label shows
// ---------------------------------------------------------------------------

export interface DragView {
  x: number;
  y: number;
  label: string;
  count: number;
  text: string;
  ok: boolean;
}

/** Paths this app has handed to the system's own drag; if they come back to our window they are ignored. */
let ownDrag: string[] = [];
export function noteOwnDrag(paths: string[]) {
  ownDrag = paths;
}
export function isOwnDrag(paths: string[]): boolean {
  return paths.length > 0 && paths.every((p) => ownDrag.includes(p));
}

export const useDragView = create<{ view: DragView | null }>(() => ({ view: null }));
const setView = (view: DragView | null) => useDragView.setState({ view });

// ---------------------------------------------------------------------------
// The drag itself
// ---------------------------------------------------------------------------

type Hit =
  | { kind: "folder"; path: string; el: HTMLElement }
  | { kind: "pin"; index: number; el: HTMLElement; where: "before" | "after" }
  | { kind: "section"; id: string; el: HTMLElement; where: "before" | "after" }
  | { kind: "tab"; id: string; path: string | null; el: HTMLElement }
  | { kind: "nav"; id: NavId; el: HTMLElement; where: "before" | "after" }
  | { kind: "zone"; zone: Zone; el: HTMLElement }
  | { kind: "frequent"; el: HTMLElement }
  | null;

const marks = ["data-drop-over", "data-insert"];
const clearMarks = () => marks.forEach((m) => document.querySelectorAll(`[${m}]`).forEach((n) => n.removeAttribute(m)));

function targetAt(x: number, y: number, p: DragPayload): Hit {
  if (p.navItem) {
    const nodes = document.elementsFromPoint(x, y);
    for (const node of nodes) {
      const el = (node as HTMLElement).closest?.("[data-nav-item]") as HTMLElement | null;
      if (!el) continue;
      const r = el.getBoundingClientRect();
      const id = el.dataset.navItem;
      if (!isNavId(id) || !r.height) continue;
      return { kind: "nav", id, el, where: (y - r.top) / r.height < 0.5 ? "before" : "after" };
    }
    for (const node of nodes) {
      const el = (node as HTMLElement).closest?.("[data-nav-zone]") as HTMLElement | null;
      const zone = el?.dataset.navZone;
      if (el && isZone(zone)) return { kind: "zone", zone, el };
    }
    return null;
  }
  for (const node of document.elementsFromPoint(x, y)) {
    const el = (node as HTMLElement).closest?.("[data-drop]") as HTMLElement | null;
    if (!el) continue;
    const kind = el.dataset.drop;
    const r = el.getBoundingClientRect();
    const rel = r.height ? (y - r.top) / r.height : 0.5;
    if (p.section) {
      if (kind === "section") return { kind: "section", id: el.dataset.section!, el, where: rel < 0.5 ? "before" : "after" };
      continue;
    }
    if (kind === "section" && el.dataset.section === "frequent" && p.pinIndex === undefined) return { kind: "frequent", el };
    if (kind === "pinitem") {
      const index = Number(el.dataset.pinIndex);
      const isFolder = el.dataset.dropPath !== undefined;
      if (p.pinIndex !== undefined || !isFolder) return { kind: "pin", index, el, where: rel < 0.5 ? "before" : "after" };
      // A folder dragged here: the edges pin it, the middle drops it into the folder.
      if (rel < 0.25) return { kind: "pin", index, el, where: "before" };
      if (rel > 0.75) return { kind: "pin", index, el, where: "after" };
      return { kind: "folder", path: el.dataset.dropPath!, el };
    }
    if (kind === "pins") return { kind: "pin", index: Number(el.dataset.pinCount ?? 0), el, where: "before" };
    if (kind === "folder" && el.dataset.dropPath) return { kind: "folder", path: el.dataset.dropPath, el };
    if (kind === "tab") return { kind: "tab", id: el.dataset.tabId!, path: el.dataset.dropPath ?? null, el };
  }
  return null;
}

/** Start dragging once the pointer moves a few pixels. `get` is read at that moment. */
export function beginDrag(down: { clientX: number; clientY: number; button: number; ctrlKey: boolean; shiftKey: boolean }, get: () => DragPayload | null) {
  if (down.button !== 0) return;
  const [sx, sy] = [down.clientX, down.clientY];
  let payload: DragPayload | null = null;
  let mods: Mods = { ctrl: false, shift: false };
  let last = { x: sx, y: sy };
  let hit: Hit = null;
  let verdict: Verdict = { ok: false, text: "" };
  let springTimer: number | undefined;
  let springId = "";
  let scrollRaf = 0;

  const evaluate = () => {
    if (!payload) return;
    clearMarks();
    hit = targetAt(last.x, last.y, payload);
    const drives = useStore.getState().drives;
    verdict = { ok: false, text: "" };
    if (hit?.kind === "folder") {
      verdict = resolveFolderDrop(payload.paths, hit.path, mods, drives);
      if (verdict.ok) hit.el.setAttribute("data-drop-over", "");
    } else if (hit?.kind === "tab") {
      if (hit.path) {
        verdict = resolveFolderDrop(payload.paths, hit.path, mods, drives);
        if (verdict.ok) hit.el.setAttribute("data-drop-over", "");
      }
      if (springId !== hit.id) {
        window.clearTimeout(springTimer);
        springId = hit.id;
        // Resting over a tab brings it forward, so a file can be carried to a folder in another tab.
        springTimer = window.setTimeout(() => useStore.getState().activate(hit && hit.kind === "tab" ? hit.id : ""), 700);
      }
    } else if (hit?.kind === "pin") {
      const folders = payload.pinIndex !== undefined || payload.dirs.every(Boolean);
      verdict = folders
        ? { ok: true, text: payload.pinIndex !== undefined ? "Move here" : payload.paths.length > 1 ? "Pin these folders here" : "Pin here" }
        : { ok: false, text: "Only folders can be pinned" };
      if (verdict.ok) hit.el.setAttribute("data-insert", hit.where);
    } else if (hit?.kind === "section") {
      verdict = { ok: true, text: "Move section here" };
      hit.el.setAttribute("data-insert", hit.where);
    } else if (hit?.kind === "nav") {
      verdict = { ok: true, text: "Move here" };
      hit.el.setAttribute("data-insert", hit.where);
    } else if (hit?.kind === "zone") {
      verdict = { ok: true, text: hit.zone === "top" ? "Move to the top" : isBetween(hit.zone) ? "Put between sections" : `Move into ${SECTION_NAMES[hit.zone] ?? "this section"}` };
      hit.el.setAttribute("data-drop-over", "");
    } else if (hit?.kind === "frequent") {
      const folders = payload.dirs.length > 0 && payload.dirs.every(Boolean);
      verdict = folders ? { ok: true, text: "Add to Frequently used" } : { ok: false, text: "Only folders can be added" };
      if (verdict.ok) hit.el.setAttribute("data-drop-over", "");
    }
    if (hit?.kind !== "tab") {
      window.clearTimeout(springTimer);
      springId = "";
    }
    setView({ x: last.x, y: last.y, label: payload.label, count: payload.paths.length || 1, text: verdict.text, ok: verdict.ok });
  };

  // Near the top or bottom edge of a scrolling area, keep scrolling.
  const autoscroll = () => {
    scrollRaf = 0;
    if (!payload) return;
    for (const node of document.elementsFromPoint(last.x, last.y)) {
      const el = node as HTMLElement;
      const oy = getComputedStyle(el).overflowY;
      if ((oy === "auto" || oy === "scroll") && el.scrollHeight > el.clientHeight + 1) {
        const r = el.getBoundingClientRect();
        const zone = 36;
        const dy = last.y < r.top + zone ? -(r.top + zone - last.y) : last.y > r.bottom - zone ? last.y - (r.bottom - zone) : 0;
        if (dy) {
          el.scrollTop += Math.sign(dy) * Math.min(24, 4 + Math.abs(dy) / 2);
          evaluate();
          scrollRaf = requestAnimationFrame(autoscroll);
        }
        return;
      }
    }
  };

  let staging: Promise<string[] | null> | null = null;

  // Items on a phone or server are not files here. At the edge of the window they are fetched, and the system drag
  // starts when they have arrived; the button stays held meanwhile. Coming back inside, or letting go, stops it.
  let fetchToken = 0;
  let fetching: { handle: DragFetch | null } | null = null;
  let handingOver = false;
  const stopFetching = () => {
    fetchToken++;
    const f = fetching;
    fetching = null;
    if (f?.handle) void stopDragFetch(f.handle.id);
  };
  const beginFetching = (paths: string[]) => {
    const token = ++fetchToken;
    const mine = { handle: null as DragFetch | null };
    fetching = mine;
    void startDragFetch(paths, dragLimitMb(useStore.getState().settings))
      .then((handle) => {
        if (token !== fetchToken) {
          void stopDragFetch(handle.id);
          return null;
        }
        mine.handle = handle;
        return handle.done;
      })
      .then((files) => {
        if (!files || token !== fetchToken) return;
        handingOver = true;
        fetching = null;
        finish(false);
        noteOwnDrag(files);
        crumb(`drag out of the window: ${files.length} fetched item(s)`);
        flushTrail();
        return api.dragOut(files).catch((err) => useStore.getState().showToast(String(err)));
      })
      .catch((err) => {
        if (token !== fetchToken) return;
        useStore.getState().showToast(String(err?.message ?? err));
        stopFetching();
        finish(false);
      });
  };

  const move = (e: PointerEvent) => {
    // The button is already up but the release never reached the page (it happened over another window, a menu, or
    // while the system took over): the drag must not stay stuck to the pointer. It ends where the button was let go.
    if (e.buttons === 0 && e.pointerType === "mouse") return finish(payload !== null);
    last = { x: e.clientX, y: e.clientY };
    mods = { ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey };
    if (!payload) {
      if (Math.hypot(e.clientX - sx, e.clientY - sy) < 5) return;
      payload = get();
      if (!payload) return finish(false);
      document.body.setAttribute("data-dragging", "");
      if (payload.navItem) document.body.setAttribute("data-dragging-nav", "");
      // Items inside an archive do not exist on disk. Unpack them now, while the pointer is
      // still crossing the window, so they are ready when it reaches the edge.
      if (payload.paths.length && payload.paths.every(isArchiveLoc)) {
        staging = api.archiveStage(payload.paths).catch((err) => {
          useStore.getState().showToast(String(err));
          return null;
        });
      }
    }
    // Out past the edge of the window: hand over to the system's own drag, so
    // another program receives the files as if they were dropped from a file manager.
    const outside = e.clientX < 0 || e.clientY < 0 || e.clientX > window.innerWidth - 1 || e.clientY > window.innerHeight - 1;
    if (payload.paths.length && !payload.section && payload.paths.every(isRemote)) {
      if (outside) {
        if (!fetching) beginFetching(payload.paths);
        setView({ x: last.x, y: last.y, label: payload.label, count: payload.paths.length, text: "Getting the files to hand over…", ok: true });
        return;
      }
      if (fetching) stopFetching();
    }
    if (outside && payload.paths.length && !payload.section) {
      const paths = payload.paths;
      const pending = staging;
      finish(false);
      void (async () => {
        const real = pending ? await pending : paths;
        if (!real) return;
        noteOwnDrag(real);
        crumb(`drag out of the window: ${real.length} item(s)`);
        flushTrail();
        await api.dragOut(real).catch((err) => useStore.getState().showToast(String(err)));
      })();
      return;
    }
    evaluate();
    if (!scrollRaf) scrollRaf = requestAnimationFrame(autoscroll);
  };
  const keys = (e: KeyboardEvent) => {
    if (e.key === "Escape") return finish(false);
    mods = { ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey };
    evaluate();
  };
  const up = () => finish(true);
  // Anything that takes the pointer away from the page ends the drag: the window losing focus, the page being hidden,
  // the browser starting a native drag of its own (a link, an image, selected text), or the capture being lost.
  const lost = () => finish(false);
  const nativeDrag = (e: Event) => e.preventDefault();

  function finish(commit: boolean) {
    if (!handingOver) stopFetching();
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", cancel);
    window.removeEventListener("blur", lost);
    window.removeEventListener("dragstart", nativeDrag, true);
    document.removeEventListener("visibilitychange", lost);
    window.removeEventListener("keydown", keys, true);
    window.removeEventListener("keyup", keys, true);
    window.clearTimeout(springTimer);
    cancelAnimationFrame(scrollRaf);
    const started = payload !== null;
    const done = { payload, hit, verdict };
    clearMarks();
    setView(null);
    document.body.removeAttribute("data-dragging");
    document.body.removeAttribute("data-dragging-nav");
    if (!started) return;
    // The click that follows letting go must not open what was dropped on.
    const swallow = (e: Event) => (e.stopPropagation(), e.preventDefault());
    window.addEventListener("click", swallow, { capture: true, once: true });
    setTimeout(() => window.removeEventListener("click", swallow, true), 50);
    if (commit && done.payload && done.verdict.ok) void drop(done.payload, done.hit, done.verdict, mods);
  }
  const cancel = () => finish(false);

  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
  window.addEventListener("pointercancel", cancel);
  window.addEventListener("blur", lost);
  window.addEventListener("dragstart", nativeDrag, true);
  document.addEventListener("visibilitychange", lost);
  window.addEventListener("keydown", keys, true);
  window.addEventListener("keyup", keys, true);
}

async function drop(p: DragPayload, hit: Hit, v: Verdict, _mods: Mods) {
  const s = useStore.getState();
  if (!hit) return;
  if (hit.kind === "folder" && v.op) return s.startTransfer(p.paths, hit.path, v.op);
  if (hit.kind === "tab" && hit.path && v.op) return s.startTransfer(p.paths, hit.path, v.op);
  if (hit.kind === "pin") {
    const at = hit.index + (hit.where === "after" ? 1 : 0);
    if (p.pinIndex !== undefined) return s.movePin(p.pinIndex, at);
    p.paths.forEach((path, i) => s.pinAt(path, at + i, !p.dirs[i]));
    return;
  }
  if (hit.kind === "frequent") {
    const count = Number(s.settings?.advanced?.["frequent.dragCount"] ?? 3);
    await api.frequentSeed(p.paths, count).catch((e) => s.showToast(String(e)));
    return s.refreshFrequent();
  }
  if ((hit.kind === "nav" || hit.kind === "zone") && p.navItem) {
    const zone: Zone = hit.kind === "zone" ? hit.zone : zoneOfEl(hit.el);
    s.updateSettings((st) => {
      const layout = parseLayout(st.advanced?.[LAYOUT_KEY]);
      const next = hit.kind === "nav" ? moveNav(layout, p.navItem!, zone, hit.id, hit.where) : moveNav(layout, p.navItem!, zone);
      st.advanced = { ...st.advanced, [LAYOUT_KEY]: serializeLayout(next) };
      // A folded section would hide what was just put in it.
      if (zone !== "top" && !isBetween(zone)) {
        const folded = new Set(String(st.advanced?.["sidebar.folded"] ?? "").split(",").filter(Boolean));
        if (folded.delete(zone)) st.advanced = { ...st.advanced, "sidebar.folded": [...folded].join(",") };
      }
    });
    return;
  }
  if (hit.kind === "section") {
    const order = sidebarOrder(s.settings);
    const from = order.indexOf(p.section!);
    const to = order.indexOf(hit.id) + (hit.where === "after" ? 1 : 0);
    if (from < 0 || to < 0) return;
    const next = order.filter((x) => x !== p.section);
    next.splice(to > from ? to - 1 : to, 0, p.section!);
    s.updateSettings((st) => void (st.advanced = { ...st.advanced, "sidebar.order": next.join(",") }));
  }
}

/** The zone of the section or header a navigation entry is in. */
function zoneOfEl(el: HTMLElement): Zone {
  const z = (el.closest("[data-nav-zone]") as HTMLElement | null)?.dataset.navZone;
  return isZone(z) ? z : "top";
}

const SECTION_NAMES: Record<string, string> = { frequent: "Frequently used", recents: "Recents", pinned: "Pinned", drives: "Drives" };

export const SECTIONS = ["frequent", "recents", "pinned", "drives"] as const;
export function sidebarOrder(settings: { advanced?: Record<string, unknown> } | null): string[] {
  const raw = String(settings?.advanced?.["sidebar.order"] ?? SECTIONS.join(","));
  const given = raw.split(",").filter((x) => (SECTIONS as readonly string[]).includes(x));
  return [...new Set([...given, ...SECTIONS])];
}

