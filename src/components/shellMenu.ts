// Entries other programs add to the right-click menu, fetched in the
// background and turned into menu items. Results are remembered briefly per
// selection so a second right-click on the same thing is instant.

import { useRef } from "react";
import { api, type ShellItem } from "../api";
import type { MenuItem } from "./ContextMenu";

function toMenu(items: ShellItem[]): MenuItem[] {
  return items.map((it) => ({
    id: `shell-${it.id}-${it.label}`,
    label: it.label,
    iconUrl: it.iconUrl ?? undefined,
    disabled: it.disabled,
    checked: it.checked,
    children: it.children.length ? toMenu(it.children) : undefined,
    run: it.children.length
      ? undefined
      : () => {
          void api.shellInvoke(it.id).catch(() => {});
        },
  }));
}

export function useShellItems() {
  const cache = useRef(new Map<string, { at: number; items: ShellItem[] }>());

  /**
   * Fetch the entries for these paths. The system's menu object is rebuilt
   * every time (it must be the live one to run a choice), but the answer is
   * not awaited by the caller's first paint.
   */
  const fetchFor = async (paths: string[], background: boolean): Promise<MenuItem[]> => {
    try {
      const items = await api.shellMenu(paths, background);
      cache.current.set(paths.join("|"), { at: Date.now(), items });
      return toMenu(items);
    } catch {
      return [];
    }
  };

  return { fetchFor };
}
