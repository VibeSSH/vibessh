/** Mirrors the Rust `AgentConnectionState` enum's JSON shape (serde `tag = "status"`). */
export type AgentConnectionState =
  | { status: "connecting" }
  | { status: "connected"; agentId: string; agentVersion: string; issuedCredential: string | null }
  | { status: "disconnected"; reason: string };
