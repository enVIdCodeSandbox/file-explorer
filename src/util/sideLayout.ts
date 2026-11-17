// Where the five fixed entries at the top of the navigation pane (Home, This PC, Recycle Bin, Disk usage, Smart Clear)
// are shown. Each can be dragged into any section and stays there until it is dragged out. Stored as JSON in the
// advanced setting "sidebar.layout": the entries of each place, in order.

export const NAV_ITEMS = ["home", "thispc", "trash", "analyze", "clear"] as const;
export type NavId = (typeof NAV_ITEMS)[number];

/**
 * "top" is the pane's own header; "frequent" to "drives" are inside the sections; "below:x" is the strip between
 * section x and the next one, so an entry can also sit on its own between sections.
 */
export const ZONES = [
  "top", "frequent", "recents", "pinned", "drives",
  "below:frequent", "below:recents", "below:pinned", "below:drives",
] as const;
export type Zone = (typeof ZONES)[number];

export type Layout = Record<Zone, NavId[]>;

export const isNavId = (x: unknown): x is NavId => typeof x === "string" && (NAV_ITEMS as readonly string[]).includes(x);
export const isZone = (x: unknown): x is Zone => typeof x === "string" && (ZONES as readonly string[]).includes(x);

export const SETTING = "sidebar.layout";

export function defaultLayout(): Layout {
  const out = Object.fromEntries(ZONES.map((z) => [z, [] as NavId[]])) as Layout;
  out.top = [...NAV_ITEMS];
  return out;
}

/** Reads the saved layout; whatever is missing, repeated or unknown is put right, so every entry is in exactly one place. */
export function parseLayout(raw: unknown): Layout {
  let data: unknown = raw;
  if (typeof raw === "string") {
    try {
      data = JSON.parse(raw);
    } catch {
      data = null;
    }
  }
  const out = defaultLayout();
  for (const z of ZONES) out[z] = [];
  const seen = new Set<NavId>();
  if (data && typeof data === "object") {
    for (const z of ZONES) {
      const list = (data as Record<string, unknown>)[z];
      if (!Array.isArray(list)) continue;
      for (const id of list) {
        if (isNavId(id) && !seen.has(id)) {
          seen.add(id);
          out[z].push(id);
        }
      }
    }
  }
  // Anything not placed yet goes back to the top, in its usual order.
  const lost = NAV_ITEMS.filter((id) => !seen.has(id));
  if (lost.length) {
    const top = out.top;
    for (const id of lost) top.splice(Math.min(top.length, nth(id, top)), 0, id);
  }
  return out;
}

/** Where a missing entry goes among those already at the top: after the ones that normally come before it. */
function nth(id: NavId, top: NavId[]): number {
  const rank = NAV_ITEMS.indexOf(id);
  let at = 0;
  top.forEach((t, i) => {
    if (NAV_ITEMS.indexOf(t) < rank) at = i + 1;
  });
  return at;
}

export function serializeLayout(l: Layout): string {
  const compact: Partial<Record<Zone, NavId[]>> = {};
  for (const z of ZONES) if (l[z].length) compact[z] = l[z];
  return JSON.stringify(compact);
}

/** Move an entry into `zone`: next to `near` (before or after it) when that is in the zone, else at the end. */
export function moveNav(layout: Layout, id: NavId, zone: Zone, near?: NavId | null, where: "before" | "after" = "before"): Layout {
  const next = Object.fromEntries(ZONES.map((z) => [z, layout[z].filter((x) => x !== id)])) as Layout;
  const at = near && near !== id ? next[zone].indexOf(near) : -1;
  if (at < 0) next[zone].push(id);
  else next[zone].splice(at + (where === "after" ? 1 : 0), 0, id);
  return next;
}

/** True when nothing has been moved. */
export function isDefault(l: Layout): boolean {
  return serializeLayout(l) === serializeLayout(defaultLayout());
}

/**
 * What to draw: the entries of each zone, without the hidden ones. The entries of a zone that is not shown (its section
 * is turned off) are shown at the top instead, so they cannot be lost; they go back when the section comes back.
 */
export function placed(layout: Layout, shown: ReadonlySet<Zone>, hidden: ReadonlySet<NavId>): Layout {
  const out = Object.fromEntries(ZONES.map((z) => [z, [] as NavId[]])) as Layout;
  for (const z of ZONES) {
    for (const id of layout[z]) {
      if (hidden.has(id)) continue;
      // The header and the strips between sections are always there; a turned-off section is not.
      out[shown.has(z) || z === "top" || isBetween(z) ? z : "top"].push(id);
    }
  }
  return out;
}

/** Whether a zone is the strip between two sections (not inside one). */
export const isBetween = (z: Zone) => z.startsWith("below:");

/** The zone an entry is in. */
export function zoneOf(layout: Layout, id: NavId): Zone {
  return ZONES.find((z) => layout[z].includes(id)) ?? "top";
}
