import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import pkg from "./package.json" with { type: "json" };
import { THEMES } from "./src/util/themes";

const version = pkg.version;

// Each theme's background goes into the page itself, so the first frame is the right color before any file is read.
const themeColors = {
  name: "theme-colors",
  transformIndexHtml: (html: string) => html.replace("/*THEME_BGS*/", THEMES.map((t) => `html[data-theme="${t.id}"] { --boot-bg: ${t.swatch[0]}; }`).join(" ")),
};

// Tauri expects a fixed dev port and ignores its own Rust sources.
export default defineConfig({
  plugins: [react(), themeColors],
  // Which build wrote the saved first screen: another build's copy is not trusted.
  define: { __APP_VERSION__: JSON.stringify(version) },
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**", "**/crates/**", "**/target/**"] } },
  // The app loads its files from disk, not over a network, so the usual 500 kB advice does not apply; the parts few people open are split out already.
  build: { target: "es2022", outDir: "dist", chunkSizeWarningLimit: 800 },
  test: { include: ["src/**/*.test.ts"] },
});
