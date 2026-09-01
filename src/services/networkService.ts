import { callCommand } from "./tauri";
import type {
  DnsAliasWithSync,
  DnsRecord,
  DnsSyncResult,
  DnsView,
  NodeEndpoint,
  NodeMeshStatus,
  NodeNetworkMember,
  VibeNetworkSyncResult,
} from "@/types/network";

// ---- Vibe Network membership ----

export function listNetworkMembers(): Promise<NodeNetworkMember[]> {
  return callCommand<NodeNetworkMember[]>("list_network_members");
}

/** "Dołącz do Vibe Network" - VibeSSH generates the keypair, allocates the private IP, syncs every peer's config on both sides, and best-effort pushes DNS so the new Node's own alias (and every existing service alias) resolves right away. The user never enters a CIDR, a peer, or a WireGuard setting by hand. */
export function joinVibeNetwork(serverId: string): Promise<NodeNetworkMember> {
  return callCommand<NodeNetworkMember>("join_vibe_network", { serverId });
}

export function leaveVibeNetwork(serverId: string): Promise<void> {
  return callCommand<void>("leave_vibe_network", { serverId });
}

/** Real, current state - each Node's own `wg show` output, cross-referenced back to whichever mesh member each peer is. */
export function getVibeNetworkStatus(): Promise<NodeMeshStatus[]> {
  return callCommand<NodeMeshStatus[]>("get_vibe_network_status");
}

// ---- Endpoints (a Node-scoped view over existing Application ports) ----

export function listNodeEndpoints(serverId: string): Promise<NodeEndpoint[]> {
  return callCommand<NodeEndpoint[]>("list_node_endpoints", { serverId });
}

// ---- Private DNS ----

export function listDnsRecords(): Promise<DnsRecord[]> {
  return callCommand<DnsRecord[]>("list_dns_records");
}

/** Saving an alias now automatically pushes it to every mesh member right away - `syncResults` on the return value is that push's real per-Node outcome, not just an assumption it worked. */
export function createDnsAlias(applicationId: string, hostname: string): Promise<DnsAliasWithSync> {
  return callCommand<DnsAliasWithSync>("create_dns_alias", { applicationId, hostname });
}

export function updateDnsAlias(id: string, hostname: string): Promise<DnsAliasWithSync> {
  return callCommand<DnsAliasWithSync>("update_dns_alias", { id, hostname });
}

export function deleteDnsAlias(id: string): Promise<DnsSyncResult[]> {
  return callCommand<DnsSyncResult[]>("delete_dns_alias", { id });
}

export function syncVibeDns(): Promise<DnsSyncResult[]> {
  return callCommand<DnsSyncResult[]>("sync_vibe_dns");
}

export function verifyDnsAlias(hostname: string, expectedIp: string): Promise<boolean> {
  return callCommand<boolean>("verify_dns_alias", { hostname, expectedIp });
}

/** Every live alias (Node + service), each already resolved to a real Vibe Network IP - the source for both the DNS list view and each Node's own "DNS name" shown on the Vibe Network page. */
export function resolveDnsView(): Promise<DnsView[]> {
  return callCommand<DnsView[]>("resolve_dns_view");
}

// ---- Combined sync ----

/** "Synchronizuj Vibe Network" - WireGuard peers + firewall + DNS, in one action, with a real per-Node OK/OUT OF SYNC result. */
export function syncVibeNetwork(): Promise<VibeNetworkSyncResult[]> {
  return callCommand<VibeNetworkSyncResult[]>("sync_vibe_network");
}

// ---- DNS suffix (configurable, per install) ----

export function getDnsSuffix(): Promise<string> {
  return callCommand<string>("get_dns_suffix");
}

/** Only affects aliases created from now on - existing DNS records keep whatever suffix they already had. */
export function setDnsSuffix(suffix: string): Promise<string> {
  return callCommand<string>("set_dns_suffix", { suffix });
}
