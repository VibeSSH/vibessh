/**
 * The colours a theme is allowed to set.
 *
 * **Why this list is closed.** A theme is data, not a stylesheet: it maps
 * these names to colours and nothing else. If a shared theme could carry
 * arbitrary CSS it could move, hide or repaint anything - including making
 * "Delete application" look like "Cancel", or hiding the warning in a
 * confirmation dialog. This app runs destructive commands against production
 * servers, so a theme is kept to values it cannot misuse.
 *
 * Deliberately excludes spacing, radii and the z-index ladder. A badly chosen
 * colour is ugly; a changed `--space-4` or `--z-modal` breaks the layout in
 * ways the eye cannot undo.
 */
export const THEMABLE_TOKENS = [
  "--surface-bg",
  "--surface-0",
  "--surface-1",
  "--surface-2",
  "--surface-3",
  "--border",
  "--border-hover",
  "--text-primary",
  "--text-secondary",
  "--text-tertiary",
  "--accent",
  "--accent-hover",
  "--success",
  "--danger",
  "--danger-hover",
  "--warning",
] as const;

export type ThemableToken = (typeof THEMABLE_TOKENS)[number];

/**
 * Colours the app computes rather than asks for.
 *
 * `--accent-contrast` is the text drawn *on* the accent, and `--danger-text`
 * exists because `--danger` as text lands below AA on a card. Asking somebody
 * choosing a colour to also pick a readable partner for it is asking them to
 * know WCAG; getting it wrong makes labels vanish into their own buttons. So
 * these are derived from the colours that were chosen - see `deriveContrast`.
 */
export const DERIVED_TOKENS = ["--accent-contrast", "--danger-text"] as const;

export interface Theme {
  id: string;
  /** Shown in the picker. Not translated - these are proper names. */
  name: string;
  /** Where the palette came from, for the note under the picker. */
  credit?: string;
  colors: Record<ThemableToken, string>;
}
