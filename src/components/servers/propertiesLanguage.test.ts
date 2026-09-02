import { describe, expect, it } from "vitest";
import { classifyValue } from "./propertiesLanguage";

/**
 * Values taken from a real `server.properties`. The point of the colouring
 * is that a wrong value looks wrong, which only holds if the right ones are
 * classified the way somebody reading the file would classify them.
 */
describe("classifyValue", () => {
  it("reads the numbers a server config actually contains", () => {
    expect(classifyValue("20")).toBe("number");
    expect(classifyValue("25565")).toBe("number");
    expect(classifyValue("-1")).toBe("number");
    expect(classifyValue("0.5")).toBe("number");
    expect(classifyValue("  16  ")).toBe("number");
  });

  it("reads the two spellings the format calls booleans", () => {
    expect(classifyValue("true")).toBe("bool");
    expect(classifyValue("false")).toBe("bool");
  });

  /**
   * `yes` and `on` are not booleans to a Java properties reader. Colouring
   * them as if they were would teach the file's own rules wrongly, which is
   * worse than leaving them plain.
   */
  it("does not invent booleans the format does not have", () => {
    expect(classifyValue("yes")).toBe("string");
    expect(classifyValue("on")).toBe("string");
    expect(classifyValue("TRUE")).toBe("string");
  });

  /**
   * The case the strict pattern exists for: a version is not a number, and
   * it is exactly the kind of value people scan for.
   */
  it("leaves versions and addresses as text", () => {
    expect(classifyValue("1.21.11")).toBe("string");
    expect(classifyValue("127.0.0.1")).toBe("string");
    expect(classifyValue("2G")).toBe("string");
  });

  it("treats prose and empty values as text", () => {
    expect(classifyValue("A Minecraft Server")).toBe("string");
    expect(classifyValue("")).toBe("string");
    expect(classifyValue("   ")).toBe("string");
  });
});
