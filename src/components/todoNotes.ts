// The per-folder notes and to-do items, kept on this computer. Separate from the dialog so the folder view can ask
// "how many are open" without loading the dialog.

export interface TodoItem {
  id: string;
  text: string;
  done: boolean;
}
export interface FolderNotes {
  items: TodoItem[];
  note: string;
}

const KEY = "fast.todo";

export function loadNotes(): Record<string, FolderNotes> {
  try {
    return JSON.parse(localStorage.getItem(KEY) ?? "{}") as Record<string, FolderNotes>;
  } catch {
    return {};
  }
}
export function saveNotes(all: Record<string, FolderNotes>) {
  // Folders with nothing left are forgotten.
  const kept = Object.fromEntries(Object.entries(all).filter(([, v]) => v.items.length || v.note.trim()));
  try {
    localStorage.setItem(KEY, JSON.stringify(kept));
  } catch {
    /* not saved */
  }
}

/** How many things are still to do in a folder. */
export const openCount = (path: string) => loadNotes()[path]?.items.filter((i) => !i.done).length ?? 0;

