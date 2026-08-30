import { callCommand } from "./tauri";
import type { ContainerSummary, ServiceSummary } from "@/types/serverEvent";

export function listServerServices(serverId: string): Promise<ServiceSummary[]> {
  return callCommand<ServiceSummary[]>("list_server_services", { serverId });
}

export function restartServerService(serverId: string, serviceName: string): Promise<void> {
  return callCommand<void>("restart_server_service", { serverId, serviceName });
}

export function startServerService(serverId: string, serviceName: string): Promise<void> {
  return callCommand<void>("start_server_service", { serverId, serviceName });
}

export function stopServerService(serverId: string, serviceName: string): Promise<void> {
  return callCommand<void>("stop_server_service", { serverId, serviceName });
}

export function enableServerService(serverId: string, serviceName: string): Promise<void> {
  return callCommand<void>("enable_server_service", { serverId, serviceName });
}

export function disableServerService(serverId: string, serviceName: string): Promise<void> {
  return callCommand<void>("disable_server_service", { serverId, serviceName });
}

export function listServerContainers(serverId: string): Promise<ContainerSummary[]> {
  return callCommand<ContainerSummary[]>("list_server_containers", { serverId });
}

export function restartServerContainer(serverId: string, container: string): Promise<void> {
  return callCommand<void>("restart_server_container", { serverId, container });
}

export function startServerContainer(serverId: string, container: string): Promise<void> {
  return callCommand<void>("start_server_container", { serverId, container });
}

export function stopServerContainer(serverId: string, container: string): Promise<void> {
  return callCommand<void>("stop_server_container", { serverId, container });
}

export function removeServerContainer(serverId: string, container: string): Promise<void> {
  return callCommand<void>("remove_server_container", { serverId, container });
}

export function getServerContainerLogs(serverId: string, container: string, tail: number): Promise<string> {
  return callCommand<string>("get_server_container_logs", { serverId, container, tail });
}
