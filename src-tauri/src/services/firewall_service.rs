//! Etap M2 (+ Etap M4's Vibe Network integration): derives the desired
//! firewall rule set for a Node from its own SSH port, its WireGuard mesh
//! port (if it's a mesh member), and every Application port (`external_port`
//! set) across every Application it hosts - scoped to the mesh CIDR instead
//! of the open internet for a "Vibe Network only" port - and applies it via
//! whichever `FirewallProvider` backend (if any) is detected there. See
//! `firewall::mod`'s own doc comment for why this is additive-only and
//! never auto-enables enforcement.

use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::firewall::{self, FirewallRule};
use crate::models::{PortProtocol, PortVisibility};
use crate::network::wireguard;
use crate::services::ssh_service::get_or_connect;
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::node_network_repository::NodeNetworkRepository;
use crate::storage::server_repository::ServerRepository;

/// `Serialize` so a Tauri command can hand this straight to the frontend -
/// the Ports tab's "Sync Firewall" action shows `backend`/`active` so the
/// user knows whether anything actually happened (`backend: None` means "no
/// supported firewall on this Node," not a failure).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallSyncResult {
    pub backend: Option<String>,
    pub active: bool,
    pub rules_applied: usize,
}

/// Every port this Node's Applications have asked to be reachable from
/// outside, plus the Node's own SSH port - the SSH port is always first,
/// so it's the first rule `apply_rules`/`enable` ever add. This is the hard
/// safety invariant `firewall::mod`'s own doc comment documents: computed
/// here rather than left to each `FirewallProvider` impl, so every backend
/// gets it for free rather than having to remember it independently.
fn desired_rules(app_repo: &ApplicationRepository, server_repo: &ServerRepository, network_repo: &NodeNetworkRepository, server_id: Uuid) -> AppResult<Vec<FirewallRule>> {
    let server = server_repo.get(server_id)?.ok_or_else(|| AppError::NotFound(format!("server {server_id}")))?;
    let mut rules = vec![FirewallRule { port: server.ssh_port, protocol: PortProtocol::Tcp, source_cidr: None }];

    // Etap M4: a Node that's joined the Vibe Network needs its own
    // WireGuard port reachable from anywhere (peers dial it to establish
    // the tunnel in the first place, so this can't itself be mesh-scoped),
    // and every "Vibe Network only" Application port scoped to the mesh
    // CIDR instead of the open internet.
    let is_mesh_member = network_repo.get(server_id)?.is_some();
    if is_mesh_member {
        rules.push(FirewallRule { port: wireguard::LISTEN_PORT, protocol: PortProtocol::Udp, source_cidr: None });
    }

    for application in app_repo.list_by_server(server_id)? {
        for port in app_repo.list_ports(application.id)? {
            let Some(external_port) = port.external_port else { continue };
            let source_cidr = match port.visibility {
                PortVisibility::VibeNetwork => Some(NodeNetworkRepository::MESH_CIDR.to_string()),
                _ => None,
            };
            let rule = FirewallRule { port: external_port, protocol: port.protocol, source_cidr };
            if !rules.contains(&rule) {
                rules.push(rule);
            }
        }
    }
    Ok(rules)
}

/// Additive-only reconcile (see `firewall::mod`'s own doc comment) - adds
/// every desired rule, never enables enforcement itself. `backend: None`
/// (not an error) when the Node has no supported firewall detected, the
/// same "nothing to do, not a failure" shape `firewall::provider_for`
/// already returns.
pub async fn reconcile_node(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    network_repo: &NodeNetworkRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
) -> AppResult<FirewallSyncResult> {
    let connection = get_or_connect(server_repo, sessions, server_id).await?;
    let Some(provider) = firewall::provider_for(&connection).await? else {
        return Ok(FirewallSyncResult { backend: None, active: false, rules_applied: 0 });
    };

    let rules = desired_rules(app_repo, server_repo, network_repo, server_id)?;
    provider.apply_rules(&connection, &rules).await?;
    let active = provider.is_active(&connection).await?;
    Ok(FirewallSyncResult { backend: Some(provider.name().to_string()), active, rules_applied: rules.len() })
}

/// A Local application has no Node/SSH port to sync a firewall against -
/// `None`, not an error, same shape `resolve_health_check_spec` and other
/// "this only makes sense for a Remote application" call sites already use.
pub async fn sync_application_node_firewall(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    network_repo: &NodeNetworkRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
) -> AppResult<Option<FirewallSyncResult>> {
    let application = app_repo.get(application_id)?.ok_or_else(|| AppError::NotFound(format!("application {application_id}")))?;
    let Some(server_id) = application.application.server_id else { return Ok(None) };
    Ok(Some(reconcile_node(app_repo, server_repo, network_repo, sessions, server_id).await?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CreateApplicationInput, PortInput, RuntimeType};

    fn temp_setup() -> (ApplicationRepository, ServerRepository, NodeNetworkRepository) {
        let path = std::env::temp_dir().join(format!("vibessh-firewall-service-test-{}.sqlite3", Uuid::new_v4()));
        (ApplicationRepository::open(&path).unwrap(), ServerRepository::open(&path).unwrap(), NodeNetworkRepository::open(&path).unwrap())
    }

    fn rule(port: u16, protocol: PortProtocol) -> FirewallRule {
        FirewallRule { port, protocol, source_cidr: None }
    }

    fn port_input(name: &str, bind_address: &str, internal_port: u16, external_port: Option<u16>) -> PortInput {
        PortInput {
            name: name.into(),
            protocol: PortProtocol::Tcp,
            bind_address: bind_address.into(),
            internal_port,
            external_port,
            visibility: PortVisibility::Public,
            required: false,
        }
    }

    #[test]
    fn desired_rules_always_leads_with_the_nodes_ssh_port_and_dedupes_shared_ports() {
        let (app_repo, server_repo, network_repo) = temp_setup();
        let server = server_repo
            .create(&crate::models::ServerInput {
                name: "Test Node".into(),
                host: "203.0.113.10".into(),
                ssh_port: 2222,
                username: "root".into(),
                authentication_type: crate::models::AuthenticationType::Password,
                private_key_path: None,
                group_id: None,
                password: Some("x".into()),
                key_passphrase: None,
            })
            .unwrap();

        let app_a = app_repo
            .create(&CreateApplicationInput {
                server_id: Some(server.id),
                name: "App A".into(),
                description: None,
                blueprint_id: "generic-docker".into(),
                blueprint_version: 1,
                runtime_type: RuntimeType::Docker,
                working_directory: "/srv/a".into(),
                environment: vec![],
                ports: vec![],
                runtime_config: serde_json::json!({}),
                metadata: serde_json::json!({}),
            })
            .unwrap();
        app_repo.add_port(app_a.application.id, &port_input("game", "0.0.0.0", 25565, Some(25565))).unwrap();

        let rules = desired_rules(&app_repo, &server_repo, &network_repo, server.id).unwrap();
        assert_eq!(rules[0], rule(2222, PortProtocol::Tcp), "the Node's own SSH port must always be first");
        assert!(rules.contains(&rule(25565, PortProtocol::Tcp)));
        assert_eq!(rules.len(), 2);
    }

    #[test]
    fn desired_rules_ignores_a_port_with_no_external_port_set() {
        let (app_repo, server_repo, network_repo) = temp_setup();
        let server = server_repo
            .create(&crate::models::ServerInput {
                name: "Test Node".into(),
                host: "203.0.113.10".into(),
                ssh_port: 22,
                username: "root".into(),
                authentication_type: crate::models::AuthenticationType::Password,
                private_key_path: None,
                group_id: None,
                password: Some("x".into()),
                key_passphrase: None,
            })
            .unwrap();
        let app = app_repo
            .create(&CreateApplicationInput {
                server_id: Some(server.id),
                name: "App".into(),
                description: None,
                blueprint_id: "generic-docker".into(),
                blueprint_version: 1,
                runtime_type: RuntimeType::Docker,
                working_directory: "/srv/a".into(),
                environment: vec![],
                ports: vec![],
                runtime_config: serde_json::json!({}),
                metadata: serde_json::json!({}),
            })
            .unwrap();
        app_repo.add_port(app.application.id, &port_input("internal-db", "127.0.0.1", 3306, None)).unwrap();

        let rules = desired_rules(&app_repo, &server_repo, &network_repo, server.id).unwrap();
        assert_eq!(rules, vec![rule(22, PortProtocol::Tcp)], "an internal-only port must never become a firewall rule");
    }

    #[test]
    fn desired_rules_adds_the_wireguard_port_for_a_mesh_member_and_scopes_a_vibe_network_port_to_the_mesh_cidr() {
        let (app_repo, server_repo, network_repo) = temp_setup();
        let server = server_repo
            .create(&crate::models::ServerInput {
                name: "Test Node".into(),
                host: "203.0.113.10".into(),
                ssh_port: 22,
                username: "root".into(),
                authentication_type: crate::models::AuthenticationType::Password,
                private_key_path: None,
                group_id: None,
                password: Some("x".into()),
                key_passphrase: None,
            })
            .unwrap();
        network_repo.join(server.id, "pubkeyA").unwrap();

        let app = app_repo
            .create(&CreateApplicationInput {
                server_id: Some(server.id),
                name: "App".into(),
                description: None,
                blueprint_id: "generic-docker".into(),
                blueprint_version: 1,
                runtime_type: RuntimeType::Docker,
                working_directory: "/srv/a".into(),
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
                    name: "internal-only".into(),
                    protocol: PortProtocol::Tcp,
                    bind_address: "0.0.0.0".into(),
                    internal_port: 8080,
                    external_port: Some(8080),
                    visibility: PortVisibility::VibeNetwork,
                    required: false,
                },
            )
            .unwrap();

        let rules = desired_rules(&app_repo, &server_repo, &network_repo, server.id).unwrap();
        assert!(rules.contains(&FirewallRule { port: wireguard::LISTEN_PORT, protocol: PortProtocol::Udp, source_cidr: None }));
        assert!(rules.contains(&FirewallRule { port: 8080, protocol: PortProtocol::Tcp, source_cidr: Some(NodeNetworkRepository::MESH_CIDR.to_string()) }));
    }
}
