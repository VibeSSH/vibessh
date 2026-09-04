import { applyTheme, clearTheme, parseTheme } from "./applyTheme";
import { BUILT_IN_THEMES, DEFAULT_THEME_ID, findTheme } from "./themes";
import type { Theme } from "./tokens";

const SELECTED_KEY = "vibessh.theme";
const CUSTOM_KEY = "vibessh.theme.custom";

/**
 * The theme the user last chose, and their own edited one if they have made
 * it.
 *
 * On this machine rather than in the database: how somebody likes their own
 * window to look is not a property of the infrastructure, and two people on
 * one team should not be repainting each other's screens.
 */
export function loadCustomTheme(): Theme | null {
  try {
    const stored = localStorage.getItem(CUSTOM_KEY);
    return stored ? parseTheme(JSON.parse(stored)) : null;
  } catch {
    // Unreadable or not JSON. A missing custom theme is not a failure worth
    // reporting - the built-in ones are all still there.
    return null;
  }
}

export function saveCustomTheme(theme: Theme) {
  try {
    localStorage.setItem(CUSTOM_KEY, JSON.stringify(theme));
  } catch {
    // Storage unavailable; the theme still holds for this session.
  }
}

export function loadSelectedThemeId(): string {
  try {
    return localStorage.getItem(SELECTED_KEY) ?? DEFAULT_THEME_ID;
  } catch {
    return DEFAULT_THEME_ID;
  }
}

export function saveSelectedThemeId(id: string) {
  try {
    localStorage.setItem(SELECTED_KEY, id);
  } catch {
    // As above.
  }
}

/** Every theme the picker should offer, the user's own last. */
export function allThemes(): Theme[] {
  const custom = loadCustomTheme();
  return custom ? [...BUILT_IN_THEMES, custom] : BUILT_IN_THEMES;
}

export function resolveTheme(id: string): Theme | null {
  if (id === "custom") return loadCustomTheme();
  return findTheme(id) ?? null;
}

/**
 * Puts the stored theme on screen.
 *
 * Called from `main.tsx` before React mounts, so the window never paints once
 * in the default palette and then again in the chosen one. The default theme
 * clears its overrides instead of setting them, leaving `globals.css` to
 * speak for itself - which keeps the stylesheet the single source of the
 * shipped look rather than something the theme layer has to stay in step
 * with.
 */
export function applyStoredTheme() {
  const id = loadSelectedThemeId();
  if (id === DEFAULT_THEME_ID) {
    clearTheme();
    return;
  }
  const theme = resolveTheme(id);
  if (theme) applyTheme(theme);
  else clearTheme();
}
