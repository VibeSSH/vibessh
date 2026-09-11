import { describe, expect, it } from "vitest";
import { sparklinePath } from "./Sparkline";

/**
 * The arithmetic, which is the only part of a sparkline that can be wrong
 * in a way nobody notices - a chart that is subtly mis-scaled still looks
 * like a chart.
 */
describe("sparklinePath", () => {
  it("has nothing to draw for an empty series", () => {
    expect(sparklinePath([], 100)).toBe("");
  });

  /** One sample is drawn flat across the width rather than as a dot, so the
   * chart does not flicker into existence as the first samples arrive. */
  it("draws a single sample as a flat line spanning the width", () => {
    const path = sparklinePath([50], 100);
    expect(path.startsWith("M0.00,16.00")).toBe(true);
    expect(path.endsWith("L100.00,16.00")).toBe(true);
  });

  /** The y axis starts at zero. A flat 4.5% series must sit near the
   * bottom, not fill the card - scaling to the observed range would make a
   * quiet processor look like it was thrashing. */
  it("keeps a quiet series near the bottom", () => {
    const path = sparklinePath([4.5, 4.6, 4.4, 4.5], 100);
    const ys = [...path.matchAll(/,([\d.]+)/g)].map((m) => Number(m[1]));
    // Height is 32, so a 4.5% reading sits around y = 30.6.
    expect(Math.min(...ys)).toBeGreaterThan(29);
  });

  it("puts a full-scale sample at the top and a zero at the bottom", () => {
    const path = sparklinePath([0, 100], 100);
    expect(path).toBe("M0.00,32.00 L100.00,0.00");
  });

  /** An all-zero series would divide by zero if the ceiling were taken from
   * the data. */
  it("survives a series with no magnitude at all", () => {
    expect(sparklinePath([0, 0, 0], 0)).toBe("M0.00,32.00 L50.00,32.00 L100.00,32.00");
  });

  /** A sample above the declared ceiling is clamped rather than drawn
   * outside the box - CPU percentages routinely exceed 100 on a
   * multi-core container. */
  it("clamps a sample above the ceiling", () => {
    const path = sparklinePath([250], 100);
    expect(path.includes(",0.00")).toBe(true);
  });
});
