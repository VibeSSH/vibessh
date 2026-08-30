import { callCommand } from "./tauri";
import type { ContainerSummary, ServiceSummary } from "@/types/serverEvent";

export function listServerServices(serverId: string): Promise<ServiceSummary[]> {
  return callCommand<ServiceSummary[]>("list_server_services", { serverId });
}

export function restartServerService(serverId: string, serviceName: string): Promise<void> {
  return callCommand<void>("restart_server_service", { serverId, serviceName });
}

export function listServerContainers(serverId: string): Promise<ContainerSummary[]> {
  return callCommand<ContainerSummary[]>("list_server_containers", { serverId });
}

export function restartServerContainer(serverId: string, container: string): Promise<void> {
  return callCommand<void>("restart_server_container", { serverId, container });
}
