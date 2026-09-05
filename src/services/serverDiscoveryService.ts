import { callCommand } from "./tauri";

/** Mirrors the Rust `DiscoveredServerKind`. */
export type DiscoveredServerKind = "paper" | "purpur" | "velocity" | "waterfall" | "spigot" | "fabric" | "forge" | "unknown";

/** Mirrors the Rust `DiscoveredServer` DTO. */
export interface DiscoveredServer {
  name: string;
  path: string;
  jar: string;
  /** Absent for a server with no `server.properties` - a proxy, typically. */
  port: number | null;
  kind: DiscoveredServerKind;
}

/**
 * Looks for game servers already sitting in a directory.
 *
 * `serverId` absent means this machine, the same meaning it carries
 * everywhere else. Reads only.
 */
export function scanForServers(serverId: string | null, directory: string): Promise<DiscoveredServer[]> {
  return callCommand<DiscoveredServer[]>("scan_for_servers", { serverId, directory });
}
