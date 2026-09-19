import { callCommand } from "./tauri";
import type { ProcessSummary, ServerMetrics } from "@/types/serverEvent";

export function getServerMetrics(serverId: string): Promise<ServerMetrics> {
  return callCommand<ServerMetrics>("get_server_metrics", { serverId });
}

export function listServerProcesses(serverId: string): Promise<ProcessSummary[]> {
  return callCommand<ProcessSummary[]>("list_server_processes", { serverId });
}
