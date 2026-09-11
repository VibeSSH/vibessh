/** Mirrors the Rust `AgentCapabilities` struct. `true` means the host
 * supports it - not that VibeSSH has a feature built to use it yet. */
export interface AgentCapabilities {
  docker: boolean;
  systemd: boolean;
  minecraft: boolean;
  fileAccess: boolean;
  terminal: boolean;
}

/** Mirrors the Rust `AgentConnectionState` enum's JSON shape (serde `tag = "status"`). */
export type AgentConnectionState =
  | { status: "connecting" }
  | {
      status: "connected";
      agentId: string;
      agentVersion: string;
      issuedCredential: string | null;
      capabilities: AgentCapabilities;
    }
  | { status: "disconnected"; reason: string };
