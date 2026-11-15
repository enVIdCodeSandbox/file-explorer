// The text a Details cell shows for one item and one column. Columns that come with the folder
// listing are read from the entry; the others from what the backend returned for that file
// (dates in milliseconds and sizes in bytes, as text, so they follow the date and size settings).

import type { FileEntry } from "../api";
import type { ColumnDef } from "./columns";
import { extensionOf, formatDate, formatSize, typeLabel } from "./format";

export function cellValue(def: ColumnDef, e: FileEntry, extra: Record<string, string> | undefined): string {
  switch (def.id) {
    case "modified":
      return formatDate(e.modified);
    case "created":
      return formatDate(e.created);
    case "type":
      return typeLabel(e.name, e.isDir);
    case "extension":
      return e.isDir ? "" : extensionOf(e.name);
    case "size":
      return e.isDir ? "" : formatSize(e.size);
  }
  const raw = extra?.[def.id];
  if (raw === undefined || raw === "") return "";
  if (def.kind === "date" && /^\d+$/.test(raw)) return formatDate(Number(raw));
  if (def.kind === "size" && /^\d+$/.test(raw)) return formatSize(Number(raw));
  return raw;
}
