import { DERIVED_TOKENS, THEMABLE_TOKENS, type ThemableToken, type Theme } from "./tokens";

/** `#rgb`, `#rrggbb` or `#rrggbbaa`, and nothing else. */
const HEX = /^#(?:[0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/i;

/**
 * Parses a colour a theme offered, or returns null.
 *
 * Hex only, on purpose. `color-mix()`, `var()` and the rest are valid CSS and
 * would let a theme file reach back into tokens it is not allowed to set, or
 * smuggle in a `url()`. A closed list of names is only half the guard; the
 * values have to be closed too.
 */
export function parseColor(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return HEX.test(trimmed) ? trimmed.toLowerCase() : null;
}

function channels(hex: string): [number, number, number] {
  const body = hex.slice(1);
  const full = body.length === 3 ? body.replace(/./g, (c) => c + c) : body;
  return [parseInt(full.slice(0, 2), 16), parseInt(full.slice(2, 4), 16), parseInt(full.slice(4, 6), 16)];
}

/** Relative luminance, per WCAG 2.1's own definition. */
function luminance(hex: string): number {
  const [r, g, b] = channels(hex).map((channel) => {
    const s = channel / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

export function contrastRatio(a: string, b: string): number {
  const [light, dark] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (light + 0.05) / (dark + 0.05);
}

/**
 * Picks readable text for a background, and lifts a fill to a usable text
 * colour.
 *
 * `--accent-contrast` goes near-black or near-white, whichever wins against
 * the accent the user actually chose. `--danger-text` is the danger colour
 * lightened until it clears 4.5:1 on the card behind it - the same problem
 * the hand-written palette solved by hand, solved here for whatever colour
 * turns up.
 */
export function deriveContrast(colors: Record<ThemableToken, string>): Record<string, string> {
  const onAccent = contrastRatio(colors["--accent"], "#04141c") >= contrastRatio(colors["--accent"], "#f7fdff") ? "#04141c" : "#f7fdff";

  let dangerText = colors["--danger"];
  const surface = colors["--surface-1"];
  // Walk the colour towards white in small steps rather than jumping: the
  // point is the *first* readable shade, so the result still looks like the
  // danger colour rather than pink.
  for (let step = 0; step < 24 && contrastRatio(dangerText, surface) < 4.5; step += 1) {
    const [r, g, b] = channels(dangerText);
    dangerText = `#${[r, g, b].map((c) => Math.min(255, Math.round(c + (255 - c) * 0.08)).toString(16).padStart(2, "0")).join("")}`;
  }

  return { "--accent-contrast": onAccent, "--danger-text": dangerText };
}

/**
 * Reads a theme out of untrusted JSON.
 *
 * Unknown keys are dropped and unparseable colours are ignored rather than
 * failing the whole file, so a theme written for a later version - one that
 * knows a token this build does not - still applies the parts it shares. A
 * theme missing any required colour is rejected outright, because a half
 * applied palette is worse looking than either whole one.
 */
export function parseTheme(input: unknown): Theme | null {
  if (typeof input !== "object" || input === null) return null;
  const raw = input as Record<string, unknown>;
  const colors = {} as Record<ThemableToken, string>;
  for (const token of THEMABLE_TOKENS) {
    const parsed = parseColor((raw.colors as Record<string, unknown> | undefined)?.[token]);
    if (!parsed) return null;
    colors[token] = parsed;
  }
  const id = typeof raw.id === "string" && raw.id.length > 0 ? raw.id : "custom";
  const name = typeof raw.name === "string" && raw.name.trim().length > 0 ? raw.name.trim().slice(0, 40) : id;
  return { id, name, colors };
}

/**
 * Puts a theme on screen.
 *
 * Sets custom properties on the root element rather than injecting a
 * stylesheet. That is the whole safety story in one line: the only things
 * that can change are the properties named here, so a theme cannot reach
 * anything else however it was written.
 */
export function applyTheme(theme: Theme) {
  const root = document.documentElement;
  for (const token of THEMABLE_TOKENS) {
    root.style.setProperty(token, theme.colors[token]);
  }
  const derived = deriveContrast(theme.colors);
  for (const token of DERIVED_TOKENS) {
    root.style.setProperty(token, derived[token]);
  }
}

/** Drops every override, so the stylesheet's own values show through again. */
export function clearTheme() {
  const root = document.documentElement;
  for (const token of [...THEMABLE_TOKENS, ...DERIVED_TOKENS]) {
    root.style.removeProperty(token);
  }
}
