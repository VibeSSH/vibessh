import { callCommand } from "./tauri";
import type { ManagedServer } from "@/stores/serversStore";
import type { AuthenticationType, NodeCapabilities, ServerSummary } from "@/types/server";

/** Mirrors the Rust `ServerInput` DTO - what create/update submit. */
export interface ServerFormInput {
  name: string;
  host: string;
  sshPort: number;
  username: string;
  authenticationType: AuthenticationType;
  privateKeyPath?: string;
  groupId?: string;
  /** Required on create for password auth; on update, blank means "keep the existing password". */
  password?: string;
  /** Same keep-existing-when-blank rule as password, but always optional. */
  keyPassphrase?: string;
}

export function listServers(): Promise<ServerSummary[]> {
  return callCommand<ServerSummary[]>("list_servers");
}

/** Persists an agent-paired server to the local SQLite store, same as SSH-mode servers already were - previously these only ever lived in the session-only Zustand store and vanished on app restart. Upserts by agent id, so re-pairing an already-known agent updates its existing row instead of creating a duplicate. `dockerCapable` (Etap M1) is the fresh handshake reading, when there is one - passed straight from `AgentConnectionState.capabilities.docker`, persisted alongside the rest of the row. */
export function upsertAgentServer(name: string, host: string, agentId: string, dockerCapable?: boolean): Promise<ServerSummary> {
  return callCommand<ServerSummary>("upsert_agent_server", { name, host, agentId, dockerCapable });
}

/** A real SSH-exec probe (`command -v docker`), not a guess - see the Rust `probe_node_capabilities` doc comment. SSH-mode only; an agent-mode server's capabilities come from its own handshake instead. */
export function probeServerCapabilities(id: string): Promise<NodeCapabilities> {
  return callCommand<NodeCapabilities>("probe_server_capabilities", { id });
}

/** Starts (or confirms already-running) the persistent Etap M3 connection for a just-paired Node - called right after `upsertAgentServer` succeeds, while host/port/the freshly issued credential are all still in hand. See the Rust `AgentSessionManager`'s own doc comment for why this only covers "stays connected for the running app session," not reconnecting after an app restart. */
export function startAgentSession(serverId: string, host: string, port: number, authToken: string): Promise<void> {
  return callCommand<void>("start_agent_session", { serverId, host, port, authToken });
}

/** Mirrors the Rust `NodeSyncStatus` DTO. */
export interface NodeSyncStatus {
  serverId: string;
  desiredRevision: number;
  appliedRevision: number;
  inSync: boolean;
}

export function getNodeSyncStatus(serverId: string): Promise<NodeSyncStatus> {
  return callCommand<NodeSyncStatus>("get_node_sync_status", { serverId });
}

/** Mirrors the Rust `ReconcileOutcome` enum's JSON shape (serde `tag = "status"`). Never a plain boolean - a Node that couldn't be reached is a distinct case from one that was reached but failed, see the Rust type's own doc comment. */
export type ReconcileOutcome =
  | { status: "applied"; revision: number }
  | { status: "offlinePending"; desiredRevision: number }
  | { status: "failed"; revision: number; error: string | null };

/** "Reconcile" (Etap M3, Agent-mode Nodes only) - bumps the desired revision and pushes it, waiting for a real acknowledgement rather than assuming success. */
export function reconcileAgentNode(serverId: string): Promise<ReconcileOutcome> {
  return callCommand<ReconcileOutcome>("reconcile_agent_node", { serverId });
}

export function createServer(input: ServerFormInput): Promise<ServerSummary> {
  return callCommand<ServerSummary>("create_server", { input });
}

export function updateServer(id: string, input: ServerFormInput): Promise<ServerSummary> {
  return callCommand<ServerSummary>("update_server", { id, input });
}

export function deleteServer(id: string): Promise<void> {
  return callCommand<void>("delete_server", { id });
}

/** Connects with whatever's in `input` directly, no save - for the "Test connection" button. */
export function testSshConnection(input: ServerFormInput): Promise<void> {
  return callCommand<void>("test_ssh_connection", { input });
}

/** A TCP-connect-timing reachability check against the server's SSH port - resolves to the round trip in ms, rejects if unreachable. No auth involved. */
export function pingServer(id: string): Promise<number> {
  return callCommand<number>("ping_server", { id });
}

/** A freshly loaded server starts "unknown" until the first ping resolves - see useServerPinging, which then calls updateStatus with a real online/offline reading. */
export function serverSummaryToManagedServer(server: ServerSummary): ManagedServer {
  return {
    id: server.id,
    name: server.name,
    host: server.host,
    connectionMode: server.connectionMode,
    status: "unknown",
    agentId: server.agentId,
    sshPort: server.sshPort,
    username: server.username,
    authenticationType: server.authenticationType,
    privateKeyPath: server.privateKeyPath,
    nodeCapabilities: server.nodeCapabilities,
    createdAt: server.createdAt,
  };
}
