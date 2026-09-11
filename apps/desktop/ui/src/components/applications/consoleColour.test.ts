import { describe, expect, it } from "vitest";
import { translateMinecraftCodes, withLevelColour } from "./ApplicationConsoleCard";

const ESC = "";
const SECTION = "§";

describe("what reaches the terminal", () => {
  it("turns a Minecraft colour code into one xterm understands", () => {
    // xterm speaks ANSI and nothing else - a section sign left in place shows
    // as a literal character, which is what happened when the console moved
    // to xterm and this translation was not carried over.
    const translated = translateMinecraftCodes(`${SECTION}cDEVELOPER`);

    expect(translated).toBe(`${ESC}[0;91mDEVELOPER`);
    expect(translated).not.toContain(SECTION);
  });

  it("treats a colour as clearing the formatting before it, the way Minecraft does", () => {
    // `§l§c` is bold red; `§c§l` is red then bold - the colour resets first.
    expect(translateMinecraftCodes(`${SECTION}lbold${SECTION}cred`)).toBe(`${ESC}[1mbold${ESC}[0;91mred`);
  });

  it("drops a code it has no meaning for rather than printing it", () => {
    // `k` is the obfuscating animation, which a log has no business showing.
    expect(translateMinecraftCodes(`${SECTION}khidden`)).toBe("hidden");
  });

  it("leaves an ampersand alone", () => {
    // In a log line an ampersand is an ampersand. Treating "R&D" as red text
    // would corrupt ordinary output to catch a code the server never sends.
    expect(translateMinecraftCodes("R&D and Q&A")).toBe("R&D and Q&A");
  });

  it("leaves a line with no codes untouched", () => {
    expect(translateMinecraftCodes("[22:13:27 INFO]: hello")).toBe("[22:13:27 INFO]: hello");
  });
});

describe("shading a line the server did not colour", () => {
  it("marks an error line", () => {
    expect(withLevelColour("[22:13:27 ERROR]: it broke")).toContain(`${ESC}[31m`);
  });

  it("leaves an ordinary line as it is", () => {
    expect(withLevelColour("[22:13:27 INFO]: fine")).toBe("[22:13:27 INFO]: fine");
  });

  /**
   * The rule that matters: the server has already decided. Adding a severity
   * colour on top of its own would override a choice it made deliberately.
   */
  it("does not shade a line the server coloured itself", () => {
    const coloured = `${ESC}[32m[22:13:27 ERROR]: green on purpose`;

    expect(withLevelColour(coloured)).toBe(coloured);
  });

  it("counts a translated Minecraft code as the server having decided", () => {
    expect(withLevelColour(`${SECTION}c[22:13:27 ERROR]: red`)).toBe(`${ESC}[0;91m[22:13:27 ERROR]: red`);
  });
});
