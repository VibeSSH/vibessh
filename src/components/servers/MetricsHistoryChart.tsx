import "./MetricsHistoryChart.css";

interface MetricsHistoryChartProps {
  label: string;
  values: number[];
  formatValue: (value: number) => string;
  /** A floor for the chart's scale (e.g. 100 for a percent series) - real values above it still stretch the scale, this just stops a near-empty series from looking like a wall. */
  minScale?: number;
}

const VIEW_WIDTH = 100;
const VIEW_HEIGHT = 32;

/**
 * A rolling-window sparkline built from whatever samples the caller has
 * collected so far (see Monitor.tsx's poll loop) - plain SVG, no charting
 * library, since a few dozen points scaled into a fixed viewBox is all this
 * needs. Deliberately not reused for MetricsPreview's gauges: those show one
 * live instantaneous value and are also used by Agent-mode's push-fed
 * preview, which has no local sample history to chart.
 */
export function MetricsHistoryChart({ label, values, formatValue, minScale = 0 }: MetricsHistoryChartProps) {
  const max = Math.max(minScale, ...values, 1);
  const current = values.length > 0 ? values[values.length - 1] : 0;

  const points = values.map((value, index) => {
    const x = values.length > 1 ? (index / (values.length - 1)) * VIEW_WIDTH : VIEW_WIDTH;
    const y = VIEW_HEIGHT - (Math.max(0, value) / max) * VIEW_HEIGHT;
    return `${x.toFixed(2)},${y.toFixed(2)}`;
  });
  const line = points.join(" ");
  const area = points.length > 0 ? `0,${VIEW_HEIGHT} ${line} ${VIEW_WIDTH},${VIEW_HEIGHT}` : "";

  return (
    <div className="metrics-history-chart">
      <div className="metrics-history-chart-header">
        <span>{label}</span>
        <span className="metrics-history-chart-current">{values.length > 0 ? formatValue(current) : "—"}</span>
      </div>
      <svg viewBox={`0 0 ${VIEW_WIDTH} ${VIEW_HEIGHT}`} preserveAspectRatio="none" className="metrics-history-chart-svg">
        {area && <polygon points={area} className="metrics-history-chart-area" />}
        {points.length > 1 && <polyline points={line} className="metrics-history-chart-line" />}
      </svg>
    </div>
  );
}
