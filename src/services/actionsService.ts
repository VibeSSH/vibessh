import { callCommand } from "./tauri";
import type { ServiceSummary } from "@/types/serverEvent";

export function listServerServices(serverId: string): Promise<ServiceSummary[]> {
  return callCommand<ServiceSummary[]>("list_server_services", { serverId });
}

export function restartServerService(serverId: string, serviceName: string): Promise<void> {
  return callCommand<void>("restart_server_service", { serverId, serviceName });
}
