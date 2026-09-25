/**
 * ANSI colour codes in a log line, turned into styled runs of text.
 *
 * The console goes through xterm, which understands these by itself. The
 * log views are plain text, and printed every escape as a literal
 * `[33;1m` in front of the line - Paper colours its own WARN and ERROR
 * lines, so on a Minecraft server that was most of the screen.
 *
 * Only colour and emphasis are kept. Anything else a process writes to a
 * terminal - cursor movement, clearing the line, window titles - means
 * nothing in a static list of lines and is dropped.
 */

export interface AnsiStyle {
  /** A CSS colour, or undefined for the default text colour. */
  color?: string;
  bold?: boolean;
  dim?: boolean;
  italic?: boolean;
  underline?: boolean;
}

export interface AnsiSegment {
  text: string;
  style: AnsiStyle;
}

/**
 * The 16 basic colours. The four that carry meaning in a log - red, yellow,
 * green and the greys - use the app's own tokens, so a WARN line here is the
 * same yellow as a warning anywhere else; the rest are tuned to read on the
 * dark log background without shouting.
 */
const BASIC: readonly string[] = [
  "var(--text-tertiary)", // black: pure black is invisible on the log background
  "var(--danger-text)",
  "var(--success)",
  "var(--warning)",
  "#6b9cf0",
  "#c586e0",
  "#4cc7d6",
  "var(--text-secondary)",
  "var(--text-tertiary)",
  "#ff7b72",
  "#56d364",
  "#f2cc60",
  "#8fb8ff",
  "#dba6f0",
  "#76e0ec",
  "var(--text-primary)",
];

/** Colour `n` of the xterm 256-colour palette. */
function palette256(n: number): string | undefined {
  if (n < 0 || n > 255) return undefined;
  if (n < 16) return BASIC[n];
  if (n >= 232) {
    const level = 8 + (n - 232) * 10;
    return `rgb(${level}, ${level}, ${level})`;
  }
  const index = n - 16;
  const step = (value: number) => (value === 0 ? 0 : 55 + value * 40);
  return `rgb(${step(Math.floor(index / 36))}, ${step(Math.floor(index / 6) % 6)}, ${step(index % 6)})`;
}

/** Applies one SGR sequence's parameters to the running style. */
function applySgr(style: AnsiStyle, params: number[]): AnsiStyle {
  let next = { ...style };
  for (let i = 0; i < params.length; i++) {
    const code = params[i];
    if (code === 0) next = {};
    else if (code === 1) next.bold = true;
    else if (code === 2) next.dim = true;
    else if (code === 3) next.italic = true;
    else if (code === 4) next.underline = true;
    else if (code === 22) {
      next.bold = false;
      next.dim = false;
    } else if (code === 23) next.italic = false;
    else if (code === 24) next.underline = false;
    else if (code >= 30 && code <= 37) next.color = BASIC[code - 30];
    else if (code >= 90 && code <= 97) next.color = BASIC[code - 90 + 8];
    else if (code === 39) next.color = undefined;
    else if (code === 38 || code === 48) {
      // Extended colour: `38;5;n` or `38;2;r;g;b`. Backgrounds (48) are
      // consumed so their numbers are not misread as codes, but not drawn -
      // a coloured band behind log text is noise the view does not need.
      const mode = params[i + 1];
      if (mode === 5) {
        if (code === 38) next.color = palette256(params[i + 2]);
        i += 2;
      } else if (mode === 2) {
        if (code === 38) next.color = `rgb(${params[i + 2] ?? 0}, ${params[i + 3] ?? 0}, ${params[i + 4] ?? 0})`;
        i += 4;
      }
    }
    // Everything else - backgrounds 40-47/100-107, blink, reverse - is ignored.
  }
  return next;
}

/**
 * Any escape sequence: CSI (`ESC [ ... letter`), OSC (`ESC ] ... BEL` or
 * `ESC ] ... ESC \`), or a lone two-character escape.
 */
const ESCAPE = /\u001b(?:\[([0-9;?]*)([A-Za-z])|\][^\u0007\u001b]*(?:\u0007|\u001b\\)?|[@-_]?)/g;

/** Control characters other than tab, which would otherwise print as boxes. */
const CONTROL = /[\u0000-\u0008\u000b-\u001f\u007f]/g;

/**
 * Splits one line into runs of text sharing a style.
 *
 * `initial` is the style carried over from the line before: a process that
 * sets a colour and writes several lines before resetting it means all of
 * them. The style at the end of the line is returned for the next one.
 */
export function parseAnsiLine(line: string, initial: AnsiStyle = {}): { segments: AnsiSegment[]; style: AnsiStyle } {
  const segments: AnsiSegment[] = [];
  let style = initial;
  let last = 0;

  const push = (text: string) => {
    const clean = text.replace(CONTROL, "");
    if (!clean) return;
    const previous = segments[segments.length - 1];
    if (previous && previous.style === style) previous.text += clean;
    else segments.push({ text: clean, style });
  };

  ESCAPE.lastIndex = 0;
  for (let match = ESCAPE.exec(line); match; match = ESCAPE.exec(line)) {
    push(line.slice(last, match.index));
    last = match.index + match[0].length;
    // Only SGR (`m`) changes how text looks; other CSI commands are dropped.
    if (match[2] === "m") {
      const params = match[1] === "" ? [0] : match[1].split(";").map((part) => (part === "" ? 0 : Number(part)));
      style = applySgr(style, params);
    }
  }
  push(line.slice(last));
  return { segments, style };
}

/** Whether a line carries an escape at all - the server has coloured it itself. */
export function hasAnsi(line: string): boolean {
  return line.includes("\u001b");
}

/**
 * The RFC 3339 stamp `docker logs --timestamps` puts in front of every line,
 * split off so it can be shown quietly rather than as the loudest thing on it.
 */
const DOCKER_TIMESTAMP = /^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})(?:\.\d+)?Z /;

export function splitDockerTimestamp(line: string): { timestamp: Date | null; raw: string | null; rest: string } {
  const match = DOCKER_TIMESTAMP.exec(line);
  if (!match) return { timestamp: null, raw: null, rest: line };
  const timestamp = new Date(`${match[1]}Z`);
  if (Number.isNaN(timestamp.getTime())) return { timestamp: null, raw: null, rest: line };
  return { timestamp, raw: match[0].trim(), rest: line.slice(match[0].length) };
}
