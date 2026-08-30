/** Mirrors the Rust `ServerMetrics` struct. */
export interface ServerMetrics {
  cpuUsagePercent: number;
  ramUsedBytes: number;
  ramTotalBytes: number;
  diskUsedBytes: number;
  diskTotalBytes: number;
  loadAverage1m: number;
  uptimeSeconds: number;
  networkRxBytesPerSec: number;
  networkTxBytesPerSec: number;
}

/** Mirrors the Rust `ProcessSummary` struct. */
export interface ProcessSummary {
  pid: number;
  user: string;
  cpuPercent: number;
  ramBytes: number;
  command: string;
}

/**
 * Mirrors the Rust `ServerEvent` enum's JSON shape (serde `tag = "type"`,
 * dot-notation variant names). Only `metrics.update` (Etap J) is ever
 * actually sent by the agent today - the rest exist in the protocol
 * already, waiting on the features that will emit them.
 */
export type ServerEvent =
  | { type: "metrics.update"; metrics: ServerMetrics }
  | { type: "heartbeat" }
  | { type: "error"; code: string; message: string };
