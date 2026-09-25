import { callCommand } from "./tauri";
import { clearRejectedPromptDismissal } from "@/stores/sessionPasswordStore";
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

/** Converts an already-known SSH-mode server to Agent mode in place - same id, so every Application/DNS alias/Firewall membership already pointing at this server keeps working. Used by the Setup Page's own "also install the Vibe Agent" step, so pairing an Agent for a Node you already added over SSH upgrades that same entry instead of creating a confusing second one. */
export function upgradeServerToAgent(serverId: string, agentId: string, dockerCapable?: boolean): Promise<ServerSummary> {
  return callCommand<ServerSummary>("upgrade_server_to_agent", { serverId, agentId, dockerCapable });
}

/** A real SSH-exec probe (`command -v docker`), not a guess - see the Rust `probe_node_capabilities` doc comment. SSH-mode only; an agent-mode server's capabilities come from its own handshake instead. */
export function probeServerCapabilities(id: string): Promise<NodeCapabilities> {
  return callCommand<NodeCapabilities>("probe_server_capabilities", { id });
}

/** Runs Docker's own official install script on this Node over SSH - see the Rust `install_docker` doc comment. Can take a while (real package installation, not a probe), and returns the freshly re-verified capabilities on success. */
export function installDocker(id: string): Promise<NodeCapabilities> {
  return callCommand<NodeCapabilities>("install_docker", { id });
}

/** Installs WireGuard on this Node over SSH (a no-op if it's already there) - see the Rust `install_wireguard` doc comment. Doesn't join the Vibe Network by itself, use `joinVibeNetwork` for that. */
export function installWireguard(id: string): Promise<NodeCapabilities> {
  return callCommand<NodeCapabilities>("install_wireguard", { id });
}

/** Installs ufw on this Node over SSH - see the Rust `install_ufw` doc comment. Never enables enforcement itself, use `enableServerFirewall` for that. */
export function installUfw(id: string): Promise<NodeCapabilities> {
  return callCommand<NodeCapabilities>("install_ufw", { id });
}

/** Mirrors the Rust `FirewallRule` DTO. */
export interface FirewallRule {
  port: number;
  protocol: "tcp" | "udp";
  sourceCidr?: string;
}

/** Mirrors the Rust `FirewallSyncResult` DTO. `backend: null` means no supported firewall was detected on this Node - not a failure. `rulesRemoved` counts rules this same sync just revoked (see the Rust `firewall` module's own doc comment). `unenforced` is `true` when nothing is actually restricting these ports - either no backend at all, or one that's installed but switched off. It must be surfaced as a warning: a sync that reports success while leaving ports open is exactly what made "Vibe Network only" ports publicly reachable. */
export interface FirewallSyncResult {
  backend: string | null;
  active: boolean;
  rulesApplied: number;
  rulesRemoved: number;
  unenforced: boolean;
}

/** A local-only read, no SSH round trip - what "Secure this server"'s confirmation dialog shows before anything actually changes (the SSH port is always first). See the Rust `preview_server_firewall_rules` doc comment. */
export function previewServerFirewallRules(id: string): Promise<FirewallRule[]> {
  return callCommand<FirewallRule[]>("preview_server_firewall_rules", { id });
}

/** The actual, explicit action that turns firewall enforcement on for this Node - applies every rule `previewServerFirewallRules` already showed (SSH port included, always first) and only then enables enforcement, so this can't lock the connecting user out. See the Rust `enable_server_firewall` doc comment. */
export function enableServerFirewall(id: string): Promise<FirewallSyncResult> {
  return callCommand<FirewallSyncResult>("enable_server_firewall", { id });
}

/** Mirrors the Rust `FirewallRuleOrigin` enum - which rule "owns" a `FirewallRuleView`, so the Firewall page can explain *why* a rule exists instead of showing a bare port list, and only offer a delete button for a `"custom"` one. */
export type FirewallRuleOrigin =
  | { kind: "ssh" }
  | { kind: "wireGuard" }
  | { kind: "application"; applicationId: string; applicationName: string; portName: string }
  | { kind: "custom"; ruleId: string; label?: string };

/** Mirrors the Rust `FirewallRuleView` DTO (`FirewallRule` flattened + `origin`). */
export interface FirewallRuleView extends FirewallRule {
  origin: FirewallRuleOrigin;
}

/** Mirrors the Rust `NodeFirewallOverview` DTO - everything the Firewall page renders in one call. `backend`/`active` read as `null`/`false` when the Node can't currently be reached; `rules` is still the real desired set either way (a pure DB read). */
export interface NodeFirewallOverview {
  backend: string | null;
  active: boolean;
  rules: FirewallRuleView[];
  container: ContainerFirewallState;
}

/**
 * Mirrors the Rust `ContainerFirewallState` - what the Node's `DOCKER-USER`
 * chain is really carrying, read back from the Node.
 *
 * A published Docker port goes around ufw, so for those the ufw rule list
 * says nothing about whether anything restricts them. `applicable: false`
 * means this Node has no Docker and ufw's answer stands alone; an `error`
 * means the chain could not be read, which is "unknown" and must never be
 * reported as protected.
 */
export interface ContainerFirewallState {
  applicable: boolean;
  restrictedPorts: number[];
  error: string | null;
}

export function getNodeFirewallOverview(id: string): Promise<NodeFirewallOverview> {
  return callCommand<NodeFirewallOverview>("get_node_firewall_overview", { id });
}

/** "Sync now" - re-applies every desired rule and revokes whatever's obsolete, without touching enforcement. See the Rust `sync_node_firewall` doc comment. */
export function syncNodeFirewall(id: string): Promise<FirewallSyncResult> {
  return callCommand<FirewallSyncResult>("sync_node_firewall", { id });
}

/** Mirrors the Rust `FirewallCustomRule` DTO - a manually declared rule not tied to any Application's own port. */
export interface FirewallCustomRule {
  id: string;
  serverId: string;
  label?: string;
  protocol: "tcp" | "udp";
  port: number;
  sourceCidr?: string;
  createdAt: string;
}

/** What add-custom-rule submits - mirrors the Rust `FirewallCustomRuleInput` DTO. */
export interface FirewallCustomRuleInput {
  label?: string;
  protocol: "tcp" | "udp";
  port: number;
  sourceCidr?: string;
}

/** Persists the rule then best-effort applies it live right away - see the Rust `add_custom_firewall_rule` doc comment. */
export function addFirewallCustomRule(id: string, input: FirewallCustomRuleInput): Promise<FirewallCustomRule> {
  return callCommand<FirewallCustomRule>("add_firewall_custom_rule", { id, input });
}

/** Deletes the rule then best-effort revokes it live - see the Rust `remove_custom_firewall_rule` doc comment. */
export function removeFirewallCustomRule(id: string, ruleId: string): Promise<void> {
  return callCommand<void>("remove_firewall_custom_rule", { id, ruleId });
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

/** Sets or clears a node's icon. `null` clears it.
 *
 * The value must be a base64 PNG data URL. `ServerIconPicker` produces one by
 * drawing the picked file onto a canvas and exporting PNG - which is what
 * makes it safe to render, since an SVG in an `<img src>` can carry script -
 * and the backend re-checks the format rather than trusting this. */
export function setServerIcon(id: string, icon: string | null): Promise<ServerSummary> {
  return callCommand<ServerSummary>("set_server_icon", { id, icon });
}

export async function updateServer(id: string, input: ServerFormInput): Promise<ServerSummary> {
  const updated = await callCommand<ServerSummary>("update_server", { id, input });
  // Editing a server is where refused credentials get fixed, so a dismissed
  // "wrong password" prompt may come back for it after this.
  clearRejectedPromptDismissal(id);
  return updated;
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
  // A spread, not a field-by-field copy, and that is the fix for a real bug
  // rather than a style preference. The explicit version listed twelve fields
  // and silently dropped every one added afterwards: node icons vanished from
  // the rail and the cards the moment any page re-fetched the list, because
  // `icon` was not in the list. Nothing failed - the field simply was not
  // carried, which no type error can catch when the target's field is
  // optional.
  //
  // `status` is the one value that is genuinely not the backend's to give:
  // it is live connectivity, tracked in the store and refreshed by pinging,
  // so a freshly loaded row starts as "unknown" rather than inheriting
  // whatever the last render believed.
  return { ...server, status: "unknown" };
}
