// The window is made of panels that can be docked, floated, popped out,
// collapsed or hidden. This file holds the model; Layout.tsx draws it.

export type PanelId = "tabs" | "address" | "commands" | "sidebar" | "details" | "terminal";
export type Dock = "top" | "bottom" | "left" | "right" | "float" | "popout" | "hidden";

export interface PanelState {
  dock: Dock;
  /** Position among panels in the same dock (lower comes first). */
  order: number;
  collapsed: boolean;
  /** Where a floating panel sits, in pixels. */
  x: number;
  y: number;
  w: number;
  h: number;
  /** Width (left and right docks) or height (top and bottom docks) in pixels after the person
   *  drags the panel's edge; unset means the panel's own default. */
  size?: number;
}

export type Layout = Record<PanelId, PanelState>;

export interface PanelInfo {
  id: PanelId;
  label: string;
  icon: string;
  docks: Dock[];
}

/** Panels whose edge can be dragged to change their size when docked. */
export const RESIZABLE: PanelId[] = ["details", "terminal"];

export const PANELS: PanelInfo[] = [
  { id: "tabs", label: "Tabs", icon: "plus", docks: ["top", "bottom", "left", "right", "float", "popout", "hidden"] },
  { id: "address", label: "Address and search", icon: "search", docks: ["top", "bottom", "float", "popout", "hidden"] },
  { id: "commands", label: "Command bar", icon: "more", docks: ["top", "bottom", "left", "right", "float", "popout", "hidden"] },
  { id: "sidebar", label: "Navigation pane", icon: "sidebar", docks: ["left", "right", "top", "bottom", "float", "popout", "hidden"] },
  { id: "details", label: "Details pane", icon: "details", docks: ["right", "left", "bottom", "top", "float", "popout", "hidden"] },
  { id: "terminal", label: "Terminal", icon: "terminal", docks: ["bottom", "top", "left", "right", "float", "popout", "hidden"] },
];

export const DOCK_LABEL: Record<Dock, string> = {
  top: "Top",
  bottom: "Bottom",
  left: "Left side",
  right: "Right side",
  float: "Floating",
  popout: "Pop-out button",
  hidden: "Hidden",
};

const base = (dock: Dock, order: number, extra: Partial<PanelState> = {}): PanelState => ({
  dock,
  order,
  collapsed: false,
  x: 120,
  y: 120,
  w: 420,
  h: 320,
  ...extra,
});

export const DEFAULT_LAYOUT: Layout = {
  tabs: base("top", 0),
  address: base("top", 1),
  commands: base("top", 2),
  sidebar: base("left", 0),
  details: base("right", 0),
  terminal: base("bottom", 0, { w: 520, h: 300 }),
};

export interface Preset {
  id: string;
  name: string;
  description: string;
  layout: Layout;
}

export const PRESETS: Preset[] = [
  { id: "modern", name: "Modern", description: "Tabs, address and commands on top; navigation left; details right.", layout: DEFAULT_LAYOUT },
  {
    id: "classic",
    name: "Classic",
    description: "Commands first, then the address row; tabs sit above the file list.",
    layout: { ...DEFAULT_LAYOUT, tabs: base("top", 0), commands: base("top", 1), address: base("top", 2) },
  },
  {
    id: "vertical-tabs",
    name: "Vertical tabs",
    description: "Tabs run down the left edge, next to the navigation pane.",
    layout: { ...DEFAULT_LAYOUT, tabs: base("left", 0), sidebar: base("left", 1) },
  },
  {
    id: "bottom-bar",
    name: "Bottom commands",
    description: "The command bar moves to the bottom, within thumb reach on a touch screen.",
    layout: { ...DEFAULT_LAYOUT, commands: base("bottom", 0) },
  },
  {
    id: "focus",
    name: "Focus",
    description: "Only the tabs and address bar; everything else pops out from a button.",
    layout: { ...DEFAULT_LAYOUT, commands: base("popout", 0), sidebar: base("popout", 1), details: base("hidden", 0) },
  },
  {
    id: "dual-side",
    name: "Both sides",
    description: "Navigation on the left, the details pane and commands on the right.",
    layout: { ...DEFAULT_LAYOUT, commands: base("right", 0), details: base("right", 1) },
  },
];

/** Merge a saved layout with the defaults, dropping anything invalid. */
/**
 * The details pane has a dock in the layout and an on/off setting (the Details button). Keep
 * them from disagreeing: docking it somewhere in the layout turns it on, hiding it turns it
 * off, and turning it on again brings it back from "hidden". `before` and `after` are the
 * settings around one change; the result is `after` with the two made to agree.
 */
export function syncDetailsPane<T extends { appearance: { layout?: unknown; showDetailsPane: boolean } }>(before: T, after: T): T {
  const was = resolveLayout(before.appearance.layout as Parameters<typeof resolveLayout>[0]).details.dock;
  const layout = resolveLayout(after.appearance.layout as Parameters<typeof resolveLayout>[0]);
  const now = layout.details.dock;
  if (now !== was) {
    after.appearance.showDetailsPane = now !== "hidden";
  } else if (after.appearance.showDetailsPane && !before.appearance.showDetailsPane && now === "hidden") {
    after.appearance.layout = { ...layout, details: { ...layout.details, dock: "right" } };
  }
  return after;
}

export function resolveLayout(saved: unknown): Layout {
  const out = structuredClone(DEFAULT_LAYOUT);
  if (!saved || typeof saved !== "object") return out;
  for (const info of PANELS) {
    const s = (saved as Record<string, Partial<PanelState>>)[info.id];
    if (!s || typeof s !== "object") continue;
    const cur = out[info.id];
    if (typeof s.dock === "string" && info.docks.includes(s.dock as Dock)) cur.dock = s.dock as Dock;
    for (const k of ["order", "x", "y", "w", "h"] as const) if (typeof s[k] === "number" && Number.isFinite(s[k])) cur[k] = s[k] as number;
    if (typeof s.collapsed === "boolean") cur.collapsed = s.collapsed;
    if (typeof s.size === "number" && Number.isFinite(s.size) && s.size >= 80) cur.size = Math.min(4000, Math.round(s.size));
  }
  return out;
}

/** Panels in a dock, in order. */
export function panelsAt(layout: Layout, dock: Dock): PanelId[] {
  return PANELS.filter((p) => layout[p.id].dock === dock)
    .map((p) => p.id)
    .sort((a, b) => layout[a].order - layout[b].order);
}

/** Move a panel to a dock (at the end of it). Returns the new layout. */
export function moveTo(layout: Layout, id: PanelId, dock: Dock, at?: { x: number; y: number }): Layout {
  const info = PANELS.find((p) => p.id === id)!;
  if (!info.docks.includes(dock)) return layout;
  const next = structuredClone(layout);
  const order = Math.max(-1, ...panelsAt(next, dock).map((p) => next[p].order)) + 1;
  // A size dragged for one edge means nothing on another (a height is not a width).
  const flipsAxis = (dock === "left" || dock === "right") !== (next[id].dock === "left" || next[id].dock === "right");
  next[id] = { ...next[id], dock, order, collapsed: dock === "float" ? false : next[id].collapsed };
  if (flipsAxis) delete next[id].size;
  if (at) {
    next[id].x = Math.max(0, Math.round(at.x));
    next[id].y = Math.max(0, Math.round(at.y));
  }
  return next;
}

/** Swap a panel with its neighbor in the same dock. */
export function nudge(layout: Layout, id: PanelId, delta: -1 | 1): Layout {
  const list = panelsAt(layout, layout[id].dock);
  const i = list.indexOf(id);
  const j = i + delta;
  if (j < 0 || j >= list.length) return layout;
  const next = structuredClone(layout);
  list.splice(i, 1);
  list.splice(j, 0, id);
  list.forEach((p, n) => (next[p].order = n));
  return next;
}

/** Which edge a point is nearest to, or "float" in the middle. */
export function zoneAt(x: number, y: number, w: number, h: number): Dock {
  const [fx, fy] = [x / w, y / h];
  if (fy < 0.18) return "top";
  if (fy > 0.84) return "bottom";
  if (fx < 0.16) return "left";
  if (fx > 0.84) return "right";
  return "float";
}
