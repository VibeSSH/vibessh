import type { Theme } from "./tokens";

/**
 * The palettes that ship with the app.
 *
 * Ported rather than invented. A palette that reads well across twenty
 * surfaces is months of somebody's careful work, and picking sixteen colours
 * by eye reliably produces something that looks fine in isolation and muddy
 * in a list. The hex values below were taken from each project's own
 * published palette, not from memory:
 *
 * - Catppuccin Mocha - catppuccin/palette (MIT)
 * - Nord - nordtheme.com's own palette reference (MIT)
 *
 * Only colour values are used, mapped onto this app's own token names; no
 * code or assets are taken from either project. `default` is VibeSSH's own
 * palette, the same values `globals.css` ships, restated here so the picker
 * has something to select rather than a special case meaning "no theme".
 */
export const BUILT_IN_THEMES: Theme[] = [
  {
    id: "default",
    name: "VibeSSH",
    colors: {
      "--surface-bg": "#090a0c",
      "--surface-0": "#101115",
      "--surface-1": "#141619",
      "--surface-2": "#191b20",
      "--surface-3": "#1d2026",
      "--border": "#282b31",
      "--border-hover": "#363a42",
      "--text-primary": "#ecedef",
      "--text-secondary": "#a0a1aa",
      "--text-tertiary": "#747780",
      "--accent": "#4dd9f5",
      "--accent-hover": "#6fe3fb",
      "--success": "#22c55e",
      "--danger": "#ef5350",
      "--danger-hover": "#f47571",
      "--warning": "#eabf00",
    },
  },
  {
    id: "catppuccin-mocha",
    name: "Catppuccin Mocha",
    credit: "catppuccin/palette",
    colors: {
      "--surface-bg": "#11111b",
      "--surface-0": "#181825",
      "--surface-1": "#1e1e2e",
      "--surface-2": "#313244",
      "--surface-3": "#45475a",
      "--border": "#313244",
      "--border-hover": "#585b70",
      "--text-primary": "#cdd6f4",
      "--text-secondary": "#bac2de",
      "--text-tertiary": "#a6adc8",
      "--accent": "#94e2d5",
      "--accent-hover": "#89dceb",
      "--success": "#a6e3a1",
      "--danger": "#f38ba8",
      "--danger-hover": "#eba0ac",
      "--warning": "#f9e2af",
    },
  },
  {
    id: "nord",
    name: "Nord",
    credit: "nordtheme.com",
    colors: {
      "--surface-bg": "#242933",
      "--surface-0": "#2e3440",
      "--surface-1": "#3b4252",
      "--surface-2": "#434c5e",
      "--surface-3": "#4c566a",
      "--border": "#3b4252",
      "--border-hover": "#4c566a",
      "--text-primary": "#eceff4",
      "--text-secondary": "#d8dee9",
      "--text-tertiary": "#a9b2c3",
      "--accent": "#88c0d0",
      "--accent-hover": "#8fbcbb",
      "--success": "#a3be8c",
      "--danger": "#bf616a",
      "--danger-hover": "#d08770",
      "--warning": "#ebcb8b",
    },
  },
];

export const DEFAULT_THEME_ID = "default";

export function findTheme(id: string): Theme | undefined {
  return BUILT_IN_THEMES.find((theme) => theme.id === id);
}
