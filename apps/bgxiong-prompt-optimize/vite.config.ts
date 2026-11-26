import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const appDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(appDir, "../..");

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  resolve: {
    // Keep a single React instance when pulling ui/src chrome into this app.
    // Official builds must not resolve these from ui/node_modules (often a
    // junction into the main checkout).
    dedupe: ["react", "react-dom"],
    alias: {
      react: resolve(appDir, "node_modules/react"),
      "react-dom": resolve(appDir, "node_modules/react-dom"),
    },
  },
  server: {
    port: 15183,
    strictPort: true,
    fs: {
      allow: [repoRoot],
    },
  },
  envPrefix: ["VITE_", "TAURI_"],
});
