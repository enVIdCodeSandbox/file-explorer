import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import pkg from "./package.json";

const version = pkg.version;

// Tauri expects a fixed dev port and ignores its own Rust sources.
export default defineConfig({
  plugins: [react()],
  // Which build wrote the saved first screen: another build's copy is not trusted.
  define: { __APP_VERSION__: JSON.stringify(version) },
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**", "**/crates/**", "**/target/**"] } },
  // The app loads its files from disk, not over a network, so the usual 500 kB advice does not apply; the parts few people open are split out already.
  build: { target: "es2022", outDir: "dist", chunkSizeWarningLimit: 800 },
  test: { include: ["src/**/*.test.ts"] },
});
