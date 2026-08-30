export type AuthenticationType = "password" | "privateKey";

/** Mirrors the Rust `ConnectionMode` enum — how the app reaches this server. */
export type ConnectionMode = "ssh" | "agent";

/** Mirrors the Rust `AgentStatus` enum. Only meaningful when connectionMode is "agent". */
export type AgentStatus = "pairing" | "connected" | "disconnected" | "incompatible";

export interface ServerGroup {
  id: string;
  name: string;
  color?: string;
}

export interface ServerSummary {
  id: string;
  name: string;
  host: string;
  sshPort: number;
  username: string;
  authenticationType: AuthenticationType;
  connectionMode: ConnectionMode;
  agentId?: string;
  agentStatus?: AgentStatus;
  groupId?: string;
  createdAt: string;
  updatedAt: string;
}

export type ServerConnectionStatus = "unknown" | "online" | "offline" | "connecting";
