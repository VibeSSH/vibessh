import { callCommand } from "./tauri";
import type { MinecraftMetrics, ProcessSummary, ServerMetrics } from "@/types/serverEvent";

export function getServerMetrics(serverId: string): Promise<ServerMetrics> {
  return callCommand<ServerMetrics>("get_server_metrics", { serverId });
}

export function listServerProcesses(serverId: string): Promise<ProcessSummary[]> {
  return callCommand<ProcessSummary[]>("list_server_processes", { serverId });
}

/**
 * A Minecraft server's own health - TPS, tick time, players - over RCON
 * through the SSH tunnel. The port is not a secret and is passed here; the
 * password is read from the keyring on the Rust side, never sent from here.
 */
export function getMinecraftMetrics(applicationId: string, rconPort: number): Promise<MinecraftMetrics> {
  return callCommand<MinecraftMetrics>("get_minecraft_metrics", { applicationId, rconPort });
}

/** Stores the RCON password in the OS keyring. Sent once when the user saves
 * it; there is no read-back path. */
export function setMinecraftRconPassword(applicationId: string, password: string): Promise<void> {
  return callCommand<void>("set_minecraft_rcon_password", { applicationId, password });
}
