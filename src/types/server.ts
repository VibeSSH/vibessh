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

/** Mirrors the Rust `NodeCapabilities` struct. `wireguard`/`ufw` are only ever real (SSH-probed) values for an SSH-mode Node - an Agent-mode Node's handshake doesn't report them, so they read as `false` there regardless of actual state. */
export interface NodeCapabilities {
  docker: boolean;
  wireguard: boolean;
  ufw: boolean;
}

export interface ServerSummary {
  id: string;
  name: string;
  host: string;
  sshPort: number;
  username: string;
  authenticationType: AuthenticationType;
  /** Only meaningful when authenticationType is "privateKey" - a path, never key content. */
  privateKeyPath?: string;
  connectionMode: ConnectionMode;
  agentId?: string;
  agentStatus?: AgentStatus;
  groupId?: string;
  /** `undefined` means "never probed", not "no capabilities" - see the Rust `Server::node_capabilities` doc comment. */
  nodeCapabilities?: NodeCapabilities;
  createdAt: string;
  updatedAt: string;
}

export type ServerConnectionStatus = "unknown" | "online" | "offline" | "connecting";
