/**
 * What severity a console line is, judged from the line itself.
 *
 * There is no structured level available: `docker logs` returns whatever
 * the process wrote to stdout, so the only thing to go on is the shape of
 * the text. That makes this a heuristic, and the useful question is not
 * "can it be perfect" but "which way should it be wrong".
 *
 * It is deliberately conservative. A line wrongly coloured red is worse
 * than one left plain: the whole value of colour here is that red means
 * something, and a screen where a third of the lines are red because they
 * happen to contain the word "error" carries less information than no
 * colour at all.
 */
export type LogLevel = "error" | "warn" | "debug" | "info";

/**
 * How far into a line a level marker is looked for.
 *
 * Log prefixes live at the start - a timestamp, a thread, then the level.
 * Past this it is message text, where "error" is an ordinary English word:
 * "no error was reported", "Error handling improved". Bounding the search
 * is what stops those going red.
 *
 * Generous enough for the shapes actually seen here, the longest being
 * Docker's RFC 3339 stamp followed by Paper's own bracketed clock and
 * level: `2026-09-02T12:45:39.247554712Z [12:45:39 WARN]: ...` puts the
 * marker at column 42.
 */
const PREFIX_CHARS = 96;

/**
 * Level words as they appear in a prefix, most severe first.
 *
 * Order matters: `WARNING` has to be tested before `WARN` would match a
 * shorter prefix of it, and a line carrying both an error and a warning
 * marker should read as the more severe of the two.
 */
const MARKERS: ReadonlyArray<readonly [LogLevel, RegExp]> = [
  // `\b` on both sides, so `TERROR` and `ERRORS` do not match, and the
  // separator can be any of the punctuations these formats use:
  // `[12:00:00 ERROR]:`, `level=error`, `ERROR:`, ` E `, `<3>`.
  ["error", /\b(?:ERROR|SEVERE|FATAL|CRITICAL|PANIC|EMERG)\b|\blevel=(?:error|fatal|critical)\b/],
  ["warn", /\b(?:WARN|WARNING)\b|\blevel=warn(?:ing)?\b/],
  ["debug", /\b(?:DEBUG|TRACE|VERBOSE)\b|\blevel=(?:debug|trace)\b/],
];

/**
 * Classifies one line.
 *
 * Case-sensitive for the bare words on purpose. Every logging framework
 * that emits a level emits it upper-case; matching lower-case `error` would
 * catch ordinary prose in the same breath, which is the failure this is
 * written to avoid. The `level=` forms are lower-case because that is how
 * logfmt writes them, and they are unambiguous enough to trust.
 */
export function logLevelOf(line: string): LogLevel {
  const prefix = line.slice(0, PREFIX_CHARS);
  for (const [level, pattern] of MARKERS) {
    if (pattern.test(prefix)) return level;
  }
  return "info";
}
