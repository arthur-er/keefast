import { fileURLToPath, URL } from "node:url";

import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

import { bundledPlugins } from "./bundled-plugins";

// The web UI of keefast's Tauri app. `pnpm dev` serves it to the Tauri window (devUrl in
// ../tauri.conf.json); `pnpm dev:mock` serves it to a normal browser over the mock backend, on
// $PORT if set (scripts/ui-mock.sh gives each worktree its own), else 1421.
const mockPort = Number(process.env.PORT) || 1421;

export default defineConfig(({ mode }) => ({
  plugins: [react(), tailwindcss(), bundledPlugins()],
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  // Tauri expects a fixed port, and shows Rust's errors rather than having them cleared.
  clearScreen: false,
  server: {
    port: mode === "mock" ? mockPort : 1420,
    strictPort: true,
    host: mode === "mock" ? true : "localhost",
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    // WebKitGTK and WebView2 are recent; no polyfills.
    target: "es2022",
    sourcemap: mode !== "production",
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
  },
}));
