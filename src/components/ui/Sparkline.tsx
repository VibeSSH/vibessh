import "./Sparkline.css";

/**
 * The chart's internal coordinate space.
 *
 * Fixed, with `preserveAspectRatio="none"` on the SVG, so the same path
 * stretches to whatever width the card gives it. The alternative -
 * measuring the element and recomputing on resize - buys nothing here: a
 * sparkline has no labels or ticks whose proportions would be distorted by
 * stretching.
 */
const VIEW_WIDTH = 100;
const VIEW_HEIGHT = 32;

/**
 * Headroom above the highest sample, so the peak is not drawn touching the
 * top edge where it reads as clipped.
 */
const HEADROOM = 1.15;

/**
 * Builds the polyline through a series.
 *
 * **The y axis starts at zero, not at the minimum.** Scaling to the
 * observed range makes a processor sitting flat at 4.5% look like it is
 * thrashing between extremes - the noise fills the card because the card is
 * all there is to fill. For a resource meter the distance from zero is the
 * information, so a quiet series has to *look* quiet.
 *
 * Exported for its own tests: it is the only part of this component with
 * arithmetic worth being wrong.
 */
export function sparklinePath(values: number[], max: number): string {
  if (values.length === 0) return "";
  // A single sample has no line to draw, so it becomes a flat one across
  // the whole width - honest about the value, and it stops the chart
  // flickering into existence one pixel at a time as the first samples
  // arrive.
  const points = values.length === 1 ? [values[0], values[0]] : values;
  const step = VIEW_WIDTH / (points.length - 1);
  // A series that is entirely zero would divide by zero; drawn flat along
  // the bottom instead, which is what it means.
  const scale = max > 0 ? max : 1;

  return points
    .map((value, index) => {
      const x = index * step;
      const y = VIEW_HEIGHT - Math.min(value / scale, 1) * VIEW_HEIGHT;
      return `${index === 0 ? "M" : "L"}${x.toFixed(2)},${y.toFixed(2)}`;
    })
    .join(" ");
}

interface SparklineProps {
  values: number[];
  /** What the top of the chart represents. Falls back to the highest
   * sample plus headroom, which is right for an unbounded series like
   * memory; pass 100 for a percentage so the height means the same thing
   * from one glance to the next. */
  max?: number;
  /** Read out instead of the shape, which conveys nothing to a screen
   * reader. */
  label: string;
}

export function Sparkline({ values, max, label }: SparklineProps) {
  const ceiling = max ?? Math.max(...values, 0) * HEADROOM;
  const path = sparklinePath(values, ceiling);

  if (path === "") {
    return <p className="sparkline-empty">{label}</p>;
  }

  return (
    <svg className="sparkline" viewBox={`0 0 ${VIEW_WIDTH} ${VIEW_HEIGHT}`} preserveAspectRatio="none" role="img" aria-label={label}>
      {/* The fill is the same path closed along the bottom. Drawn first so
          the line sits on top of it rather than being half-covered. */}
      <path className="sparkline-area" d={`${path} L${VIEW_WIDTH},${VIEW_HEIGHT} L0,${VIEW_HEIGHT} Z`} />
      <path className="sparkline-line" d={path} />
    </svg>
  );
}
