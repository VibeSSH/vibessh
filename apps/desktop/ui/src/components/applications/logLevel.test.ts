import { describe, expect, it } from "vitest";
import { logLevelOf } from "./logLevel";

/**
 * The interesting half of these tests is the negative one.
 *
 * Getting `WARN` orange is easy. What decides whether the colour is worth
 * having is whether ordinary sentences containing the word "error" stay
 * plain - because a console where a third of the lines are red carries less
 * information than one with no colour at all.
 */
describe("logLevelOf", () => {
  it("reads Paper's bracketed level, through Docker's timestamp", () => {
    const line = (level: string) => `2026-09-02T12:45:39.247554712Z [12:45:39 ${level}]: Loading Paper 1.21.11`;
    expect(logLevelOf(line("INFO"))).toBe("info");
    expect(logLevelOf(line("WARN"))).toBe("warn");
    expect(logLevelOf(line("ERROR"))).toBe("error");
  });

  it("reads the logfmt and bare-word shapes other images use", () => {
    expect(logLevelOf("time=2026-09-02T12:00:00Z level=error msg=\"connection refused\"")).toBe("error");
    expect(logLevelOf("time=2026-09-02T12:00:00Z level=warning msg=\"retrying\"")).toBe("warn");
    expect(logLevelOf("ERROR: could not bind to port")).toBe("error");
    expect(logLevelOf("[WARNING] deprecated option")).toBe("warn");
    expect(logLevelOf("SEVERE: unable to start")).toBe("error");
  });

  it("treats debug and trace as their own, quieter level", () => {
    expect(logLevelOf("2026-09-02T12:00:00Z [12:00:00 DEBUG]: tick took 4ms")).toBe("debug");
    expect(logLevelOf("level=trace span=startup")).toBe("debug");
  });

  /** The point of bounding the search to the prefix. */
  it("leaves message text alone", () => {
    expect(logLevelOf("2026-09-02T12:00:00Z [12:00:00 INFO]: no error was reported during startup")).toBe("info");
    expect(logLevelOf("2026-09-02T12:00:00Z [12:00:00 INFO]: Error handling has been improved")).toBe("info");
    expect(logLevelOf("Done (20.870s)! For help, type \"help\"")).toBe("info");
  });

  /** Word boundaries, so a level word inside another word is not a level. */
  it("does not match a level word buried in a longer one", () => {
    expect(logLevelOf("2026-09-02T12:00:00Z [12:00:00 INFO]: TERROR mob spawned")).toBe("info");
    expect(logLevelOf("2026-09-02T12:00:00Z [12:00:00 INFO]: WARNINGS_DISABLED=1")).toBe("info");
  });

  /** A line carrying both markers reads as the more severe one. */
  it("prefers the more severe marker", () => {
    expect(logLevelOf("[12:00:00 ERROR]: WARN threshold exceeded")).toBe("error");
  });

  it("has an opinion about nothing in particular", () => {
    expect(logLevelOf("")).toBe("info");
    expect(logLevelOf("   ")).toBe("info");
  });
});
