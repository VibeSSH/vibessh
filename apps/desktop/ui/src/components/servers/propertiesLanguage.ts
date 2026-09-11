import { StreamLanguage } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";

/**
 * What kind of value sits on the right of a `.properties` line.
 *
 * The format itself has no types - everything is a string, and the server
 * parses it however it likes. This is a reading aid, not a claim about the
 * file: `max-players=20` and `motd=A Minecraft Server` look identical
 * without it, and the first is the kind of line people mistype.
 */
export type PropertyValueKind = "number" | "bool" | "string";

export function classifyValue(raw: string): PropertyValueKind {
  const value = raw.trim();
  if (value === "") return "string";

  // Only the two spellings the format actually uses. `yes`/`on` are not
  // booleans to a Java properties reader, and colouring them as if they
  // were would be teaching the file's own rules wrongly.
  if (value === "true" || value === "false") return "bool";

  // Deliberately strict: a leading sign, digits, one optional fractional
  // part. `1.21.11` is a version and stays a string, which matters because
  // that is exactly the sort of value somebody scans for.
  if (/^[+-]?\d+(\.\d+)?$/.test(value)) return "number";

  return "string";
}

interface PropertiesState {
  /** Everything after the first separator on this line is the value, even
   * if it contains more `=` or `:` characters - which URLs and message
   * templates routinely do. */
  inValue: boolean;
}

/**
 * Syntax highlighting for `.properties`, replacing the legacy mode.
 *
 * The legacy mode marks comments and leaves the rest one colour, so a
 * `server.properties` was a wall of identical text. Here the key, the
 * separator and the value are distinguishable, and the value's own shape
 * shows: numbers and booleans read differently from free text, which is
 * what makes a wrong one visible.
 *
 * The token names are mapped explicitly rather than relying on the legacy
 * name table, so what the theme is asked for is written down here.
 */
export const propertiesLanguage = StreamLanguage.define<PropertiesState>({
  name: "properties",

  startState: () => ({ inValue: false }),

  token(stream, state) {
    if (stream.sol()) {
      state.inValue = false;
      // Leading whitespace is not part of the key.
      if (stream.eatSpace()) return null;
      // Both comment markers the format allows.
      if (stream.peek() === "#" || stream.peek() === "!") {
        stream.skipToEnd();
        return "comment";
      }
    }

    if (stream.eatSpace()) return null;

    if (!state.inValue) {
      if (stream.peek() === "=" || stream.peek() === ":") {
        stream.next();
        state.inValue = true;
        return "operator";
      }
      // Up to the separator, or the whole line if there is none - a bare
      // key with no value is legal and should still read as a key.
      while (!stream.eol()) {
        const next = stream.peek();
        if (next === "=" || next === ":") break;
        stream.next();
      }
      return "propertiesKey";
    }

    const rest = stream.string.slice(stream.pos);
    stream.skipToEnd();
    switch (classifyValue(rest)) {
      case "number":
        return "propertiesNumber";
      case "bool":
        return "propertiesBool";
      default:
        return "propertiesString";
    }
  },

  tokenTable: {
    propertiesKey: t.propertyName,
    propertiesNumber: t.number,
    propertiesBool: t.bool,
    propertiesString: t.string,
  },
});
