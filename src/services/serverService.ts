import { callCommand } from "./tauri";
import type { ManagedServer } from "@/stores/serversStore";
import type { AuthenticationType, ServerSummary } from "@/types/server";

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

/** Persists an agent-paired server to the local SQLite store, same as SSH-mode servers already were - previously these only ever lived in the session-only Zustand store and vanished on app restart. Upserts by agent id, so re-pairing an already-known agent updates its existing row instead of creating a duplicate. */
export function upsertAgentServer(name: string, host: string, agentId: string): Promise<ServerSummary> {
  return callCommand<ServerSummary>("upsert_agent_server", { name, host, agentId });
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
    createdAt: server.createdAt,
  };
}
