// Contrast checks for the built-in themes: reads the color variables of one theme block and reports the pairs that
// would be hard to read. Used by a test so a theme can never ship with unreadable text or see-through menus.

export type Rgba = [number, number, number, number];

/** #rgb, #rrggbb or rgb()/rgba(); anything else (color-mix, var()) is not a plain color and gives null. */
export function parseColor(v: string): Rgba | null {
  const s = v.trim().toLowerCase();
  let m = /^#([0-9a-f]{3})$/.exec(s);
  if (m) return [...m[1]].map((c) => parseInt(c + c, 16)).concat(1) as Rgba;
  m = /^#([0-9a-f]{6})$/.exec(s);
  if (m) return [parseInt(m[1].slice(0, 2), 16), parseInt(m[1].slice(2, 4), 16), parseInt(m[1].slice(4, 6), 16), 1];
  m = /^rgba?\(([^)]+)\)$/.exec(s);
  if (m) {
    const p = m[1].split(",").map((x) => parseFloat(x));
    if (p.length >= 3 && p.every((n) => !Number.isNaN(n))) return [p[0], p[1], p[2], p[3] ?? 1];
  }
  return null;
}

/** Lay a translucent color over an opaque one. */
export function over(top: Rgba, under: Rgba): Rgba {
  const a = top[3];
  return [top[0] * a + under[0] * (1 - a), top[1] * a + under[1] * (1 - a), top[2] * a + under[2] * (1 - a), 1];
}

const lum = ([r, g, b]: Rgba) => {
  const f = (c: number) => {
    const x = c / 255;
    return x <= 0.03928 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
};

/** WCAG contrast ratio, 1 to 21. */
export function contrast(a: Rgba, b: Rgba): number {
  const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** The variables of one theme: the base block, then the theme's own, with later declarations winning. */
export function themeVars(css: string, id: string): Record<string, string> {
  const out: Record<string, string> = {};
  const blocks = [...css.replace(/\/\*[\s\S]*?\*\//g, "").matchAll(/([^{}]+)\{([^{}]*)\}/g)];
  for (const [, sel, body] of blocks) {
    // A block may serve several selectors: "…[data-theme="a"], …[data-theme="b"]".
    const alternatives = sel.split(",").map((x) => x.trim());
    if (!alternatives.some((a) => a === ":root" || a === `:root[data-theme="${id}"]`)) continue;
    for (const d of body.matchAll(/(--[a-z0-9-]+)\s*:\s*([^;]+);/g)) out[d[1]] = d[2].trim();
  }
  return out;
}

export interface Problem {
  what: string;
  ratio?: number;
  need?: number;
}

/** Everything wrong with a theme's colors, or an empty list. */
export function auditTheme(vars: Record<string, string>): Problem[] {
  const problems: Problem[] = [];
  const get = (k: string) => (vars[k] ? parseColor(vars[k]) : null);
  const bg = get("--bg");
  const surface = get("--surface");
  const raised = get("--surface-raised");
  const text = get("--text");
  const muted = get("--text-muted");
  const accent = get("--accent");
  const accentText = get("--accent-text");
  if (!bg || !surface || !raised || !text || !muted || !accent || !accentText) return [{ what: "a core color is missing or not a plain color" }];
  // Menus, dropdowns and dialogs draw on these: if they let the page through, text behind shows over them.
  if (surface[3] < 1) problems.push({ what: "--surface is see-through" });
  if (raised[3] < 1) problems.push({ what: "--surface-raised is see-through" });
  const pairs: [string, Rgba, Rgba, number][] = [
    ["text on the window", text, bg, 7],
    ["text on the content area", text, surface, 7],
    ["text on menus and dialogs", text, raised, 7],
    ["muted text on the content area", over(muted, surface), surface, 4.5],
    ["muted text on menus and dialogs", over(muted, raised), raised, 4.5],
    ["text on the selected accent color", accentText, accent, 4.5],
    ["the accent against the content area", accent, surface, 3],
  ];
  for (const k of ["--surface-hover", "--surface-active"]) {
    const c = get(k);
    if (!c) continue;
    pairs.push([`text on ${k}`, text, over(c, surface), 4.5]);
    pairs.push([`muted text on ${k}`, over(muted, over(c, surface)), over(c, surface), 3.5]);
    pairs.push([`text on ${k} in menus`, text, over(c, raised), 4.5]);
  }
  const strong = get("--border-strong");
  if (strong) pairs.push(["the strong border against the content area", over(strong, surface), surface, 1.25]);
  for (const [what, a, b, need] of pairs) {
    const ratio = contrast(a, b);
    if (ratio < need) problems.push({ what, ratio: Math.round(ratio * 100) / 100, need });
  }
  return problems;
}
