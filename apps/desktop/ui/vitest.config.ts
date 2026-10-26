import { fileURLToPath, URL } from "node:url";
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

/// Separate from `vite.config.ts` on purpose: that config carries Tauri's
/// own build settings (fixed dev port, platform-specific build targets,
/// `TAURI_*` env prefixes) which have nothing to do with running tests, and
/// mixing them makes both harder to read.
/// Node 25 turned on its own Web Storage by default, and its global
/// `localStorage` - inert without `--localstorage-file` - shadows jsdom's, so
/// every test touching storage failed with "clear is not a function". The
/// flag exists from Node 22; older versions would refuse it.
const nodeMajor = Number(process.versions.node.split(".")[0]);
const execArgv = nodeMajor >= 22 ? ["--no-experimental-webstorage"] : [];

export default defineConfig({
  // Mirrors `vite.config.ts` - without it any test that renders the layout
  // hits an undefined global.
  define: { __APP_VERSION__: JSON.stringify("0.0.0-test") },
  plugins: [react()],
  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
  },
  test: {
    environment: "jsdom",
    execArgv,
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
