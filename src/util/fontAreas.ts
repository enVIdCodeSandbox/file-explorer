// Fonts for the major parts of the app. Each part can use a font of its own, chosen from the fonts installed on the
// computer or imported in Settings; empty means "the same as the app". The choices are kept in the advanced settings.

export interface FontArea {
  id: string;
  label: string;
  /** Where the choice is kept (settings.advanced). */
  key: string;
  /** The CSS variable the part reads, or null when the part reads the setting itself (the terminal). */
  cssVar: string | null;
  /** What follows the chosen font, so text still shows when it is missing. */
  fallback: string;
  /** Only fixed-width fonts make sense here. */
  fixedWidth?: boolean;
  /** What the part shows, for the preview. */
  sample: string;
}

const SANS = '"Segoe UI Variable Text", "Segoe UI", system-ui, -apple-system, "Helvetica Neue", "Noto Sans", sans-serif';
export const MONO = '"Cascadia Mono", "Cascadia Code", Consolas, "Lucida Console", Menlo, "SF Mono", "DejaVu Sans Mono", "Liberation Mono", "Courier New", monospace';

export const FONT_AREAS: FontArea[] = [
  { id: "files", label: "File list and columns", key: "font.files", cssVar: "--font-files", fallback: SANS, sample: "Quarterly report 2026.xlsx   12.4 MB   Modified yesterday" },
  { id: "sidebar", label: "Navigation pane (left)", key: "font.sidebar", cssVar: "--font-sidebar", fallback: SANS, sample: "Frequently used · Pinned · Drives" },
  { id: "toolbar", label: "Tabs, address bar and toolbar", key: "font.toolbar", cssVar: "--font-toolbar", fallback: SANS, sample: "Home › Documents › Taxes   Copy  Paste  Share" },
  { id: "mono", label: "Paths, viewer and code", key: "font.mono", cssVar: "--font-mono", fallback: MONO, fixedWidth: true, sample: "C:\\Users\\me\\Documents\\notes.txt" },
  { id: "terminal", label: "Terminal", key: "terminal.fontFamily", cssVar: null, fallback: MONO, fixedWidth: true, sample: "PS C:\\Users\\me> git add ." },
];

/** The name an imported font file is registered under (see loadFont in useAppearance). */
export const importedFamily = (file: string) => `Imported ${file.replace(/\.[^.]+$/, "")}`;

/** A font name as it goes in a CSS list: quoted, with quotes and backslashes made safe. */
export function cssFamily(name: string): string {
  return `"${name.replace(/[\\"]/g, "")}"`;
}

/** The CSS font list for a choice: the chosen font, then the fallbacks. Empty choice: nothing (follow the app). */
export function fontStack(choice: string, fallback: string, importedFamily?: string | null): string {
  const c = choice.trim();
  if (!c) return "";
  if (c.startsWith("imported:")) return importedFamily ? `${cssFamily(importedFamily)}, ${fallback}` : "";
  // Several names may have been typed, separated by commas: keep each as a name.
  const names = c.split(",").map((n) => n.trim().replace(/^["']|["']$/g, "")).filter(Boolean);
  return `${names.map(cssFamily).join(", ")}, ${fallback}`;
}
