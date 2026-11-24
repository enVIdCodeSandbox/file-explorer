// Files dragged into the window from another program (Explorer, Finder, a file
// manager, a browser's download bar). They are copied into the folder they are
// dropped on (a folder row, a breadcrumb, a tab, a sidebar entry), or else into
// the folder that is open.
//
// The in-app drag uses pointer events, so the window's native drop handler is
// free for this. Tauri reports positions in physical pixels.

import { getCurrentWebview } from "@tauri-apps/api/webview";
import { isOwnDrag, useDragView } from "./dnd";
import { useStore } from "./store";
import { baseName, insideArchive } from "./util/path";

function openFolder(): string | null {
  const s = useStore.getState();
  const loc = s.tabs.find((t) => t.id === s.activeTab)?.location ?? "";
  return loc && !loc.startsWith("fast://") ? loc : null;
}

interface Target {
  dest: string | null;
  el: HTMLElement | null;
}

function targetAt(x: number, y: number): Target {
  const hit = document.elementsFromPoint(x, y).find((el) => {
    const kind = (el as HTMLElement).dataset?.drop;
    return (kind === "folder" || kind === "tab") && !!(el as HTMLElement).dataset.dropPath;
  }) as HTMLElement | undefined;
  if (hit?.dataset.dropPath) {
    const path = hit.dataset.dropPath;
    if (!path.startsWith("fast://")) return { dest: path, el: hit };
  }
  return { dest: openFolder(), el: null };
}

const clearMarks = () => document.querySelectorAll("[data-drop-over]").forEach((el) => el.removeAttribute("data-drop-over"));

/** A file dropped on the page must never be opened by the page itself: the web view would navigate to it and leave the
 * app (a text file shown instead of the explorer, then nothing). The window's own drop handler takes real file drops
 * first; this is for whatever reaches the page anyway, on every system. */
function keepPageFromOpeningDrops(): () => void {
  const hasFiles = (e: DragEvent) => !!e.dataTransfer && [...(e.dataTransfer.types ?? [])].some((t) => t === "Files" || t === "text/uri-list" || t === "public.file-url");
  const stop = (e: DragEvent) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    if (e.type === "dragover" && e.dataTransfer) e.dataTransfer.dropEffect = "copy";
    if (e.type === "drop") e.stopPropagation();
  };
  for (const t of ["dragenter", "dragover", "drop"]) window.addEventListener(t, stop as EventListener);
  return () => {
    for (const t of ["dragenter", "dragover", "drop"]) window.removeEventListener(t, stop as EventListener);
  };
}

/** Start listening. Returns a function that stops. */
export async function listenForExternalDrops(): Promise<() => void> {
  let alive = true;
  const unguard = keepPageFromOpeningDrops();
  const off = await getCurrentWebview().onDragDropEvent((event) => {
    const p = event.payload;
    if (p.type === "leave") {
      clearMarks();
      useDragView.setState({ view: null });
      return;
    }
    if (p.type === "enter" || p.type === "over") {
      const paths = p.type === "enter" ? p.paths : undefined;
      if (paths && isOwnDrag(paths)) return;
      // macOS AppKit draggingLocation is already in logical points; Windows/Linux report physical pixels.
      const dpr = useStore.getState().info?.os === "macos" ? 1 : (window.devicePixelRatio || 1);
      const { dest, el } = targetAt(p.position.x / dpr, p.position.y / dpr);
      clearMarks();
      if (el) el.setAttribute("data-drop-over", "");
      const prev = useDragView.getState().view;
      const count = paths?.length ?? prev?.count ?? 1;
      useDragView.setState({
        view: {
          x: p.position.x / dpr,
          y: p.position.y / dpr,
          label: paths ? baseName(paths[0]) : (prev?.label ?? ""),
          count,
          ok: !!dest,
          text: dest ? (insideArchive(dest) ? `Add to ${baseName(insideArchive(dest) ?? "")}` : `Copy to ${baseName(dest) || dest}`) : "Open a folder to drop files here",
        },
      });
      return;
    }
    // drop
    clearMarks();
    useDragView.setState({ view: null });
    if (isOwnDrag(p.paths) || !p.paths.length) return;
    const dpr = useStore.getState().info?.os === "macos" ? 1 : (window.devicePixelRatio || 1);
    const { dest } = targetAt(p.position.x / dpr, p.position.y / dpr);
    const s = useStore.getState();
    if (!dest) {
      s.showToast("Open a folder first, then drop the files into it.");
      return;
    }
    void s.startTransfer(p.paths, dest, "copy");
  });
  if (!alive) {
    unguard();
    off();
    return () => {};
  }
  return () => {
    alive = false;
    unguard();
    off();
  };
}
