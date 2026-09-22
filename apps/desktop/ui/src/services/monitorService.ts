import { callCommand } from "./tauri";
import type { ProcessSummary, ServerMetrics } from "@/types/serverEvent";

export function getServerMetrics(serverId: string): Promise<ServerMetrics> {
  return callCommand<ServerMetrics>("get_server_metrics", { serverId });
}

export function listServerProcesses(serverId: string): Promise<ProcessSummary[]> {
  return callCommand<ProcessSummary[]>("list_server_processes", { serverId });
}

/** Start a live metrics stream for one SSH-mode server. The backend then emits
 *  `metrics://<serverId>` events until `stopMetricsStream` is called. */
export function startMetricsStream(serverId: string): Promise<void> {
  return callCommand<void>("start_metrics_stream", { serverId });
}

/** Stop the live metrics stream for one server. */
export function stopMetricsStream(serverId: string): Promise<void> {
  return callCommand<void>("stop_metrics_stream", { serverId });
}
