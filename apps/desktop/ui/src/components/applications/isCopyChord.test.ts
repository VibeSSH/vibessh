import { describe, expect, it } from "vitest";
import { isCopyChord } from "./ApplicationConsoleCard";

function key(overrides: Partial<KeyboardEvent>): Pick<KeyboardEvent, "type" | "ctrlKey" | "metaKey" | "key"> {
  return { type: "keydown", ctrlKey: false, metaKey: false, key: "c", ...overrides };
}

/**
 * Which chord copies in the application console.
 *
 * Somebody selected a stack trace in the console, pressed Ctrl+C, and got
 * nothing: xterm swallows the keystroke, and this console had no handler for
 * it. The SSH terminal is the opposite case on purpose - plain Ctrl+C has to
 * reach the shell there as "interrupt" - so these pin that the two stay
 * different, and that a later attempt to make them consistent has to argue
 * with a failing test rather than quietly take copying away again.
 */
describe("isCopyChord", () => {
  it("accepts plain Ctrl+C, which the SSH terminal reserves for interrupt", () => {
    expect(isCopyChord(key({ ctrlKey: true }))).toBe(true);
  });

  it("accepts Ctrl+Shift+C, for the terminal habit", () => {
    expect(isCopyChord(key({ ctrlKey: true, shiftKey: true } as Partial<KeyboardEvent>))).toBe(true);
  });

  it("accepts an uppercase C, which is what the event carries with shift held", () => {
    expect(isCopyChord(key({ ctrlKey: true, key: "C" }))).toBe(true);
  });

  it("ignores C on its own - the console is full of text somebody may be reading", () => {
    expect(isCopyChord(key({}))).toBe(false);
  });

  /** xterm hands the handler both halves of every keystroke; acting on the
   *  release as well would copy twice and toast twice. */
  it("ignores the keyup half of the same keystroke", () => {
    expect(isCopyChord(key({ type: "keyup", ctrlKey: true }))).toBe(false);
  });

  it("ignores other chords", () => {
    expect(isCopyChord(key({ ctrlKey: true, key: "v" }))).toBe(false);
    expect(isCopyChord(key({ ctrlKey: true, key: "a" }))).toBe(false);
  });
});
