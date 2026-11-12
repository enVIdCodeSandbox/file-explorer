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
import { baseName } from "./util/path";

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

/** Start listening. Returns a function that stops. */
export async function listenForExternalDrops(): Promise<() => void> {
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
      const dpr = window.devicePixelRatio || 1;
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
          text: dest ? `Copy to ${baseName(dest) || dest}` : "Open a folder to drop files here",
        },
      });
      return;
    }
    // drop
    clearMarks();
    useDragView.setState({ view: null });
    if (isOwnDrag(p.paths) || !p.paths.length) return;
    const dpr = window.devicePixelRatio || 1;
    const { dest } = targetAt(p.position.x / dpr, p.position.y / dpr);
    const s = useStore.getState();
    if (!dest) {
      s.showToast("Open a folder first, then drop the files into it.");
      return;
    }
    void s.startTransfer(p.paths, dest, "copy");
  });
  return off;
}
