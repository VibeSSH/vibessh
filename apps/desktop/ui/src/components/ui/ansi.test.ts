import { describe, expect, it } from "vitest";
import { parseAnsiLine, splitDockerTimestamp } from "./ansi";

const ESC = "\u001b";

describe("parseAnsiLine", () => {
  it("turns a Paper WARN line into coloured text with no escape left in it", () => {
    // The line from the report: Paper resets, sets bold yellow, and the log
    // view printed `[m[33;1m` in front of every warning.
    const { segments } = parseAnsiLine(`${ESC}[m${ESC}[33;1m[14:01:50 WARN]: Unknown minor version${ESC}[m`);

    expect(segments).toHaveLength(1);
    expect(segments[0].text).toBe("[14:01:50 WARN]: Unknown minor version");
    expect(segments[0].style).toMatchObject({ color: "var(--warning)", bold: true });
  });

  it("leaves plain text as one unstyled run", () => {
    const { segments } = parseAnsiLine("[14:09:31 INFO]: [Images] Loaded 1 images...");
    expect(segments).toEqual([{ text: "[14:09:31 INFO]: [Images] Loaded 1 images...", style: {} }]);
  });

  it("splits text where the colour changes and resets on 0", () => {
    const { segments } = parseAnsiLine(`a${ESC}[31mb${ESC}[0mc`);
    expect(segments.map((s) => s.text)).toEqual(["a", "b", "c"]);
    expect(segments[1].style.color).toBe("var(--danger-text)");
    expect(segments[2].style).toEqual({});
  });

  it("reads 256-colour and true-colour foregrounds, and skips backgrounds without misreading them", () => {
    expect(parseAnsiLine(`${ESC}[38;5;196mx`).segments[0].style.color).toBe("rgb(255, 0, 0)");
    expect(parseAnsiLine(`${ESC}[38;2;10;20;30mx`).segments[0].style.color).toBe("rgb(10, 20, 30)");
    // `48;5;31` must not be read as "31 = red".
    expect(parseAnsiLine(`${ESC}[48;5;31mx`).segments[0].style.color).toBeUndefined();
  });

  it("drops cursor movement, line clearing and window titles", () => {
    const { segments } = parseAnsiLine(`${ESC}[2K${ESC}[1Gdone${ESC}]0;title\u0007!`);
    expect(segments.map((s) => s.text).join("")).toBe("done!");
  });

  it("carries a colour left open into the next line", () => {
    const first = parseAnsiLine(`${ESC}[32mstarted`);
    const second = parseAnsiLine("still green", first.style);
    expect(second.segments[0].style.color).toBe("var(--success)");
  });

  it("removes stray control characters rather than printing boxes", () => {
    expect(parseAnsiLine("a\u0007b\u0000c\td").segments[0].text).toBe("abc\td");
  });
});

describe("splitDockerTimestamp", () => {
  it("splits off the stamp docker logs --timestamps adds", () => {
    const { timestamp, raw, rest } = splitDockerTimestamp("2026-09-25T14:01:50.426168440Z [14:01:50 WARN]: hi");
    expect(timestamp?.toISOString()).toBe("2026-09-25T14:01:50.000Z");
    expect(raw).toBe("2026-09-25T14:01:50.426168440Z");
    expect(rest).toBe("[14:01:50 WARN]: hi");
  });

  it("leaves a line without one alone", () => {
    expect(splitDockerTimestamp("no stamp here")).toEqual({ timestamp: null, raw: null, rest: "no stamp here" });
  });
});
