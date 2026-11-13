// Built-in themes. Each id has a matching [data-theme="id"] block in
// src/styles/tokens.css; `swatch` is only used for the preview in Settings.

export interface ThemeInfo {
  id: string;
  name: string;
  dark: boolean;
  /** background, surface, text, accent: for the preview card. */
  swatch: [string, string, string, string];
}

export const THEMES: ThemeInfo[] = [
  { id: "dark", name: "Glass Dark", dark: true, swatch: ["#1c1c1c", "#272727", "#f2f2f2", "#4cc2ff"] },
  { id: "light", name: "Glass Light", dark: false, swatch: ["#eef1f5", "#ffffff", "#1b1b1b", "#0067c0"] },
  { id: "midnight", name: "Midnight", dark: true, swatch: ["#0b1220", "#111a2e", "#e6edf7", "#5aa9ff"] },
  { id: "graphite", name: "Graphite", dark: true, swatch: ["#121212", "#1b1b1b", "#e8e8e8", "#b8b8b8"] },
  { id: "nord", name: "Nord", dark: true, swatch: ["#2e3440", "#3b4252", "#eceff4", "#88c0d0"] },
  { id: "dracula", name: "Dracula", dark: true, swatch: ["#21222c", "#282a36", "#f8f8f2", "#bd93f9"] },
  { id: "solarized-dark", name: "Solarized Dark", dark: true, swatch: ["#002b36", "#073642", "#eee8d5", "#2aa198"] },
  { id: "forest", name: "Forest", dark: true, swatch: ["#10190f", "#182518", "#e3efe0", "#6fcf7a"] },
  { id: "solarized-light", name: "Solarized Light", dark: false, swatch: ["#eee8d5", "#fdf6e3", "#073642", "#268bd2"] },
  { id: "sepia", name: "Sepia", dark: false, swatch: ["#e9dfcc", "#f6efe0", "#3b2f1e", "#a8631a"] },
  { id: "rose", name: "Rose", dark: false, swatch: ["#f6e8ee", "#fff7fa", "#3a1d2a", "#d6336c"] },
  { id: "win11-dark", name: "Windows 11 Dark", dark: true, swatch: ["#202020", "#2b2b2b", "#ffffff", "#4cc2ff"] },
  { id: "win11-light", name: "Windows 11 Light", dark: false, swatch: ["#f3f3f3", "#ffffff", "#1a1a1a", "#005fb8"] },
  { id: "mac-dark", name: "macOS Dark", dark: true, swatch: ["#1e1e1e", "#2a2a2c", "#f5f5f7", "#0a84ff"] },
  { id: "mac-light", name: "macOS Light", dark: false, swatch: ["#ececec", "#ffffff", "#1d1d1f", "#007aff"] },
  { id: "adwaita-dark", name: "GNOME Dark", dark: true, swatch: ["#1e1e1e", "#2e2e32", "#ffffff", "#78aeed"] },
  { id: "adwaita-light", name: "GNOME Light", dark: false, swatch: ["#fafafb", "#ffffff", "#2e3436", "#3584e4"] },
  { id: "contrast-dark", name: "High contrast (dark)", dark: true, swatch: ["#000000", "#000000", "#ffffff", "#ffff00"] },
  { id: "contrast-light", name: "High contrast (light)", dark: false, swatch: ["#ffffff", "#ffffff", "#000000", "#0000cc"] },
];

/** The theme to apply for a saved id; "system" follows the OS colour scheme. */
export function resolveTheme(id: string, systemDark: boolean): string {
  if (id === "system") return systemDark ? "dark" : "light";
  return THEMES.some((t) => t.id === id) ? id : "dark";
}
