//! Etap M4: Vibe Network (WireGuard mesh) orchestration - IPAM
//! (`storage::node_network_repository`) + the real `wg`/`wg-quick`
//! mechanics (`network::wireguard`) + a full-mesh reconcile that keeps
//! every member's peer list in sync with the current membership set.
//!
//! **SSH-mode Nodes only, for now.** An Agent-mode Node has no direct SSH
//! exec path this service could use, and pushing WireGuard config over the
//! Etap M3 command channel needs `vibessh_protocol::NodeDesiredState` to
//! actually carry a real payload, which it doesn't yet (see that type's own
//! doc comment) - rejected outright rather than silently no-oping, the same
//! "don't pretend two connection modes have identical capabilities" stance
//! `runtime::docker::DockerRuntime::validate`/`services::node_state_service::reconcile_node`
//! already take elsewhere.

use std::collections::HashMap;

use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{ConnectionMode, NodeNetworkMember, Server};
use crate::network::wireguard::{self, Peer};
use crate::services::ssh_service::{get_or_connect, retry_on_connection_failure};
use crate::state::SshSessionManager;
use crate::storage::node_network_repository::NodeNetworkRepository;
use crate::storage::server_repository::ServerRepository;

/// Per-member outcome of a mesh reconcile - never collapsed into one
/// boolean across the fleet, the same "don't claim SUCCESS for a Node that
/// was unreachable" requirement `firewall_service`/`node_state_service`
/// already apply.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeshReconcileResult {
    pub server_id: Uuid,
    pub ok: bool,
    pub error: Option<String>,
}

fn require_ssh_mode(server: &Server) -> AppResult<()> {
    if server.connection_mode != ConnectionMode::Ssh {
        return Err(AppError::InvalidInput("the Vibe Network is only supported for SSH-mode Nodes right now".into()));
    }
    Ok(())
}

/// Installs WireGuard if missing, generates (or reuses) this Node's own
/// keypair, allocates it a mesh IP, and reconciles the *whole* mesh - every
/// existing member also needs a `[Peer]` block for the new one, not just
/// the new Node needing blocks for everyone else.
pub async fn join_node(
    network_repo: &NodeNetworkRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
) -> AppResult<NodeNetworkMember> {
    let server = server_repo.get(server_id)?.ok_or_else(|| AppError::NotFound(format!("server {server_id}")))?;
    require_ssh_mode(&server)?;
    if let Some(existing) = network_repo.get(server_id)? {
        return Ok(existing);
    }

    let connection = get_or_connect(server_repo, sessions, server_id).await?;
    wireguard::install_if_missing(&connection).await?;
    let public_key = wireguard::ensure_keypair(&connection).await?;
    let member = network_repo.join(server_id, &public_key)?;

    reconcile_mesh(network_repo, server_repo, sessions).await?;
    Ok(member)
}

/// Tears down this Node's own interface (best-effort - an unreachable Node
/// still gets removed from the membership ledger, it just can't be told to
/// clean up its own side), then reconciles the remaining members so they
/// drop the departed peer from their own configs.
pub async fn leave_node(
    network_repo: &NodeNetworkRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
) -> AppResult<()> {
    // The row is only removed once the Node has actually been torn down.
    //
    // Previously the teardown result was discarded and the row removed
    // regardless, which produced the worst possible half-state: every other
    // member drops the departed Node from its peer list, while the departed
    // Node keeps its `wg-vibessh0` interface up with the *old* config -
    // still holding mesh addresses, still trying to reach peers that no
    // longer know it, and with nothing in VibeSSH left pointing at it to
    // clean it up. `wireguard::teardown` swallowed its own errors too, so
    // even checking the result would not have helped until it stopped.
    let connection = get_or_connect(server_repo, sessions, server_id).await?;
    wireguard::teardown(&connection).await?;
    network_repo.leave(server_id)?;
    reconcile_mesh(network_repo, server_repo, sessions).await?;
    Ok(())
}

/// Re-derives and re-applies the full peer set for every current member -
/// safe and idempotent to call after any membership change (join, leave)
/// or on demand, same "always re-derive the whole desired state" shape
/// `firewall_service::reconcile_node` already uses. A member that's
/// currently unreachable is reported, not silently skipped or treated as a
/// hard failure for the rest.
pub async fn reconcile_mesh(
    network_repo: &NodeNetworkRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
) -> AppResult<Vec<MeshReconcileResult>> {
    let members = network_repo.list()?;
    if members.is_empty() {
        return Ok(vec![]);
    }

    let mut hosts: HashMap<Uuid, String> = HashMap::new();
    for member in &members {
        let server = server_repo.get(member.server_id)?.ok_or_else(|| AppError::NotFound(format!("server {}", member.server_id)))?;
        hosts.insert(member.server_id, server.host);
    }

    let mut results = Vec::with_capacity(members.len());
    for member in &members {
        let peers: Vec<Peer> = members
            .iter()
            .filter(|other| other.server_id != member.server_id)
            .map(|other| Peer {
                public_key: other.wireguard_public_key.clone(),
                allowed_ip: format!("{}/32", other.wireguard_ip),
                endpoint: format!("{}:{}", hosts[&other.server_id], wireguard::LISTEN_PORT),
            })
            .collect();

        // A cached session gone stale (idle timeout, network blip, Node
        // reboot) fails here with a raw "couldn't open an SSH channel"
        // rather than reconnecting, and `apply` runs a whole script over the
        // connection directly - so it gets the same drop-and-retry-once
        // recovery `ssh_service::execute_command` gives a single command.
        //
        // Notably *not* retried any more: a peer value `wireguard::apply`
        // rejects. The local copy this replaced retried on every error, so a
        // rejected public key tore down a healthy session and re-ran the
        // apply to be rejected again, on every member, on every reconcile.
        let outcome = retry_on_connection_failure(sessions, Some(member.server_id), || async {
            let connection = get_or_connect(server_repo, sessions, member.server_id).await?;
            wireguard::apply(&connection, &member.wireguard_ip, &peers).await
        })
        .await;
        results.push(match outcome {
            Ok(()) => MeshReconcileResult { server_id: member.server_id, ok: true, error: None },
            Err(err) => MeshReconcileResult { server_id: member.server_id, ok: false, error: Some(err.to_string()) },
        });
    }
    Ok(results)
}

pub fn list_members(network_repo: &NodeNetworkRepository) -> AppResult<Vec<NodeNetworkMember>> {
    network_repo.list()
}

/// One resolved peer entry from a Node's own real `wg show ... dump` -
/// `public_key` cross-referenced back to whichever mesh member owns it, so
/// the UI can show "db01 <-> Hetzner-02: last handshake 3s ago" instead of
/// a bare, meaningless key.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerHandshake {
    pub server_id: Uuid,
    pub latest_handshake_unix: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

/// What the tunnel on one Node is doing - which is not the same question as
/// whether the Node answered SSH.
///
/// This exists because those two were the same field. `reachable` is an SSH
/// fact, and the UI drew "Connection: active" from it while the WireGuard
/// interface might not have existed at all; every case that produced no peer
/// list then rendered as "last handshake: never", including the cases where
/// nothing had been read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TunnelState {
    /// `wg show` ran and reported the peers in `peers`.
    Up,
    /// The interface is not on this Node - it has not joined, or has not
    /// been reconciled since joining.
    Down,
    /// The interface could not be read; see `tunnel_error`.
    Unknown,
    /// The Node itself did not answer, so nothing could be asked of it.
    Unreachable,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeMeshStatus {
    pub server_id: Uuid,
    /// Whether this Node was reachable (SSH) *right now*, when this status
    /// was collected - the closest thing to "online" this module claims
    /// without a live, continuously-updated connection to base it on.
    pub reachable: bool,
    pub tunnel: TunnelState,
    /// Why the tunnel could not be read, when `tunnel` is `Unknown`.
    pub tunnel_error: Option<String>,
    pub peers: Vec<PeerHandshake>,
    /// Peers `wg` reported whose public key belongs to no known member.
    ///
    /// Not noise: it is what a Node re-keyed behind the app's back looks
    /// like. Without it, that Node's peers are silently dropped and the card
    /// reads exactly like a tunnel that has never handshaked.
    pub unknown_peers: usize,
}

/// Real, current mesh state for every member - each Node is asked for its
/// own `wg show` output (not assumed from Desktop's side), and every peer
/// public key in that output is resolved back to the mesh member it
/// belongs to. A Node that's currently unreachable is reported as such
/// (`reachable: false`, empty `peers`), never silently omitted.
pub async fn mesh_status(network_repo: &NodeNetworkRepository, server_repo: &ServerRepository, sessions: &SshSessionManager) -> AppResult<Vec<NodeMeshStatus>> {
    let members = network_repo.list()?;
    let mut by_public_key: std::collections::HashMap<&str, Uuid> = std::collections::HashMap::new();
    for member in &members {
        by_public_key.insert(member.wireguard_public_key.as_str(), member.server_id);
    }

    let mut results = Vec::with_capacity(members.len());
    for member in &members {
        let status = match get_or_connect(server_repo, sessions, member.server_id).await {
            Ok(connection) => match wireguard::show_peers(&connection).await {
                Ok(wireguard::InterfaceState::Up(peers)) => {
                    let total = peers.len();
                    let resolved: Vec<PeerHandshake> = peers
                        .into_iter()
                        .filter_map(|peer| {
                            by_public_key.get(peer.public_key.as_str()).map(|&server_id| PeerHandshake {
                                server_id,
                                latest_handshake_unix: peer.latest_handshake_unix,
                                rx_bytes: peer.rx_bytes,
                                tx_bytes: peer.tx_bytes,
                            })
                        })
                        .collect();
                    NodeMeshStatus {
                        server_id: member.server_id,
                        reachable: true,
                        tunnel: TunnelState::Up,
                        tunnel_error: None,
                        unknown_peers: total - resolved.len(),
                        peers: resolved,
                    }
                }
                Ok(wireguard::InterfaceState::Missing) => NodeMeshStatus {
                    server_id: member.server_id,
                    reachable: true,
                    tunnel: TunnelState::Down,
                    tunnel_error: None,
                    peers: vec![],
                    unknown_peers: 0,
                },
                // The Node answered and told us why, so pass that on rather
                // than turning it into an empty list the UI reads as "never".
                Ok(wireguard::InterfaceState::Unreadable(detail)) => NodeMeshStatus {
                    server_id: member.server_id,
                    reachable: true,
                    tunnel: TunnelState::Unknown,
                    tunnel_error: Some(detail),
                    peers: vec![],
                    unknown_peers: 0,
                },
                Err(err) => NodeMeshStatus {
                    server_id: member.server_id,
                    reachable: true,
                    tunnel: TunnelState::Unknown,
                    tunnel_error: Some(err.to_string()),
                    peers: vec![],
                    unknown_peers: 0,
                },
            },
            Err(err) => NodeMeshStatus {
                server_id: member.server_id,
                reachable: false,
                tunnel: TunnelState::Unreachable,
                tunnel_error: Some(err.to_string()),
                peers: vec![],
                unknown_peers: 0,
            },
        };
        results.push(status);
    }
    Ok(results)
}

/// One port, with the owning Application's name attached - the "Endpoints"
/// view (Etap M4) is a Node-scoped read over the *existing* Ports data,
/// deliberately not a new, separate model: an Endpoint and an
/// `ApplicationPort` are the same thing, just viewed across every
/// Application on one Node instead of one Application at a time. CRUD
/// still goes through the very same `add_application_port`/
/// `update_application_port`/`remove_application_port` this reuses for
/// listing.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeEndpoint {
    pub application_id: uuid::Uuid,
    pub application_name: String,
    #[serde(flatten)]
    pub port: crate::models::ApplicationPort,
}

pub fn list_node_endpoints(app_repo: &crate::storage::application_repository::ApplicationRepository, server_id: Uuid) -> AppResult<Vec<NodeEndpoint>> {
    let mut endpoints = Vec::new();
    for application in app_repo.list_by_server(server_id)? {
        for port in app_repo.list_ports(application.id)? {
            endpoints.push(NodeEndpoint { application_id: application.id, application_name: application.name.clone(), port });
        }
    }
    Ok(endpoints)
}

/// Per-Node outcome of a combined "Synchronize Vibe Network" sync -
/// WireGuard peers + firewall + DNS, all in one action (the spec's own
/// "Sync / Reconcile" ask: one button, per-node OK/OUT OF SYNC). A Node
/// counts as fully `ok` only if every one of the three sub-syncs succeeded
/// for it - never a false-positive "synced" hiding a partial failure.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VibeNetworkSyncResult {
    pub server_id: Uuid,
    pub ok: bool,
    pub mesh_error: Option<String>,
    pub firewall_error: Option<String>,
    pub dns_error: Option<String>,
}

/// The one, whole-mesh "Synchronize Vibe Network" action - reconciles
/// WireGuard peers for every member, then re-applies each member's
/// firewall rules (SSH port + WireGuard port + every Application port,
/// scoped per `visibility`), then pushes the same Private DNS fragment to
/// every member. Best-effort per Node and per sub-system: one Node being
/// unreachable, or one sync type failing, never blocks the other Nodes or
/// the other sync types from still being attempted.
pub async fn sync_vibe_network(
    network_repo: &NodeNetworkRepository,
    server_repo: &ServerRepository,
    app_repo: &crate::storage::application_repository::ApplicationRepository,
    dns_repo: &crate::storage::dns_repository::DnsRepository,
    dns_suffix: &str,
    firewall_rule_repo: &crate::storage::firewall_rule_repository::FirewallRuleRepository,
    sessions: &SshSessionManager,
) -> AppResult<Vec<VibeNetworkSyncResult>> {
    let members = network_repo.list()?;
    if members.is_empty() {
        return Ok(vec![]);
    }

    let mesh_results = reconcile_mesh(network_repo, server_repo, sessions).await?;
    let dns_results = crate::services::dns_service::sync_dns(dns_suffix, network_repo, server_repo, app_repo, dns_repo, sessions).await?;

    let mut results = Vec::with_capacity(members.len());
    for member in &members {
        let mesh_error = mesh_results.iter().find(|r| r.server_id == member.server_id).and_then(|r| r.error.clone());
        let dns_error = dns_results.iter().find(|r| r.server_id == member.server_id).and_then(|r| r.error.clone());
        let firewall_error = match crate::services::firewall_service::reconcile_node(app_repo, server_repo, network_repo, firewall_rule_repo, sessions, member.server_id).await {
            // The container half is the half that restricts a "Vibe Network
            // only" Docker port - which is what joining the mesh is for. A
            // sync that could not write it is not a sync that worked.
            Ok(result) => result.container_error.map(|err| format!("container ports are not restricted to the Vibe Network: {err}")),
            Err(err) => Some(err.to_string()),
        };
        results.push(VibeNetworkSyncResult {
            server_id: member.server_id,
            ok: mesh_error.is_none() && dns_error.is_none() && firewall_error.is_none(),
            mesh_error,
            firewall_error,
            dns_error,
        });
    }
    Ok(results)
}
