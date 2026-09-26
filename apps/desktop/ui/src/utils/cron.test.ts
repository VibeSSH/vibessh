import { describe, expect, it } from "vitest";
import { buildCron, isValidCron, nextRun, presetOf, type CronPreset } from "./cron";

// 2026-09-26 is a Saturday.
const saturdayNoonUtc = new Date("2026-09-26T12:00:00Z");

describe("isValidCron", () => {
  it("accepts what the backend accepts and refuses the rest", () => {
    expect(isValidCron("0 4 * * *")).toBe(true);
    expect(isValidCron("30 */6 1,15 1-12 1-5")).toBe(true);
    expect(isValidCron("0 4 * *")).toBe(false);
    expect(isValidCron("60 4 * * *")).toBe(false);
    expect(isValidCron("0 4 * * MON")).toBe(false);
    expect(isValidCron("*/0 * * * *")).toBe(false);
  });
});

describe("nextRun", () => {
  it("finds today's time when it has not passed yet, in the Node's zone", () => {
    // 04:00 on a UTC+2 Node is 02:00 UTC; at noon UTC that has passed today.
    expect(nextRun("0 4 * * *", saturdayNoonUtc, 120)?.toISOString()).toBe("2026-09-27T02:00:00.000Z");
    // 18:00 on the same Node is 16:00 UTC, still ahead today.
    expect(nextRun("0 18 * * *", saturdayNoonUtc, 120)?.toISOString()).toBe("2026-09-26T16:00:00.000Z");
  });

  it("skips to the next matching weekday, with 7 read as Sunday", () => {
    expect(nextRun("0 4 * * 1", saturdayNoonUtc, 0)?.toISOString()).toBe("2026-09-28T04:00:00.000Z");
    expect(nextRun("0 4 * * 7", saturdayNoonUtc, 0)?.toISOString()).toBe("2026-09-27T04:00:00.000Z");
  });

  it("matches either day field when both are restricted, as cron does", () => {
    // The 1st of the month, or any Monday - Monday the 28th comes first.
    expect(nextRun("0 4 1 * 1", saturdayNoonUtc, 0)?.toISOString()).toBe("2026-09-28T04:00:00.000Z");
  });

  it("steps through hours", () => {
    expect(nextRun("0 */6 * * *", saturdayNoonUtc, 0)?.toISOString()).toBe("2026-09-26T18:00:00.000Z");
  });

  it("gives up on a date that never comes", () => {
    expect(nextRun("0 4 31 2 *", saturdayNoonUtc, 0)).toBeNull();
    expect(nextRun("nonsense", saturdayNoonUtc, 0)).toBeNull();
  });
});

describe("presets", () => {
  it("round-trips every friendly shape", () => {
    const presets: CronPreset[] = [
      { kind: "daily", hour: 4, minute: 0 },
      { kind: "weekdays", hour: 6, minute: 30, days: [1, 3, 5] },
      { kind: "hours", every: 6 },
    ];
    for (const preset of presets) {
      expect(presetOf(buildCron(preset))).toEqual(preset);
    }
  });

  it("falls back to custom for anything else", () => {
    expect(presetOf("*/15 * * * *")).toEqual({ kind: "custom", expression: "*/15 * * * *" });
    expect(presetOf("0 4 1 * *")).toEqual({ kind: "custom", expression: "0 4 1 * *" });
  });
});
