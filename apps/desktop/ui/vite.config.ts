import { createRequire } from "node:module";
import { fileURLToPath, URL } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Tauri expects a fixed port and will fail if it is already in use.
const host = process.env.TAURI_DEV_HOST;

// The fallback the Early Access badge shows when it is not running inside a
// Tauri webview (a plain `npm run dev`, or the browser-based UI checks).
// Inside Tauri the badge asks `getVersion()` instead, which reads
// `tauri.conf.json` - the number that actually ships.
const appVersion = createRequire(import.meta.url)("./package.json").version;

export default defineConfig({
  define: { __APP_VERSION__: JSON.stringify(appVersion) },
  plugins: [react(), tailwindcss()],
  optimizeDeps: {
    include: ["@iconify/react/offline"],
  },
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "safari13",
    // `oxc`, not `esbuild`: Vite 8 replaced its bundler with Rolldown and no
    // longer ships esbuild at all, so naming it here fails the build with a
    // module-not-found rather than falling back. This is also what removes
    // esbuild from the dependency tree, and with it the advisory that came
    // through it.
    minify: !process.env.TAURI_ENV_DEBUG ? "oxc" : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
});
