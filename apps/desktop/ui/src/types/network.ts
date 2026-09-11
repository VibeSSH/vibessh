/** Mirrors the Rust `NodeNetworkMember` DTO (Etap M4). */
export interface NodeNetworkMember {
  serverId: string;
  wireguardIp: string;
  wireguardPublicKey: string;
  joinedAt: string;
}

/** Mirrors the Rust `PeerHandshake` DTO - one resolved peer entry from a Node's own real `wg show` output. */
export interface PeerHandshake {
  serverId: string;
  latestHandshakeUnix: number;
  rxBytes: number;
  txBytes: number;
}

/**
 * What one Node's tunnel is doing - deliberately not the same question as
 * whether the Node answered SSH. Mirrors the Rust `TunnelState`.
 */
export type TunnelState =
  /** `wg show` ran and reported the peers in `peers`. */
  | "up"
  /** The interface is not on this Node. */
  | "down"
  /** The interface could not be read; `tunnelError` says why. */
  | "unknown"
  /** The Node itself did not answer. */
  | "unreachable";

/** Mirrors the Rust `NodeMeshStatus` DTO. */
export interface NodeMeshStatus {
  serverId: string;
  reachable: boolean;
  tunnel: TunnelState;
  tunnelError: string | null;
  peers: PeerHandshake[];
  /** Peers `wg` reported whose key belongs to no known member - what a Node
   * re-keyed behind the app's back looks like. */
  unknownPeers: number;
}

/** Mirrors the Rust `NodeEndpoint` DTO - the "Endpoints" view, a Node-scoped read over existing Application ports. */
export interface NodeEndpoint {
  applicationId: string;
  applicationName: string;
  id: string;
  name: string;
  protocol: "tcp" | "udp";
  bindAddress: string;
  internalPort: number;
  externalPort?: number;
  visibility: PortVisibility;
  required: boolean;
  createdAt: string;
  updatedAt: string;
}

export type PortVisibility = "public" | "vibeNetwork" | "localhost" | "custom";

/** Mirrors the Rust `DnsRecord` DTO. */
export interface DnsRecord {
  id: string;
  applicationId: string;
  hostname: string;
  createdAt: string;
}

/** Mirrors the Rust `DnsView` DTO's JSON shape (serde `tag = "type"` on `kind`). */
export interface DnsView {
  hostname: string;
  ip: string;
  kind: { type: "node" } | { type: "service"; applicationId: string };
  serverId: string;
}

/** Mirrors the Rust `DnsSyncResult` DTO. */
export interface DnsSyncResult {
  serverId: string;
  ok: boolean;
  error: string | null;
}

/** Mirrors the Rust `DnsAliasWithSync` DTO - what `createDnsAlias`/`updateDnsAlias` return now that saving an alias automatically pushes it to every mesh member, instead of leaving the user to separately click "Synchronizuj". */
export interface DnsAliasWithSync {
  alias: DnsRecord | null;
  syncResults: DnsSyncResult[];
}

/** Mirrors the Rust `VibeNetworkSyncResult` DTO - the combined "Synchronize Vibe Network" action's per-Node outcome. */
export interface VibeNetworkSyncResult {
  serverId: string;
  ok: boolean;
  meshError: string | null;
  firewallError: string | null;
  dnsError: string | null;
}
