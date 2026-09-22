/**
 * A tiny timestamped console logger.
 *
 * Not a logging framework - a bot this size needs a readable, greppable line
 * per event, not transports and levels nobody will configure. Colours are ANSI
 * so they show in a terminal and fall back to plain text in a file.
 */
const COLOURS = {
  info: "\x1b[36m", // cyan
  warn: "\x1b[33m", // yellow
  error: "\x1b[31m", // red
  ok: "\x1b[32m", // green
  reset: "\x1b[0m",
} as const;

function line(kind: keyof typeof COLOURS, label: string, message: string) {
  const time = new Date().toISOString().slice(11, 19);
  const stream = kind === "error" || kind === "warn" ? console.error : console.log;
  stream(`${COLOURS[kind]}${time} ${label.padEnd(5)}${COLOURS.reset} ${message}`);
}

export const log = {
  info: (message: string) => line("info", "info", message),
  ok: (message: string) => line("ok", "ok", message),
  warn: (message: string) => line("warn", "warn", message),
  error: (message: string, error?: unknown) => {
    line("error", "error", message);
    if (error instanceof Error) console.error(error.stack ?? error.message);
    else if (error !== undefined) console.error(error);
  },
};
