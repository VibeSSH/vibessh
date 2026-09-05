import { describe, expect, it } from "vitest";
import { parseAnsi } from "./ansi";

const ESC = "";
const BEL = "";

describe("colouring a line of server output", () => {
  it("leaves a plain line as one plain piece", () => {
    const [only, ...rest] = parseAnsi("[21:27:34 INFO]: Done (12.3s)!");

    expect(rest).toHaveLength(0);
    expect(only.text).toBe("[21:27:34 INFO]: Done (12.3s)!");
    expect(only.color).toBeUndefined();
  });

  /**
   * The reason the pattern insists on the escape character. Log lines are
   * full of square brackets, and a looser match would eat them.
   */
  it("does not mistake ordinary brackets for escape sequences", () => {
    const line = "[21:27:34 ERROR]: at Foo.bar(Foo.java:150) [?:?]";

    expect(parseAnsi(line)).toEqual([{ text: line }]);
  });

  it("splits a coloured fragment out of the line around it", () => {
    const segments = parseAnsi(`plain ${ESC}[31mred${ESC}[0m plain again`);

    expect(segments.map((segment) => segment.text)).toEqual(["plain ", "red", " plain again"]);
    expect(segments[1].color).toBeDefined();
    expect(segments[0].color).toBeUndefined();
    expect(segments[2].color).toBeUndefined();
  });

  it("carries a style forward until it is reset", () => {
    const segments = parseAnsi(`${ESC}[1mbold ${ESC}[31mand red${ESC}[0m neither`);

    expect(segments[0].bold).toBe(true);
    expect(segments[1].bold).toBe(true);
    expect(segments[1].color).toBeDefined();
    expect(segments[2].bold).toBeUndefined();
    expect(segments[2].color).toBeUndefined();
  });

  /** The 24-bit form - what "hex support" actually means on the wire. */
  it("reads a true-colour sequence as its exact colour", () => {
    const [segment] = parseAnsi(`${ESC}[38;2;255;128;0mamber`);

    expect(segment.color).toBe("#ff8000");
    expect(segment.text).toBe("amber");
  });

  it("reads a 256-colour index", () => {
    // 196 is the cube's pure red.
    const [segment] = parseAnsi(`${ESC}[38;5;196mred`);

    expect(segment.color).toBe("#ff0000");
  });

  it("does not let an extended colour's own numbers be read as more codes", () => {
    // `2;255;128;0` belongs to the 38 - read as separate codes, the 0 would
    // reset everything and the colour would vanish.
    const [segment] = parseAnsi(`${ESC}[38;2;255;128;0mstill amber`);

    expect(segment.color).toBe("#ff8000");
  });

  it("understands a background as well as a foreground", () => {
    const [segment] = parseAnsi(`${ESC}[48;2;0;0;255mon blue`);

    expect(segment.background).toBe("#0000ff");
    expect(segment.color).toBeUndefined();
  });

  /**
   * Seen in a real Paper log: a line arriving with a bare reset in front of
   * it, which printed as the literal text `[m` because only colour
   * sequences were being consumed.
   */
  it("swallows a sequence it does not render rather than printing it", () => {
    const segments = parseAnsi(`${ESC}[K${ESC}[2J[21:58:52 INFO]: Enabling`);

    expect(segments.map((segment) => segment.text).join("")).toBe("[21:58:52 INFO]: Enabling");
  });

  it("still colours when a swallowed sequence sits beside a colour", () => {
    const segments = parseAnsi(`${ESC}[m${ESC}[32mgreen`);

    expect(segments.map((segment) => segment.text).join("")).toBe("green");
    expect(segments[segments.length - 1].color).toBeDefined();
  });

  it("swallows an operating system command with its terminator", () => {
    // A server setting the window title, which has no meaning in a log.
    const segments = parseAnsi(`${ESC}]0;a title${BEL}after`);

    expect(segments.map((segment) => segment.text).join("")).toBe("after");
  });

  describe("Minecraft's own codes, as they arrive from chat", () => {
    it("colours what follows a section sign", () => {
      const segments = parseAnsi("<CrispiDEV> §chello");

      expect(segments[0].text).toBe("<CrispiDEV> ");
      expect(segments[1].text).toBe("hello");
      expect(segments[1].color).toBeDefined();
    });

    it("applies bold after a colour, and drops it when the colour changes", () => {
      // Minecraft's rule: a colour code resets formatting with it.
      const [, boldRed, plainGreen] = parseAnsi("x§c§lbold red§aplain green");

      expect(boldRed.bold).toBe(true);
      expect(plainGreen.bold).toBeUndefined();
    });

    it("leaves obfuscated text readable rather than hiding it", () => {
      const segments = parseAnsi("§ksecret");

      expect(segments.map((segment) => segment.text).join("")).toBe("secret");
    });
  });
});
