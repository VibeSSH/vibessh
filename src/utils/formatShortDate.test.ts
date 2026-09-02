import { describe, expect, it } from "vitest";
import { formatShortDate } from "./formatShortDate";

describe("formatShortDate", () => {
  it("formats in the locale it is given", () => {
    const date = new Date(Date.UTC(2026, 8, 2, 12, 0, 0));
    expect(formatShortDate(date, "en-GB")).toBe("02/09/2026");
    expect(formatShortDate(date, "en-US")).toBe("9/2/2026");
  });

  // The formatter is cached by locale; switching back and forth must not
  // hand back the previous language's formatter.
  it("follows a language change in both directions", () => {
    const date = new Date(Date.UTC(2026, 8, 2, 12, 0, 0));
    expect(formatShortDate(date, "en-GB")).toBe("02/09/2026");
    expect(formatShortDate(date, "en-US")).toBe("9/2/2026");
    expect(formatShortDate(date, "en-GB")).toBe("02/09/2026");
  });

  it("accepts what a listing actually carries", () => {
    expect(formatShortDate("2026-09-02T12:00:00Z", "en-GB")).toBe("02/09/2026");
    expect(formatShortDate(Date.UTC(2026, 8, 2), "en-GB")).toBe("02/09/2026");
  });

  // A Node can write a timestamp in a shape we cannot read. "Invalid Date"
  // in a file listing is worse than an empty cell.
  it("says nothing about a date it cannot read", () => {
    expect(formatShortDate("not a date", "en-GB")).toBe("");
  });
});
