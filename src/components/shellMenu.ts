// Entries other programs add to the right-click menu, fetched in the
// background and turned into menu items. Results are remembered per selection
// (for as long as the performance settings say) so a second right-click on the
// same thing is instant.

import { api, type ShellItem } from "../api";
import type { MenuItem } from "./ContextMenu";
import { useStore } from "../store";
import { shellCacheSeconds } from "../util/shellCache";

function toMenu(items: ShellItem[], paths: string[], background: boolean): MenuItem[] {
  return items.map((it) => ({
    id: `shell-${it.id}-${it.label}`,
    label: it.label,
    iconUrl: it.iconUrl ?? undefined,
    disabled: it.disabled,
    checked: it.checked,
    children: it.children.length ? toMenu(it.children, paths, background) : undefined,
    run: it.children.length
      ? undefined
      : () => {
          // The paths and the name go along: if this menu came from memory, the shell builds it again to run the entry.
          void api.shellInvoke(paths, background, it.id, it.label).catch(() => {});
        },
  }));
}

/** Whether two menus offer the same entries (so a refresh that changed nothing need not redraw an open menu). */
export function sameShell(a: MenuItem[] | null | undefined, b: MenuItem[]): boolean {
  if (!a || a.length !== b.length) return false;
  return a.every((x, i) => x.id === b[i].id && !!x.disabled === !!b[i].disabled && !!x.checked === !!b[i].checked && (x.iconUrl ?? "") === (b[i].iconUrl ?? "") && (!x.children?.length && !b[i].children?.length ? true : sameShell(x.children, b[i].children ?? [])));
}

/** What the last answers were, so a right-click on the same thing can show them in its very first paint. */
const seen = new Map<string, { at: number; items: ShellItem[] }>();
const KEEP = 64;
const keyOf = (paths: string[], background: boolean) => `${background ? "bg" : "it"}:${paths.join("\u001f")}`;

/** The entries remembered for this selection, if they are younger than the setting allows. */
export function peekShell(paths: string[], background: boolean): MenuItem[] | null {
  const maxAge = shellCacheSeconds(useStore.getState().settings) * 1000;
  if (!maxAge) return null;
  const hit = seen.get(keyOf(paths, background));
  return hit && Date.now() - hit.at < maxAge && hit.items.length ? toMenu(hit.items, paths, background) : null;
}

/** Ask for the entries now, so they are ready when the right-click comes (the answer is kept, not shown). */
export function prefetchShell(paths: string[], background: boolean) {
  if (!paths.length) return;
  void fetchShell(paths, background).catch(() => {});
}

async function fetchShell(paths: string[], background: boolean): Promise<ShellItem[]> {
  const secs = shellCacheSeconds(useStore.getState().settings);
  const items = await api.shellMenu(paths, background, secs);
  if (secs) {
    const k = keyOf(paths, background);
    seen.set(k, { at: Date.now(), items });
    while (seen.size > KEEP) seen.delete(seen.keys().next().value as string);
  } else seen.clear();
  return items;
}

export function useShellItems() {
  /**
   * Fetch the entries for these paths. The caller shows its own menu first and merges these in; with a level that
   * remembers, the shell answers at once for something it has built lately.
   */
  const fetchFor = async (paths: string[], background: boolean): Promise<MenuItem[]> => {
    try {
      return toMenu(await fetchShell(paths, background), paths, background);
    } catch {
      return [];
    }
  };

  return { fetchFor, peek: peekShell };
}
