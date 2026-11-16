// Turns the appearance settings into what the page shows: the theme, accent,
// fonts, style choices, imported style sheets and the Advanced display options.

import { useEffect } from "react";
import { api } from "./api";
import { adv } from "./settings/advanced";
import { useStore } from "./store";
import { configureSort } from "./util/filter";
import { configureFormat } from "./util/format";
import { resolveTheme } from "./util/themes";

/** Built-in font choices: a name for Settings and the CSS font stack. */
export const FONTS: { id: string; name: string; stack: string }[] = [
  { id: "system", name: "System default", stack: '"Segoe UI Variable Text", "Segoe UI", system-ui, -apple-system, "Helvetica Neue", "Noto Sans", sans-serif' },
  { id: "ui", name: "Native UI font", stack: "system-ui, -apple-system, sans-serif" },
  { id: "humanist", name: "Humanist", stack: '"Gill Sans", "Gill Sans MT", Calibri, "Trebuchet MS", "Noto Sans", sans-serif' },
  { id: "rounded", name: "Rounded", stack: 'ui-rounded, "SF Pro Rounded", "Nunito", "Varela Round", "Segoe UI", system-ui, sans-serif' },
  { id: "serif", name: "Serif", stack: 'Georgia, "Iowan Old Style", "Palatino Linotype", "Noto Serif", serif' },
  { id: "mono", name: "Monospace", stack: '"Cascadia Mono", "SF Mono", ui-monospace, Menlo, Consolas, monospace' },
  { id: "compact", name: "Compact", stack: '"Arial Narrow", "Roboto Condensed", "Helvetica Neue", Arial, sans-serif' },
];

const sheet = (id: string) => {
  let el = document.getElementById(id) as HTMLStyleElement | null;
  if (!el) {
    el = document.createElement("style");
    el.id = id;
    document.head.appendChild(el);
  }
  return el;
};

const loadedFonts = new Map<string, string>();

/** Load an imported font file and return the family name to use. */
async function loadFont(file: string): Promise<string | null> {
  if (loadedFonts.has(file)) return loadedFonts.get(file)!;
  try {
    const family = `Imported ${file.replace(/\.[^.]+$/, "")}`;
    const face = new FontFace(family, await api.readAsset("font", file));
    await face.load();
    document.fonts.add(face);
    loadedFonts.set(file, family);
    return family;
  } catch {
    return null;
  }
}

export function useAppearance() {
  const settings = useStore((s) => s.settings);
  const appearance = settings?.appearance;

  // Theme, accent, density and the person's custom CSS.
  useEffect(() => {
    if (!appearance) return;
    const root = document.documentElement;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      root.dataset.theme = resolveTheme(appearance.theme, media.matches);
      // The next start paints this theme before any script has run.
      try {
        localStorage.setItem("fast.theme", root.dataset.theme);
      } catch {
        // storage may be unavailable
      }
    };
    apply();
    media.addEventListener("change", apply);
    root.dataset.density = appearance.density;
    // The accent from Settings replaces the theme's own only once changed
    // from the default, so each theme keeps its colors until then.
    const accent = /^#[0-9a-f]{3,8}$/i.test(appearance.accent) ? appearance.accent : "#4cc2ff";
    sheet("accent-css").textContent = accent.toLowerCase() === "#4cc2ff" ? "" : `:root { --accent: ${accent}; --accent-text: ${isLight(accent) ? "#0a0a0a" : "#ffffff"}; }`;
    sheet("user-css").textContent = appearance.customCss;
    return () => media.removeEventListener("change", apply);
  }, [appearance?.theme, appearance?.accent, appearance?.density, appearance?.customCss]); // eslint-disable-line react-hooks/exhaustive-deps

  // Fonts, menus, icons, background, corners.
  const style = appearance?.style;
  useEffect(() => {
    if (!style) return;
    const root = document.documentElement;
    let alive = true;
    const builtin = FONTS.find((f) => f.id === style.font);
    if (builtin) root.style.setProperty("--font", builtin.stack);
    else if (style.font.startsWith("imported:")) {
      void loadFont(style.font.slice("imported:".length)).then((family) => {
        if (alive && family) root.style.setProperty("--font", `"${family}", ${FONTS[0].stack}`);
      });
    }
    root.style.setProperty("--font-size", `${style.fontSize}px`);
    root.style.setProperty("--radius", `${style.radius}px`);
    root.style.setProperty("--radius-small", `${Math.max(2, Math.round(style.radius / 2))}px`);
    root.dataset.menu = style.menu;
    root.dataset.icons = style.icons;
    root.dataset.folder = style.folderColor;
    root.dataset.bg = style.background;
    return () => {
      alive = false;
    };
  }, [style?.font, style?.fontSize, style?.radius, style?.menu, style?.icons, style?.folderColor, style?.background]); // eslint-disable-line react-hooks/exhaustive-deps

  // Imported background picture.
  useEffect(() => {
    const root = document.documentElement;
    if (!style || style.background !== "image" || !style.backgroundImage) {
      root.style.removeProperty("--window-image");
      return;
    }
    let url: string | null = null;
    let alive = true;
    void api
      .readAsset("image", style.backgroundImage)
      .then((bytes) => {
        if (!alive) return;
        url = URL.createObjectURL(new Blob([bytes]));
        root.style.setProperty("--window-image", `url("${url}")`);
      })
      .catch(() => {});
    return () => {
      alive = false;
      if (url) URL.revokeObjectURL(url);
    };
  }, [style?.background, style?.backgroundImage]); // eslint-disable-line react-hooks/exhaustive-deps

  // Imported style sheets that are switched on.
  const enabled = style?.enabledSheets.join("|");
  useEffect(() => {
    if (!style) return;
    let alive = true;
    void Promise.all(
      style.enabledSheets.map((name) =>
        api
          .readAsset("css", name)
          .then((b) => `/* ${name} */\n${new TextDecoder().decode(b)}`)
          .catch(() => ""),
      ),
    ).then((parts) => alive && (sheet("imported-css").textContent = parts.join("\n\n")));
    return () => {
      alive = false;
    };
  }, [enabled]); // eslint-disable-line react-hooks/exhaustive-deps

  // Advanced display options.
  useEffect(() => {
    if (!settings) return;
    const root = document.documentElement;
    const flag = (name: string, on: boolean) => (on ? root.setAttribute(name, "") : root.removeAttribute(name));
    root.dataset.chrome = adv<string>(settings, "ui.chrome");
    root.dataset.motion = adv<boolean>(settings, "ui.animations") ? "on" : "off";
    flag("data-zebra", adv<boolean>(settings, "ui.zebra"));
    flag("data-gridlines", adv<boolean>(settings, "ui.gridLines"));
    root.dataset.focus = adv<boolean>(settings, "ui.focusRing") ? "on" : "off";
    root.dataset.scrollbars = adv<string>(settings, "ui.scrollbars");
    root.dataset.statusbar = adv<boolean>(settings, "ui.statusBar") ? "on" : "off";
    root.dataset.dimhidden = adv<boolean>(settings, "ui.dimHidden") ? "on" : "off";
    root.dataset.tabclose = adv<string>(settings, "tabs.closeButton");
    root.style.setProperty("--tab-width", `${adv<number>(settings, "tabs.width")}px`);
    root.style.setProperty("--tab-min-width", `${adv<number>(settings, "tabs.minWidth")}px`);
    root.style.setProperty("--tab-height", `${adv<number>(settings, "tabs.height")}px`);
    root.style.setProperty("--selection-alpha", `${adv<number>(settings, "ui.selectionOpacity")}%`);
    const follow = adv<boolean>(settings, "band.followAccent");
    const fill = follow ? "var(--accent)" : adv<string>(settings, "band.fillColor");
    root.style.setProperty("--band-fill", `color-mix(in srgb, ${fill} ${adv<number>(settings, "band.fillOpacity")}%, transparent)`);
    root.style.setProperty("--band-border", follow ? "var(--accent)" : adv<string>(settings, "band.borderColor"));
    root.style.setProperty("--band-border-width", `${adv<number>(settings, "band.borderWidth")}px`);
    root.style.setProperty("--band-border-style", adv<string>(settings, "band.borderStyle"));
    document.body.style.zoom = String(adv<number>(settings, "ui.scale") / 100);
    configureFormat({ units: adv<"decimal" | "binary" | "windows">(settings, "files.sizeUnits"), dates: adv<string>(settings, "files.dateFormat"), clock: adv<string>(settings, "files.clock") });
    configureSort({ natural: adv<boolean>(settings, "files.naturalSort"), caseSensitive: adv<boolean>(settings, "files.caseSensitive") });
  }, [settings]);
}

function isLight(hex: string): boolean {
  const h = hex.replace("#", "");
  const full = h.length === 3 ? h.split("").map((c) => c + c).join("") : h.slice(0, 6);
  const [r, g, b] = [0, 2, 4].map((i) => parseInt(full.slice(i, i + 2), 16));
  return (r * 299 + g * 587 + b * 114) / 1000 > 150;
}
