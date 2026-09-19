import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import process from "node:process";

// Set by the Tauri CLI when it targets a physical device; unset for an ordinary desktop run.
const host = process.env.TAURI_DEV_HOST;
const debug = !!process.env.TAURI_ENV_DEBUG;

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [react()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    // The webview is Chromium (WebView2) on Windows and WebKit elsewhere; both are well past this.
    target: "chrome105",
    // Vite 8 minifies with oxc; naming esbuild here would pull in a dependency it no longer ships.
    minify: !debug,
    sourcemap: debug,
  },
}));
