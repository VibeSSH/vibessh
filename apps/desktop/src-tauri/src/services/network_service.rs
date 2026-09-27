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
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::firewall_rule_repository::FirewallRuleRepository;
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

/// Something about a join or a leave that did not go as far as the change
/// itself. The membership change stands; each of these is something the
/// operator has to know to trust it - they were log lines, or `let _`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum NetworkWarning {
    /// Another member could not be updated, so it and this Node cannot
    /// reach each other until a sync gets through to it.
    #[serde(rename_all = "camelCase")]
    PeerNotUpdated { server_id: Uuid, message: String },
    /// The tunnel is up, but no peer answered - see
    /// `wireguard::await_handshake`.
    NoHandshake,
    /// This Node's firewall was not brought in line: on a join, the
    /// WireGuard port may still be closed; on a leave, still open.
    #[serde(rename_all = "camelCase")]
    Firewall { message: String },
    /// Private DNS was not updated on a Node - `None` when the sync could
    /// not run at all.
    #[serde(rename_all = "camelCase")]
    Dns { server_id: Option<Uuid>, message: String },
    /// "Vibe Network only" ports were not re-pointed at the mesh address.
    #[serde(rename_all = "camelCase")]
    BindAddresses { message: String },
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinOutcome {
    pub member: NodeNetworkMember,
    pub warnings: Vec<NetworkWarning>,
}

fn peer_warnings(results: &[MeshReconcileResult], except: Uuid) -> Vec<NetworkWarning> {
    results
        .iter()
        .filter(|result| result.server_id != except && !result.ok)
        .map(|result| NetworkWarning::PeerNotUpdated { server_id: result.server_id, message: result.error.clone().unwrap_or_default() })
        .collect()
}

fn firewall_warning(outcome: AppResult<crate::services::firewall_service::FirewallSyncResult>) -> Option<NetworkWarning> {
    match outcome {
        Ok(result) => result.container_error.map(|message| NetworkWarning::Firewall { message }),
        Err(err) => Some(NetworkWarning::Firewall { message: err.to_string() }),
    }
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
///
/// **A join that did not bring the tunnel up is not a join.** The mesh
/// reconcile reports per member, and this used to ignore the report: a Node
/// whose own `wg-quick up` failed was recorded as a member and the join
/// reported success. Now that failure is undone - the row removed, the
/// others reconciled without it - and returned.
///
/// Then the Node's firewall, which is what opens the WireGuard port; the
/// join never did that, so on a Node with ufw enforcing, no peer could ever
/// reach it. And only then the handshake check, which is the first point
/// at which "joined" can mean "reachable".
pub async fn join_node(
    network_repo: &NodeNetworkRepository,
    server_repo: &ServerRepository,
    app_repo: &ApplicationRepository,
    firewall_rule_repo: &FirewallRuleRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
) -> AppResult<JoinOutcome> {
    let server = server_repo.get(server_id)?.ok_or_else(|| AppError::NotFound(format!("server {server_id}")))?;
    require_ssh_mode(&server)?;
    if let Some(existing) = network_repo.get(server_id)? {
        return Ok(JoinOutcome { member: existing, warnings: vec![] });
    }

    let connection = get_or_connect(server_repo, sessions, server_id).await?;
    wireguard::install_if_missing(&connection).await?;
    let public_key = wireguard::ensure_keypair(&connection).await?;
    let member = network_repo.join(server_id, &public_key)?;

    let results = match reconcile_mesh(network_repo, server_repo, sessions).await {
        Ok(results) => results,
        Err(err) => {
            undo_join(network_repo, server_repo, sessions, server_id).await;
            return Err(err);
        }
    };
    if let Some(own) = results.iter().find(|result| result.server_id == server_id && !result.ok) {
        let detail = own.error.clone().unwrap_or_default();
        undo_join(network_repo, server_repo, sessions, server_id).await;
        return Err(AppError::Connection(format!("the tunnel couldn't be brought up on this Node, so it hasn't joined the Vibe Network: {detail}")));
    }

    let mut warnings = peer_warnings(&results, server_id);
    warnings.extend(firewall_warning(
        crate::services::firewall_service::reconcile_node(app_repo, server_repo, network_repo, firewall_rule_repo, sessions, server_id).await,
    ));
    // Nobody to shake hands with when this is the first member.
    if results.len() > 1 {
        let answered = match get_or_connect(server_repo, sessions, server_id).await {
            Ok(connection) => wireguard::await_handshake(&connection).await,
            Err(err) => Err(err),
        };
        match answered {
            Ok(true) => {}
            Ok(false) => warnings.push(NetworkWarning::NoHandshake),
            Err(err) => {
                log::warn!("couldn't check the tunnel on server {server_id} after joining: {err}");
                warnings.push(NetworkWarning::NoHandshake);
            }
        }
    }
    Ok(JoinOutcome { member, warnings })
}

/// Takes back a join that did not work. Best-effort by nature - it runs
/// because something already failed, and that failure is what the caller
/// returns - but each step that does not work is logged, not dropped.
async fn undo_join(network_repo: &NodeNetworkRepository, server_repo: &ServerRepository, sessions: &SshSessionManager, server_id: Uuid) {
    match get_or_connect(server_repo, sessions, server_id).await {
        Ok(connection) => {
            if let Err(err) = wireguard::teardown(&connection).await {
                log::warn!("couldn't take down the half-joined interface on server {server_id}: {err}");
            }
        }
        Err(err) => log::warn!("couldn't reach server {server_id} to take down its half-joined interface: {err}"),
    }
    if let Err(err) = network_repo.leave(server_id) {
        log::error!("couldn't remove the failed join of server {server_id} from the Vibe Network: {err}");
        return;
    }
    match reconcile_mesh(network_repo, server_repo, sessions).await {
        Ok(results) => {
            for failed in results.iter().filter(|result| !result.ok) {
                log::warn!("server {} still lists the Node whose join failed as a peer: {}", failed.server_id, failed.error.clone().unwrap_or_default());
            }
        }
        Err(err) => log::warn!("couldn't reconcile the Vibe Network after undoing a failed join: {err}"),
    }
}

/// Tears down this Node's own interface, then reconciles the remaining
/// members so they drop the departed peer from their own configs.
///
/// **Everything the membership put on the Node goes.** The interface and its
/// systemd unit (`wireguard::teardown`), the private DNS block in
/// `/etc/hosts`, which only the remaining members were ever re-synced
/// and which this Node kept pointing at mesh addresses, and the WireGuard
/// port in its firewall, which stayed open.
///
/// **Refused while a port depends on the mesh.** A "Vibe Network only"
/// port is bound to this Node's mesh address, which stops existing here:
/// the Application would fail to start with "cannot assign requested
/// address", and quietly re-binding it anywhere else would widen who can
/// reach it. The operator decides where each one goes first.
pub async fn leave_node(
    network_repo: &NodeNetworkRepository,
    server_repo: &ServerRepository,
    app_repo: &ApplicationRepository,
    firewall_rule_repo: &FirewallRuleRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
) -> AppResult<Vec<NetworkWarning>> {
    let mut mesh_ports = Vec::new();
    for application in app_repo.list_by_server(server_id)? {
        for port in app_repo.list_ports(application.id)? {
            if port.visibility == crate::models::PortVisibility::VibeNetwork {
                mesh_ports.push(format!("{} - {}", application.name, port.name));
            }
        }
    }
    if !mesh_ports.is_empty() {
        return Err(AppError::NetworkLeaveHasMeshPorts { ports: mesh_ports });
    }

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
    let mut warnings = Vec::new();
    if let Err(err) = crate::services::dns_service::clear_hosts_block(&connection).await {
        warnings.push(NetworkWarning::Dns { server_id: Some(server_id), message: err.to_string() });
    }
    network_repo.leave(server_id)?;
    warnings.extend(peer_warnings(&reconcile_mesh(network_repo, server_repo, sessions).await?, server_id));
    warnings.extend(firewall_warning(
        crate::services::firewall_service::reconcile_node(app_repo, server_repo, network_repo, firewall_rule_repo, sessions, server_id).await,
    ));
    Ok(warnings)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CreateApplicationInput, PortInput, PortProtocol, PortVisibility, RuntimeType};

    /// Leaving takes the mesh address away, and a port bound to it would
    /// never start again. Refused before anything on the Node is touched -
    /// the server here is not even reachable - and the membership stays.
    #[tokio::test]
    async fn a_node_with_vibe_network_only_ports_cannot_leave() {
        let path = std::env::temp_dir().join(format!("vibessh-network-service-test-{}.sqlite3", Uuid::new_v4()));
        let server_repo = ServerRepository::open(&path).unwrap();
        let network_repo = NodeNetworkRepository::open(&path).unwrap();
        let app_repo = ApplicationRepository::open(&path).unwrap();
        let firewall_rule_repo = FirewallRuleRepository::open(&path).unwrap();
        let sessions = SshSessionManager::new();

        let server = server_repo
            .create(&crate::models::ServerInput {
                name: "Unreachable".into(),
                host: "127.0.0.1".into(),
                ssh_port: 1,
                username: "root".into(),
                authentication_type: crate::models::AuthenticationType::Password,
                private_key_path: None,
                group_id: None,
                password: Some("x".into()),
                key_passphrase: None,
            })
            .unwrap();
        network_repo.join(server.id, "K4hV1cB0mQ2sT7nZ9xY3lJ6pR8dW5gA0fE1uI2oC3vM=").unwrap();
        let app = app_repo
            .create(&CreateApplicationInput {
                server_id: Some(server.id),
                name: "Database".into(),
                description: None,
                blueprint_id: "generic-docker".into(),
                blueprint_version: 1,
                runtime_type: RuntimeType::Docker,
                working_directory: "/srv/db".into(),
                environment: vec![],
                ports: vec![],
                runtime_config: serde_json::json!({}),
                metadata: serde_json::json!({}),
            })
            .unwrap();
        app_repo
            .add_port(
                app.application.id,
                &PortInput {
                    name: "mysql".into(),
                    protocol: PortProtocol::Tcp,
                    bind_address: "10.77.0.1".into(),
                    internal_port: 3306,
                    external_port: Some(3306),
                    visibility: PortVisibility::VibeNetwork,
                    required: false,
                },
            )
            .unwrap();

        let err = leave_node(&network_repo, &server_repo, &app_repo, &firewall_rule_repo, &sessions, server.id).await.unwrap_err();
        match err {
            AppError::NetworkLeaveHasMeshPorts { ports } => assert_eq!(ports, vec!["Database - mysql".to_string()]),
            other => panic!("expected the mesh-ports refusal, got {other}"),
        }
        assert!(network_repo.get(server.id).unwrap().is_some(), "a refused leave must not remove the membership");
    }
}
