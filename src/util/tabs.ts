/** Move every tab of a group together to `index`, counted among the tabs outside the group. */
export function moveGroupBlock<T extends { groupId: string | null }>(tabs: T[], groupId: string, index: number): T[] {
  const block = tabs.filter((t) => t.groupId === groupId);
  if (!block.length) return tabs;
  const rest = tabs.filter((t) => t.groupId !== groupId);
  const at = Math.max(0, Math.min(rest.length, index));
  return [...rest.slice(0, at), ...block, ...rest.slice(at)];
}
