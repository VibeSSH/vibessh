import { describe, expect, it } from "vitest";
import { formatBytes, formatBytesOf } from "./formatBytes";

describe("formatBytes", () => {
  it("uses binary units, matching what the machine's own tools report", () => {
    expect(formatBytes(1024)).toBe("1.0 KB");
    expect(formatBytes(1024 ** 3)).toBe("1.0 GB");
    expect(formatBytes(16 * 1024 ** 3)).toBe("16.0 GB");
  });

  it("does not put a decimal on raw bytes", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(0)).toBe("0 B");
  });

  // A reading that never arrived must not render as a number.
  it("says nothing for a value it cannot format", () => {
    expect(formatBytes(Number.NaN)).toBe("—");
    expect(formatBytes(-1)).toBe("—");
  });
});

describe("formatBytesOf", () => {
  // The point of the pair: one unit for both halves, so they can be
  // compared without converting.
  it("scales both halves to the total's unit", () => {
    expect(formatBytesOf(980 * 1024 ** 2, 16 * 1024 ** 3)).toBe("1.0 / 16.0 GB");
    expect(formatBytesOf(5 * 1024 ** 3, 16 * 1024 ** 3)).toBe("5.0 / 16.0 GB");
  });

  it("handles a used figure larger than nothing on a small total", () => {
    expect(formatBytesOf(300, 900)).toBe("300 / 900 B");
  });

  // A machine that reported no total is a machine that told us nothing.
  it("says nothing when there is no total", () => {
    expect(formatBytesOf(0, 0)).toBe("—");
    expect(formatBytesOf(5, Number.NaN)).toBe("—");
  });
});
