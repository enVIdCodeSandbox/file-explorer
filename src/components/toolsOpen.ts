// Opening the file tools from menus. Separate from the window itself so menus do not need to load it.

import { activeLocation, useStore } from "../store";

export type ToolTab = "compareFiles" | "compareDirs" | "duplicates" | "split" | "encode" | "convert" | "grep";

/** Called from menus: open the tools on a tab, for the folder being viewed and the items selected in it. */
export function openTools(tab: ToolTab, paths?: string[]) {
  const s = useStore.getState();
  const folder = activeLocation() ?? "";
  useStore.setState({ tools: { tab, folder, paths: paths ?? s.selection, serial: Date.now() } });
}

/** The "File tools" menu: the same entries in the command bar menu and the right-click menu. */
export function fileToolsItems(selection: string[]): import("./ContextMenu").MenuItem[] {
  const two = selection.length === 2;
  const item = (id: ToolTab, label: string, shortcut?: string) => ({ id: `tools-${id}`, label, shortcut, run: () => openTools(id, selection) });
  return [
    item("compareFiles", two ? "Compare the two files…" : "Compare files…"),
    item("compareDirs", "Compare / synchronize folders…"),
    item("duplicates", "Find duplicate files…"),
    item("split", "Split or combine files…"),
    item("encode", "Encode or decode (UUE, XXE, MIME)…"),
    item("convert", "Convert pictures…"),
    item("grep", "Search in files…", "Alt+F7"),
  ];
}
