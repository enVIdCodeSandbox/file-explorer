// The Recycle Bin (Windows) / Trash (macOS, Linux): what was deleted, where it came from, and when,
// with Put back, Delete for good and Empty.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type Bin, type BinItem } from "../api";
import { ask, confirmDeletePassword, openFile, useStore } from "../store";
import { formatCount, formatDate, formatSize, kindOf } from "../util/format";
import { trashLabel } from "../util/path";
import { pickFolder } from "../util/pickFile";
import { ContextMenu, type MenuState } from "./ContextMenu";
import { FileIcon, Icon } from "./Icons";

type Order = "deleted" | "name" | "from" | "size";

export function TrashView() {
  const os = useStore((s) => s.info?.os);
  const label = trashLabel(os);
  const [bin, setBin] = useState<Bin | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [order, setOrder] = useState<{ by: Order; asc: boolean }>({ by: "deleted", asc: false });
  const [busy, setBusy] = useState(false);
  const anchor = useRef<string | null>(null);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const refreshedAt = useStore((s) => s.refreshedAt);

  const load = useCallback(() => {
    api
      .trashList()
      .then((b) => {
        setBin(b);
        setError(null);
        // Keep only what is still there selected.
        setPicked((p) => new Set([...p].filter((id) => b.items.some((i) => i.id === id))));
      })
      .catch((e) => setError(String(e)));
  }, []);
  useEffect(load, [load, refreshedAt]);
  // Another program may empty or fill the bin: look again when the window comes back.
  useEffect(() => {
    const again = () => !document.hidden && load();
    window.addEventListener("focus", again);
    return () => window.removeEventListener("focus", again);
  }, [load]);

  const items = useMemo(() => {
    const list = [...(bin?.items ?? [])];
    const dir = order.asc ? 1 : -1;
    const key = (i: BinItem): string | number =>
      order.by === "name" ? i.name.toLowerCase() : order.by === "from" ? (i.originalParent ?? "").toLowerCase() : order.by === "size" ? (i.size ?? -1) : (i.deleted ?? 0);
    list.sort((a, b) => (key(a) < key(b) ? -dir : key(a) > key(b) ? dir : 0));
    return list;
  }, [bin, order]);

  const chosen = items.filter((i) => picked.has(i.id));
  const canRestore = !!bin?.canRestore;

  const click = (e: React.MouseEvent, item: BinItem) => {
    if (e.shiftKey && anchor.current) {
      const a = items.findIndex((i) => i.id === anchor.current);
      const b = items.findIndex((i) => i.id === item.id);
      const [from, to] = a < b ? [a, b] : [b, a];
      setPicked(new Set(items.slice(from, to + 1).map((i) => i.id)));
      return;
    }
    anchor.current = item.id;
    if (e.ctrlKey || e.metaKey) {
      setPicked((p) => {
        const next = new Set(p);
        if (!next.delete(item.id)) next.add(item.id);
        return next;
      });
    } else setPicked(new Set([item.id]));
  };

  const run = async (work: () => Promise<unknown>, done: string) => {
    setBusy(true);
    try {
      await work();
      useStore.getState().showToast(done);
    } catch (e) {
      useStore.getState().showToast(String(e));
    } finally {
      setBusy(false);
      load();
    }
  };

  const restore = () => {
    if (!chosen.length) return;
    const parents = [...new Set(chosen.map((i) => i.originalParent).filter((p): p is string => !!p))];
    void run(async () => {
      const n = await api.trashRestore(chosen.map((i) => i.id), parents);
      useStore.getState().bumpDirs(parents);
      if (n < chosen.length) throw new Error("Some items are no longer in the bin.");
    }, chosen.length === 1 ? `Put back ${chosen[0].name}` : `Put back ${chosen.length} items`);
  };

  const purge = async () => {
    if (!chosen.length) return;
    const r = await ask({
      title: "Delete permanently",
      message: chosen.length === 1 ? `Delete ${chosen[0].name} for good? It cannot be brought back.` : `Delete these ${chosen.length} items for good? They cannot be brought back.`,
      buttons: [
        { id: "ok", label: "Delete", primary: true },
        { id: "cancel", label: "Cancel" },
      ],
    });
    if (r?.button !== "ok") return;
    if (!(await confirmDeletePassword(chosen.length === 1 ? chosen[0].name : `${chosen.length} items`))) return;
    void run(() => api.trashPurge(chosen.map((i) => i.id)), "Deleted for good");
  };

  const empty = async () => {
    if (!items.length) return;
    const r = await ask({
      title: `Empty the ${label}`,
      message: `Delete all ${formatCount(items.length)} items in the ${label} for good? They cannot be brought back.`,
      buttons: [
        { id: "ok", label: `Empty the ${label}`, primary: true },
        { id: "cancel", label: "Cancel" },
      ],
    });
    if (r?.button !== "ok") return;
    if (!(await confirmDeletePassword(`everything in the ${label}`))) return;
    void run(() => api.trashEmpty(), `The ${label} is empty`);
  };

  // Looking at an item does not take it out of the bin: a folder opens as a folder, a file in its program or the viewer.
  const open = (i: BinItem) => {
    if (!i.path) return void useStore.getState().showToast("This item cannot be opened from here. Put it back, or copy it out.");
    if (i.isDir) useStore.getState().navigate(i.path);
    else void openFile(i.path);
  };
  const preview = (i: BinItem) => {
    if (i.path && !i.isDir) useStore.getState().openViewer(i.path, { siblings: items.filter((x) => x.path && !x.isDir).map((x) => x.path!) });
  };
  /** A copy goes where you choose and the original stays in the bin. */
  const copyOut = async (list: BinItem[]) => {
    const paths = list.map((i) => i.path).filter((p): p is string => !!p);
    if (!paths.length) return void useStore.getState().showToast("These items cannot be copied from here. Put them back instead.");
    const to = await pickFolder("Copy to which folder?");
    if (!to) return;
    await useStore.getState().startTransfer(paths, to, "copy");
  };

  const head = (by: Order, text: string) => (
    <button className="trash-th" data-active={order.by === by || undefined} onClick={() => setOrder((o) => (o.by === by ? { by, asc: !o.asc } : { by, asc: by === "name" || by === "from" }))}>
      {text}
      {order.by === by && <span aria-hidden> {order.asc ? "▲" : "▼"}</span>}
    </button>
  );

  const total = items.reduce((n, i) => n + (i.size ?? 0), 0);

  return (
    <div className="page trash-view" tabIndex={-1} onKeyDown={(e) => {
      if (e.key === "Delete" && chosen.length) void purge();
      else if (e.key === "Enter" && chosen.length === 1) open(chosen[0]);
      else if (e.key === " " && chosen.length === 1 && !chosen[0].isDir) {
        e.preventDefault();
        preview(chosen[0]);
      }
      else if (e.key === "a" && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        setPicked(new Set(items.map((i) => i.id)));
      }
    }}>
      <div className="section-head">
        <h2>{label}</h2>
        <span className="muted">{bin ? `${formatCount(items.length)} ${items.length === 1 ? "item" : "items"}${total ? `, ${formatSize(total)}` : ""}` : "Reading…"}</span>
      </div>
      <div className="trash-actions">
        {canRestore && (
          <button className="primary" disabled={busy || !chosen.length} onClick={restore} title="Put the selected items back where they were deleted from">
            <Icon name="undo" size={13} /> Put back
          </button>
        )}
        <button disabled={busy || chosen.length !== 1 || !chosen[0].path} onClick={() => open(chosen[0])} title="Look inside the selected item without putting it back">
          Open
        </button>
        <button disabled={busy || !chosen.some((i) => i.path)} onClick={() => void copyOut(chosen)} title="Copy the selected items to a folder; they stay in the bin">
          Copy to…
        </button>
        <button disabled={busy || !chosen.length} onClick={() => void purge()} title="Delete the selected items for good">
          <Icon name="trash" size={13} /> Delete permanently
        </button>
        <button disabled={busy || !items.length} onClick={() => void empty()}>
          Empty the {label}
        </button>
        <button disabled={busy} onClick={load} title="Look again">
          <Icon name="refresh" size={13} /> Refresh
        </button>
      </div>
      {!canRestore && bin && items.length > 0 && <p className="muted trash-note">Items cannot be put back from here on this system. Open one to look inside it, copy it out with Copy to…, or use Put Back in Finder.</p>}
      {error && <div className="empty-state" data-error>{error}</div>}
      {bin && !error && items.length === 0 && <div className="empty-state">The {label} is empty.</div>}
      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
      {items.length > 0 && (
        <div className="trash-table" role="grid" aria-label={label} aria-multiselectable>
          <div className="trash-row trash-header" role="row">
            {head("name", "Name")}
            {head("from", "Original location")}
            {head("deleted", "Date deleted")}
            {head("size", "Size")}
          </div>
          {items.map((i) => (
            <div
              key={i.id}
              role="row"
              className="trash-row"
              aria-selected={picked.has(i.id)}
              data-selected={picked.has(i.id) || undefined}
              onClick={(e) => click(e, i)}
              onDoubleClick={() => open(i)}
              onContextMenu={(e) => {
                e.preventDefault();
                const now = picked.has(i.id) ? chosen : [i];
                if (!picked.has(i.id)) setPicked(new Set([i.id]));
                const one = now.length === 1 ? now[0] : null;
                setMenu({
                  x: e.clientX,
                  y: e.clientY,
                  items: [
                    ...(one ? [{ id: "open", label: one.isDir ? "Open folder" : "Open", disabled: !one.path, run: () => open(one) }] : []),
                    ...(one && !one.isDir ? [{ id: "preview", label: "Preview", disabled: !one.path, run: () => preview(one) }] : []),
                    { id: "copy", label: "Copy to…", disabled: !now.some((x) => x.path), run: () => void copyOut(now) },
                    ...(canRestore ? [{ id: "restore", label: "Put back", separatorBefore: true, run: () => restore() }] : []),
                    { id: "purge", label: "Delete permanently", separatorBefore: !canRestore, run: () => void purge() },
                  ],
                });
              }}
            >
              <span className="trash-name" role="gridcell" title={i.name}>
                <FileIcon kind={kindOf(i.name, i.isDir)} name={i.name} size={16} />
                <span>{i.name}</span>
              </span>
              <span className="trash-from muted" role="gridcell" title={i.originalParent ?? ""}>{i.originalParent ?? ""}</span>
              <span className="muted" role="gridcell">{i.deleted ? formatDate(i.deleted * 1000) : ""}</span>
              <span className="muted trash-size" role="gridcell">{i.size != null ? formatSize(i.size) : i.entries != null ? `${formatCount(i.entries)} ${i.entries === 1 ? "item" : "items"}` : ""}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
