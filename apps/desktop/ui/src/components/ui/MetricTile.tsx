import type { ReactNode } from "react";
import { Sparkline } from "@/components/ui/Sparkline";
import "./MetricTile.css";

export type MetricTone = "ok" | "warn" | "bad";

interface MetricTileProps {
  label: string;
  value: ReactNode;
  unit?: string;
  /** One line of context under the value. Always takes its row, empty or not,
   *  so tiles side by side keep their charts at the same height. */
  sub?: ReactNode;
  /** Colours the value - kept for a reading under strain, not for decoration. */
  tone?: MetricTone;
  /** A session chart under the value. `max` fixes the scale where the
   *  reading has a natural ceiling (TPS 20, a tick's 50 ms, 100%). */
  spark?: { values: number[]; max?: number; label: string };
}

/**
 * One live reading - label, value, context, chart - in the same shape on every
 * screen that shows one: the Application overview and the Minecraft tab.
 */
export function MetricTile({ label, value, unit, sub, tone, spark }: MetricTileProps) {
  return (
    <div className="metric-tile">
      <span className="metric-tile-label">{label}</span>
      <span className={tone ? `metric-tile-value metric-tile-${tone}` : "metric-tile-value"}>
        {value}
        {unit && <span className="metric-tile-unit">{unit}</span>}
      </span>
      <span className="metric-tile-sub">{sub ?? " "}</span>
      {spark && (
        <div className="metric-tile-spark">
          <Sparkline values={spark.values} max={spark.max} label={spark.label} />
        </div>
      )}
    </div>
  );
}

/** Tiles laid out to share a row, wrapping on a narrow window. */
export function MetricTileGrid({ children }: { children: ReactNode }) {
  return <div className="metric-tile-grid">{children}</div>;
}
