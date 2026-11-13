// Right-click menu: Windows 10's flat, complete list (no "Show more
// options"), drawn with Windows 11's rounded, translucent look.

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Icon } from "./Icons";

export interface MenuItem {
  /** Stable id; users can hide items by id in Settings > Right-click menu. */
  id: string;
  label: string;
  shortcut?: string;
  disabled?: boolean;
  danger?: boolean;
  separatorBefore?: boolean;
  /** Small picture shown at the left (a data: URL from the system). */
  iconUrl?: string;
  /** Name of a built-in icon from Icons.tsx. */
  icon?: string;
  /** Shows a tick, for options you can switch on and off. */
  checked?: boolean;
  /** A submenu; `run` is ignored when present. */
  children?: MenuItem[];
  run?: () => void;
}

export interface MenuState {
  x: number;
  y: number;
  items: MenuItem[];
}

/**
 * One column of the menu. Submenus are separate fixed-position panels placed
 * beside the row that opened them, so a long submenu can scroll without being
 * clipped by its parent.
 */
function Panel({
  items,
  x,
  y,
  depth,
  onClose,
  anchorLeft,
}: {
  items: MenuItem[];
  x: number;
  y: number;
  depth: number;
  onClose: () => void;
  /** Where to flip to if there is no room on the right (the parent's left edge). */
  anchorLeft?: number;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });
  const [open, setOpen] = useState<{ id: string; top: number; right: number; left: number } | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);

  // Keep the panel inside the window; flip submenus to the left when needed.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    let nx = x;
    if (depth > 0 && x + width > window.innerWidth - 4 && anchorLeft != null) nx = anchorLeft - width + 2;
    setPos({
      x: Math.max(4, Math.min(nx, window.innerWidth - width - 4)),
      y: Math.max(4, Math.min(y, window.innerHeight - height - 4)),
    });
    if (depth === 0) el.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
  }, [x, y, depth, anchorLeft, items.length]);

  useEffect(() => () => clearTimeout(timer.current), []);

  const onKeyDown = (e: React.KeyboardEvent) => {
    const buttons = [...(ref.current?.querySelectorAll<HTMLButtonElement>(":scope > div > button:not(:disabled)") ?? [])];
    const i = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === "ArrowDown") buttons[(i + 1) % buttons.length]?.focus();
    else if (e.key === "ArrowUp") buttons[(i - 1 + buttons.length) % buttons.length]?.focus();
    else if (e.key === "ArrowRight" && i >= 0) buttons[i].dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    else if (e.key === "ArrowLeft" && depth > 0) onClose();
    else return;
    e.preventDefault();
    e.stopPropagation();
  };

  const openChild = (item: MenuItem, row: HTMLElement, delay: number) => {
    clearTimeout(timer.current);
    const show = () => {
      const r = row.getBoundingClientRect();
      setOpen({ id: item.id, top: r.top - 4, right: r.right, left: r.left });
    };
    if (delay) timer.current = setTimeout(show, delay);
    else show();
  };

  const active = open && items.find((i) => i.id === open.id && i.children);
  return (
    <>
      <div ref={ref} className="context-menu" role="menu" data-depth={depth} style={{ left: pos.x, top: pos.y }} onKeyDown={onKeyDown}>
        {items.map((item) => (
          <div key={item.id} role="none">
            {item.separatorBefore && <div className="menu-separator" role="separator" />}
            <button
              role="menuitem"
              className="menu-item"
              data-menu-id={item.id}
              data-danger={item.danger || undefined}
              data-open={open?.id === item.id || undefined}
              aria-haspopup={item.children ? "menu" : undefined}
              disabled={item.disabled}
              onMouseEnter={(e) => {
                if (item.children && !item.disabled) openChild(item, e.currentTarget, 140);
                else {
                  clearTimeout(timer.current);
                  setOpen(null);
                }
              }}
              onMouseOver={(e) => item.children && e.target === e.currentTarget && openChild(item, e.currentTarget, 0)}
              onClick={(e) => {
                if (item.children) return openChild(item, e.currentTarget, 0);
                onClose();
                item.run?.();
              }}
            >
              <span className="menu-icon">
                {item.iconUrl ? (
                  <img src={item.iconUrl} width={16} height={16} alt="" draggable={false} />
                ) : item.icon ? (
                  <Icon name={item.icon} size={15} />
                ) : item.checked ? (
                  <Icon name="check" size={14} />
                ) : null}
              </span>
              <span className="menu-label">{item.label}</span>
              {item.shortcut && <kbd>{item.shortcut}</kbd>}
              {item.children && <Icon name="chevron" size={10} />}
            </button>
          </div>
        ))}
      </div>
      {active && active.children && (
        <Panel items={active.children} x={open.right - 2} y={open.top} depth={depth + 1} onClose={() => setOpen(null)} anchorLeft={open.left} />
      )}
    </>
  );
}

export function ContextMenu({ menu, onClose }: { menu: MenuState; onClose: () => void }) {
  useEffect(() => {
    const close = () => onClose();
    window.addEventListener("blur", close);
    window.addEventListener("resize", close);
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("blur", close);
      window.removeEventListener("resize", close);
      window.removeEventListener("keydown", onKey);
    };
  }, [onClose]);

  return (
    <div className="menu-backdrop" onMouseDown={onClose} onContextMenu={(e) => (e.preventDefault(), onClose())}>
      <div onMouseDown={(e) => e.stopPropagation()}>
        <Panel items={menu.items} x={menu.x} y={menu.y} depth={0} onClose={onClose} />
      </div>
    </div>
  );
}

/** Every right-click entry that can be shown or hidden in settings. */
export const MENU_CATALOG: { id: string; label: string }[] = [
  { id: "open", label: "Open" },
  { id: "openNewTab", label: "Open in new tab" },
  { id: "cut", label: "Cut" },
  { id: "copy", label: "Copy" },
  { id: "paste", label: "Paste" },
  { id: "copyPath", label: "Copy as path" },
  { id: "rename", label: "Rename" },
  { id: "delete", label: "Delete" },
  { id: "deletePermanent", label: "Delete permanently" },
  { id: "pin", label: "Pin / Unpin" },
  { id: "terminal", label: "Open terminal here" },
  { id: "terminalAdmin", label: "Open terminal as administrator" },
  { id: "analyze", label: "Analyze disk usage" },
  { id: "newFolder", label: "New folder" },
  { id: "newFile", label: "New file" },
  { id: "refresh", label: "Refresh" },
  { id: "openWith", label: "Open with" },
  { id: "sendTo", label: "Send to" },
  { id: "share", label: "Share" },
  { id: "createShortcut", label: "Create shortcut" },
  { id: "archive", label: "Compress and Extract" },
  { id: "shellExtras", label: "Entries from other programs" },
  { id: "properties", label: "Properties" },
  { id: "lock", label: "Lock with passcode" },
];
