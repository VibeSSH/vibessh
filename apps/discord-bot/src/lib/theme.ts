/**
 * The bot's colours, matching the VibeSSH app: a teal accent used as a signal,
 * not a wash, plus the standard status tones. Kept here so every panel and
 * every role the setup creates reads from one palette.
 */
export const COLOR = {
  /** VibeSSH teal - the brand accent. */
  accent: 0x14b8a6,
  neutral: 0x2b2d31,
  success: 0x22c55e,
  warning: 0xf59e0b,
  danger: 0xef4444,
  /** Language accents, reused for the language roles and their panels. */
  polski: 0xdc2626,
  english: 0x2563eb,
} as const;
