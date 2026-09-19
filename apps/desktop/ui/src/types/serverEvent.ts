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
  /** What the Node calls itself - `PRETTY_NAME` from `/etc/os-release`.
   * Null when the file is absent (some container images), when the
   * distribution omits the field, or when an older Agent is reporting. */
  osName: string | null;
}

/** Mirrors the Rust `MinecraftMetrics` struct. The TPS/MSPT fields are null
 * on a server that has no `/tps` command (not Paper/Purpur); an honest gap,
 * never a guessed 20. */
export interface MinecraftMetrics {
  tps1m: number | null;
  tps5m: number | null;
  tps15m: number | null;
  msptAvg: number | null;
  msptMax: number | null;
  playersOnline: number;
  playersMax: number;
  playerNames: string[];
}

/** Mirrors the Rust `ProcessSummary` struct. */
export interface ProcessSummary {
  pid: number;
  user: string;
  cpuPercent: number;
  ramBytes: number;
  command: string;
}

/** Mirrors the Rust `ServiceSummary` struct. */
export interface ServiceSummary {
  name: string;
  active: boolean;
  enabled: boolean;
  description: string;
}

/** Mirrors the Rust `ContainerSummary` struct. */
export interface ContainerSummary {
  id: string;
  name: string;
  image: string;
  status: string;
  running: boolean;
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
