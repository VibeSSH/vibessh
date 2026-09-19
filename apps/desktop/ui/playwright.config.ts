import { defineConfig } from "@playwright/test";

/**
 * End-to-end / visual-regression layer for the desktop UI. Unit tests stay in
 * vitest; this drives the built app in a real browser against the `?fixtures=1`
 * mock data, at the four window sizes the app supports (its Tauri config sets a
 * 960x600 minimum), and asserts the layout facts the manual audit checked:
 * no accidental horizontal scroll and no view that crashes to a blank screen.
 *
 * The dev server is reused when one is already running (the Tauri/Vite dev
 * server on 1420); in CI Playwright starts `bun run dev` itself.
 */
const PORT = 1420;

export default defineConfig({
  testDir: "./e2e",
  timeout: 30_000,
  expect: { timeout: 10_000 },
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: [["list"]],
  use: {
    baseURL: `http://localhost:${PORT}`,
    trace: "on-first-retry",
  },
  webServer: {
    command: "bun run dev",
    url: `http://localhost:${PORT}`,
    reuseExistingServer: true,
    timeout: 120_000,
  },
  projects: [
    { name: "1920x1080", use: { viewport: { width: 1920, height: 1080 } } },
    { name: "1366x768", use: { viewport: { width: 1366, height: 768 } } },
    { name: "1024x768", use: { viewport: { width: 1024, height: 768 } } },
    // The minimum window size the app supports (tauri.conf.json).
    { name: "960x600", use: { viewport: { width: 960, height: 600 } } },
  ],
});
