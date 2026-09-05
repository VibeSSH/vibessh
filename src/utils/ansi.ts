/**
 * Turns a line of server output into coloured pieces.
 *
 * **Why this exists.** The Node terminal has had colour all along because
 * xterm.js understands escape sequences. The Application console never did:
 * it coloured whole lines by the log level it could read out of the text, so
 * a Paper stack trace arrived with its first line red and the twenty
 * continuation lines beneath it uniformly grey, and anything the server
 * itself coloured - a plugin's output, a player's chat - arrived as plain
 * text with the escape sequences either stripped or shown raw.
 *
 * Both vocabularies are handled because servers use both. ANSI is what the
 * process writes to its own stdout; the section sign is what Minecraft uses
 * inside the game, and it reaches the console whenever chat is logged.
 */

export interface AnsiSegment {
  text: string;
  /** A CSS colour, already resolved - the caller sets it as an inline style. */
  color?: string;
  background?: string;
  bold?: boolean;
  italic?: boolean;
  underline?: boolean;
  strikethrough?: boolean;
}

/**
 * The sixteen classic terminal colours.
 *
 * Tuned for a dark surface rather than taken from the VGA original: the
 * standard `#0000AA` blue and `#000000` black are unreadable on this app's
 * background, and a console nobody can read is worse than one with no colour
 * at all. Same reasoning every terminal emulator applies to its own default
 * scheme.
 */
const BASE_COLORS = [
  "#5c6773", // black - lifted to a grey that is actually visible
  "#f2777a", // red
  "#8ec07c", // green
  "#e5c07b", // yellow
  "#6fb3d2", // blue
  "#c397d8", // magenta
  "#70c0ba", // cyan
  "#d3d7cf", // white
  "#7f8c99", // bright black
  "#ff8b8b", // bright red
  "#a3e08e", // bright green
  "#ffd68a", // bright yellow
  "#8ac6f2", // bright blue
  "#d7a3e8", // bright magenta
  "#87d7d0", // bright cyan
  "#ffffff", // bright white
];

/** Minecraft's own sixteen, in the order its codes 0-9 and a-f run. */
const MINECRAFT_COLORS: Record<string, string> = {
  "0": "#5c6773",
  "1": "#3b4cca",
  "2": "#4e9a4e",
  "3": "#3ba3a3",
  "4": "#d05050",
  "5": "#a350c0",
  "6": "#e0a030",
  "7": "#9aa0a6",
  "8": "#6b7280",
  "9": "#6f8fe0",
  a: "#7fd47f",
  b: "#7fd9d9",
  c: "#ff7b7b",
  d: "#e79ae7",
  e: "#ffd76a",
  f: "#ffffff",
};

/** The 256-colour cube, computed rather than tabulated. */
function xterm256(index: number): string {
  if (index < 16) return BASE_COLORS[index];
  if (index < 232) {
    const level = (value: number) => (value === 0 ? 0 : 55 + value * 40);
    const n = index - 16;
    return rgb(level(Math.floor(n / 36) % 6), level(Math.floor(n / 6) % 6), level(n % 6));
  }
  const grey = 8 + (index - 232) * 10;
  return rgb(grey, grey, grey);
}

function rgb(r: number, g: number, b: number): string {
  const hex = (value: number) => Math.max(0, Math.min(255, value)).toString(16).padStart(2, "0");
  return `#${hex(r)}${hex(g)}${hex(b)}`;
}

// Both an escape sequence and a section-sign code, in one pass - so a line
// carrying both does not need two rounds of splitting that would each have
// to understand the other's output.
//
// The section sign only, never the ampersand people type in config files:
// in a log line an ampersand is overwhelmingly just an ampersand, and
// treating "R&D" as red text would corrupt ordinary output to catch a code
// the server does not emit.
const TOKEN = /\x1b\[([0-9;]*)m|§([0-9a-fk-or])/gi;

interface Style {
  color?: string;
  background?: string;
  bold?: boolean;
  italic?: boolean;
  underline?: boolean;
  strikethrough?: boolean;
}

export function parseAnsi(line: string): AnsiSegment[] {
  const segments: AnsiSegment[] = [];
  let style: Style = {};
  let cursor = 0;

  const push = (text: string) => {
    if (text.length > 0) segments.push({ text, ...style });
  };

  for (const match of line.matchAll(TOKEN)) {
    push(line.slice(cursor, match.index));
    cursor = match.index + match[0].length;
    style = match[1] !== undefined ? applySgr(style, match[1]) : applyMinecraft(style, match[2].toLowerCase());
  }
  push(line.slice(cursor));

  // A line with no codes at all is one plain segment, which is what the
  // caller renders as ordinary text.
  return segments.length > 0 ? segments : [{ text: line }];
}

function applySgr(style: Style, params: string): Style {
  // A bare `[m` means reset, same as `[0m`.
  const codes = params === "" ? [0] : params.split(";").map((value) => Number(value) || 0);
  let next: Style = { ...style };

  for (let i = 0; i < codes.length; i += 1) {
    const code = codes[i];
    if (code === 0) next = {};
    else if (code === 1) next.bold = true;
    else if (code === 3) next.italic = true;
    else if (code === 4) next.underline = true;
    else if (code === 9) next.strikethrough = true;
    else if (code === 22) next.bold = false;
    else if (code === 23) next.italic = false;
    else if (code === 24) next.underline = false;
    else if (code === 29) next.strikethrough = false;
    else if (code >= 30 && code <= 37) next.color = BASE_COLORS[code - 30];
    else if (code >= 90 && code <= 97) next.color = BASE_COLORS[code - 90 + 8];
    else if (code >= 40 && code <= 47) next.background = BASE_COLORS[code - 40];
    else if (code >= 100 && code <= 107) next.background = BASE_COLORS[code - 100 + 8];
    else if (code === 39) next.color = undefined;
    else if (code === 49) next.background = undefined;
    else if (code === 38 || code === 48) {
      // Extended colour, which is where the real hex support lives:
      // `38;2;R;G;B` is 24-bit, `38;5;N` indexes the 256-colour table. Both
      // consume their own parameters, so the loop skips past them.
      const target = code === 38 ? "color" : "background";
      if (codes[i + 1] === 2) {
        next[target] = rgb(codes[i + 2], codes[i + 3], codes[i + 4]);
        i += 4;
      } else if (codes[i + 1] === 5) {
        next[target] = xterm256(codes[i + 2]);
        i += 2;
      }
    }
  }
  return next;
}

function applyMinecraft(style: Style, code: string): Style {
  // A colour resets the formatting with it, which is how Minecraft's own
  // codes behave - `§c§lfoo` is bold red, `§l§cfoo` is plain red.
  const color = MINECRAFT_COLORS[code];
  if (color) return { color };

  switch (code) {
    case "l":
      return { ...style, bold: true };
    case "o":
      return { ...style, italic: true };
    case "n":
      return { ...style, underline: true };
    case "m":
      return { ...style, strikethrough: true };
    case "r":
      return {};
    // `k` is the obfuscated/"magic" code, an animation this has no business
    // reproducing in a log. Its text is left readable rather than hidden.
    default:
      return style;
  }
}
