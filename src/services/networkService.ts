import { callCommand } from "./tauri";
import type {
  DnsRecord,
  DnsSyncResult,
  DnsView,
  MeshReconcileResult,
  NodeEndpoint,
  NodeMeshStatus,
  NodeNetworkMember,
  VibeNetworkSyncResult,
} from "@/types/network";

// ---- Vibe Network membership ----

export function listNetworkMembers(): Promise<NodeNetworkMember[]> {
  return callCommand<NodeNetworkMember[]>("list_network_members");
}

/** "Dołącz do Vibe Network" - VibeSSH generates the keypair, allocates the private IP, and syncs every peer's config on both sides. The user never enters a CIDR, a peer, or a WireGuard setting by hand. */
export function joinVibeNetwork(serverId: string): Promise<NodeNetworkMember> {
  return callCommand<NodeNetworkMember>("join_vibe_network", { serverId });
}

export function leaveVibeNetwork(serverId: string): Promise<void> {
  return callCommand<void>("leave_vibe_network", { serverId });
}

export function reconcileVibeMesh(): Promise<MeshReconcileResult[]> {
  return callCommand<MeshReconcileResult[]>("reconcile_vibe_mesh");
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

export function createDnsAlias(applicationId: string, hostname: string): Promise<DnsRecord> {
  return callCommand<DnsRecord>("create_dns_alias", { applicationId, hostname });
}

export function updateDnsAlias(id: string, hostname: string): Promise<DnsRecord> {
  return callCommand<DnsRecord>("update_dns_alias", { id, hostname });
}

export function deleteDnsAlias(id: string): Promise<void> {
  return callCommand<void>("delete_dns_alias", { id });
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
