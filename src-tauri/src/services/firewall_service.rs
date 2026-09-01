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
use crate::models::{FirewallCustomRuleInput, PortProtocol, PortVisibility};
use crate::network::wireguard;
use crate::services::ssh_service::get_or_connect;
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::firewall_rule_repository::FirewallRuleRepository;
use crate::storage::node_network_repository::NodeNetworkRepository;
use crate::storage::server_repository::ServerRepository;

/// Which rule "owns" a `FirewallRule` in `desired_rules` - lets the Firewall
/// page show *why* a rule exists (and, for `Custom`, offer to remove it
/// directly) instead of a bare, unexplained port list. Never itself the
/// thing that decides what's safe to revoke - that's still
/// `FirewallProvider::vibessh_owned_rules`, see `firewall::mod`'s own doc
/// comment.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum FirewallRuleOrigin {
    Ssh,
    WireGuard,
    Application { application_id: Uuid, application_name: String, port_name: String },
    Custom { rule_id: Uuid, label: Option<String> },
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallRuleView {
    #[serde(flatten)]
    pub rule: FirewallRule,
    pub origin: FirewallRuleOrigin,
}

/// What the Firewall page renders in one call - every desired rule
/// (labeled with where it came from) plus whether enforcement is actually
/// on right now. `backend`/`active` fall back to `None`/`false` when the
/// Node can't be reached rather than failing the whole read - the rule
/// list itself is a pure DB read and always available even when the Node
/// is temporarily down, same "a connectivity hiccup must never block an
/// otherwise-valid read" stance `services::application_service::
/// check_external_port_available`'s own live probe already takes.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeFirewallOverview {
    pub backend: Option<String>,
    pub active: bool,
    pub rules: Vec<FirewallRuleView>,
}

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
    /// How many previously-applied rules this reconcile just revoked - a
    /// port that's been unpublished, or belonged to an Application that's
    /// been deleted or migrated away. See `firewall::mod`'s own doc comment
    /// for why this can only ever remove a rule the same backend can prove
    /// it added itself.
    pub rules_removed: usize,
    /// `true` when nothing on this Node is actually enforcing these rules -
    /// either there is no firewall backend VibeSSH can drive, or there is
    /// one and it is switched off.
    ///
    /// This used to be indistinguishable from success. `provider_for`
    /// returning `None` produced `FirewallSyncResult { active: false,
    /// rules_applied: 0 }` wrapped in `Ok`, which the frontend rendered as
    /// a success toast - so on a Node without `ufw`, publishing a port
    /// reported "synced" while nothing whatsoever restricted it. The caller
    /// needs to tell "your rules are enforced" apart from "there is nothing
    /// enforcing your rules", so it gets an explicit flag rather than every
    /// call site having to infer it from `backend.is_none() || !active`.
    pub unenforced: bool,
}

impl FirewallSyncResult {
    /// The result for a Node with no firewall backend at all. Deliberately
    /// not a `Default` impl - constructing one should always be a conscious
    /// choice, never what you get by forgetting a field.
    fn unenforced() -> Self {
        Self { backend: None, active: false, rules_applied: 0, rules_removed: 0, unenforced: true }
    }
}

/// Every port this Node's Applications have asked to be reachable from
/// outside, plus the Node's own SSH port and any manually declared custom
/// rule - the SSH port is always first, so it's the first rule
/// `apply_rules`/`enable` ever add. This is the hard safety invariant
/// `firewall::mod`'s own doc comment documents: computed here rather than
/// left to each `FirewallProvider` impl, so every backend gets it for free
/// rather than having to remember it independently.
pub fn desired_rules_with_origin(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    network_repo: &NodeNetworkRepository,
    firewall_rule_repo: &FirewallRuleRepository,
    server_id: Uuid,
) -> AppResult<Vec<FirewallRuleView>> {
    let server = server_repo.get(server_id)?.ok_or_else(|| AppError::NotFound(format!("server {server_id}")))?;
    let mut views = vec![FirewallRuleView {
        rule: FirewallRule { port: server.ssh_port, protocol: PortProtocol::Tcp, source_cidr: None },
        origin: FirewallRuleOrigin::Ssh,
    }];

    // Etap M4: a Node that's joined the Vibe Network needs its own
    // WireGuard port reachable from anywhere (peers dial it to establish
    // the tunnel in the first place, so this can't itself be mesh-scoped),
    // and every "Vibe Network only" Application port scoped to the mesh
    // CIDR instead of the open internet.
    let is_mesh_member = network_repo.get(server_id)?.is_some();
    if is_mesh_member {
        views.push(FirewallRuleView {
            rule: FirewallRule { port: wireguard::LISTEN_PORT, protocol: PortProtocol::Udp, source_cidr: None },
            origin: FirewallRuleOrigin::WireGuard,
        });
    }

    for application in app_repo.list_by_server(server_id)? {
        for port in app_repo.list_ports(application.id)? {
            let Some(external_port) = port.external_port else { continue };
            let source_cidr = match port.visibility {
                PortVisibility::VibeNetwork => Some(NodeNetworkRepository::MESH_CIDR.to_string()),
                _ => None,
            };
            let rule = FirewallRule { port: external_port, protocol: port.protocol, source_cidr };
            if !views.iter().any(|view| view.rule == rule) {
                views.push(FirewallRuleView {
                    rule,
                    origin: FirewallRuleOrigin::Application { application_id: application.id, application_name: application.name.clone(), port_name: port.name.clone() },
                });
            }
        }
    }

    for custom in firewall_rule_repo.list(server_id)? {
        let rule = FirewallRule { port: custom.port, protocol: custom.protocol, source_cidr: custom.source_cidr.clone() };
        if !views.iter().any(|view| view.rule == rule) {
            views.push(FirewallRuleView { rule, origin: FirewallRuleOrigin::Custom { rule_id: custom.id, label: custom.label.clone() } });
        }
    }

    Ok(views)
}

/// The bare rule list `FirewallProvider::apply_rules`/`enable` actually
/// need - every reconcile path goes through this, not
/// `desired_rules_with_origin` directly, so a backend impl never has to
/// know origin tracking exists.
pub fn desired_rules(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    network_repo: &NodeNetworkRepository,
    firewall_rule_repo: &FirewallRuleRepository,
    server_id: Uuid,
) -> AppResult<Vec<FirewallRule>> {
    Ok(desired_rules_with_origin(app_repo, server_repo, network_repo, firewall_rule_repo, server_id)?.into_iter().map(|view| view.rule).collect())
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
    firewall_rule_repo: &FirewallRuleRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
) -> AppResult<FirewallSyncResult> {
    let rules = desired_rules(app_repo, server_repo, network_repo, firewall_rule_repo, server_id)?;

    async fn attempt(server_repo: &ServerRepository, sessions: &SshSessionManager, server_id: Uuid, rules: &[FirewallRule]) -> AppResult<FirewallSyncResult> {
        let connection = get_or_connect(server_repo, sessions, server_id).await?;
        let Some(provider) = firewall::provider_for(&connection).await? else {
            return Ok(FirewallSyncResult::unenforced());
        };
        // Desired first, unconditionally - the SSH port (always first in
        // `rules`) must never go missing even for a moment, including the
        // case where the Node's configured SSH port itself just changed
        // and the old one is about to be revoked below.
        provider.apply_rules(&connection, rules).await?;
        let rules_removed = revoke_obsolete_rules(provider.as_ref(), &connection, rules).await?;
        // Published Docker ports bypass ufw entirely, so a source-scoped
        // rule only actually restricts container traffic once it also
        // exists in `DOCKER-USER` - see that module's own doc comment.
        // Best-effort: a Node with no Docker has nothing to reconcile, and
        // an iptables failure must not make an otherwise-successful ufw
        // sync look like a total failure.
        if let Err(err) = firewall::docker_user::reconcile(&connection, rules).await {
            log::warn!("couldn't reconcile the DOCKER-USER chain on server {server_id}: {err}");
        }
        let active = provider.is_active(&connection).await?;
        // A backend that exists but is switched off enforces nothing
        // either: the rules are recorded and take effect the moment it
        // is enabled, but right now the ports are open.
        Ok(FirewallSyncResult { backend: Some(provider.name().to_string()), active, rules_applied: rules.len(), rules_removed, unenforced: !active })
    }

    // Same dead-cached-session recovery as `network_service::reconcile_mesh`
    // - this runs several sequential SSH round-trips against the connection,
    // any of which surfaces the same raw channel error if the cache handed
    // back a session whose underlying transport already died.
    match attempt(server_repo, sessions, server_id, &rules).await {
        Ok(result) => Ok(result),
        Err(first_err) => {
            sessions.remove(server_id).await;
            attempt(server_repo, sessions, server_id, &rules).await.map_err(|_| first_err)
        }
    }
}

/// Turns firewall *enforcement* on for a Node - the explicit, user-triggered
/// action `firewall::mod`'s own doc comment says `reconcile_node` must never
/// do as a side effect. Delegates the actual ordering (apply every desired
/// rule, SSH port included and always first, only then flip enforcement on)
/// to `FirewallProvider::enable` itself, not to caller discipline here - see
/// that trait method's own doc comment for why. `backend: None` (not an
/// error) when the Node has no supported firewall detected, same shape
/// `reconcile_node` already uses.
pub async fn enable_node_firewall(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    network_repo: &NodeNetworkRepository,
    firewall_rule_repo: &FirewallRuleRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
) -> AppResult<FirewallSyncResult> {
    let rules = desired_rules(app_repo, server_repo, network_repo, firewall_rule_repo, server_id)?;

    async fn attempt(server_repo: &ServerRepository, sessions: &SshSessionManager, server_id: Uuid, rules: &[FirewallRule]) -> AppResult<FirewallSyncResult> {
        let connection = get_or_connect(server_repo, sessions, server_id).await?;
        let Some(provider) = firewall::provider_for(&connection).await? else {
            return Ok(FirewallSyncResult::unenforced());
        };
        provider.enable(&connection, rules).await?;
        let rules_removed = revoke_obsolete_rules(provider.as_ref(), &connection, rules).await?;
        let active = provider.is_active(&connection).await?;
        Ok(FirewallSyncResult { backend: Some(provider.name().to_string()), active, rules_applied: rules.len(), rules_removed, unenforced: !active })
    }

    // Same dead-cached-session recovery `reconcile_node` already uses.
    match attempt(server_repo, sessions, server_id, &rules).await {
        Ok(result) => Ok(result),
        Err(first_err) => {
            sessions.remove(server_id).await;
            attempt(server_repo, sessions, server_id, &rules).await.map_err(|_| first_err)
        }
    }
}

/// Diffs `desired` against whatever this backend can prove it already
/// applied (`vibessh_owned_rules`) and revokes the difference - the other
/// half of a real reconcile, not just an additive one. Returns how many
/// rules were actually removed, for `FirewallSyncResult::rules_removed`.
async fn revoke_obsolete_rules(provider: &dyn firewall::FirewallProvider, connection: &crate::ssh::SshSession, desired: &[FirewallRule]) -> AppResult<usize> {
    let owned = provider.vibessh_owned_rules(connection).await?;
    let obsolete: Vec<FirewallRule> = owned.into_iter().filter(|rule| !desired.contains(rule)).collect();
    if obsolete.is_empty() {
        return Ok(0);
    }
    let count = obsolete.len();
    provider.revoke_rules(connection, &obsolete).await?;
    Ok(count)
}

/// One socket already listening on the target host, parsed from `ss
/// -tlnp`/`ss -ulnp` - used by `services::application_service`'s port
/// collision check (the design doc's "Docker bindings, listening sockets"
/// requirement) before a new Exit Port is ever saved. `sudo` since a
/// non-root SSH user (Etap H's Agent-less SSH mode) can't otherwise see
/// which process owns a socket - only whether one is bound at all, which
/// isn't enough to build a useful error message from.
pub async fn listening_process(connection: &crate::ssh::SshSession, protocol: PortProtocol, port: u16) -> AppResult<Option<String>> {
    let flag = match protocol {
        PortProtocol::Tcp => "-tlnp",
        PortProtocol::Udp => "-ulnp",
    };
    let output = connection.execute_command(&format!("sudo ss {flag}")).await?;
    Ok(parse_ss_output(&output.stdout)
        .into_iter()
        .find(|socket| socket.port == port)
        .map(|socket| socket.process.unwrap_or_else(|| "unknown process".to_string())))
}

struct ListeningSocket {
    port: u16,
    process: Option<String>,
}

/// Parses `ss -tlnp`/`ss -ulnp` output, e.g.:
/// ```text
/// State  Recv-Q Send-Q Local Address:Port  Peer Address:PortProcess
/// LISTEN 0      4096         0.0.0.0:22         0.0.0.0:*    users:(("sshd",pid=1123,fd=3))
/// LISTEN 0      4096            [::]:22            [::]:*    users:(("sshd",pid=1123,fd=4))
/// UNCONN 0      0               127.0.0.54:53         0.0.0.0:*    users:(("systemd-resolve",pid=533,fd=16))
/// ```
/// The header row is never mistaken for data - its own "Local" token in the
/// port-bearing column has no `:port` suffix to parse, so it's filtered out
/// by the same `?` every other malformed row already falls through. The
/// port is always whatever follows the *last* `:` in that column, which
/// holds for a plain `0.0.0.0:22`, an IPv6 `[::]:22`, and a scoped
/// `127.0.0.53%lo:53` alike. `process` is best-effort (`None` for a socket
/// this SSH user isn't allowed to see the owner of, e.g. one already
/// wrapped in `sudo` that still can't cross a container/namespace boundary)
/// - the caller still knows *a* socket is there either way.
fn parse_ss_output(output: &str) -> Vec<ListeningSocket> {
    output
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let local_address = fields.get(3)?;
            let port: u16 = local_address.rsplit(':').next()?.parse().ok()?;
            let process = line.find("users:((").and_then(|start| {
                let rest = &line[start + "users:((".len()..];
                rest.split('"').nth(1).map(str::to_string)
            });
            Some(ListeningSocket { port, process })
        })
        .collect()
}

/// A Local application has no Node/SSH port to sync a firewall against -
/// `None`, not an error, same shape `resolve_health_check_spec` and other
/// "this only makes sense for a Remote application" call sites already use.
pub async fn sync_application_node_firewall(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    network_repo: &NodeNetworkRepository,
    firewall_rule_repo: &FirewallRuleRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
) -> AppResult<Option<FirewallSyncResult>> {
    let application = app_repo.get(application_id)?.ok_or_else(|| AppError::NotFound(format!("application {application_id}")))?;
    let Some(server_id) = application.application.server_id else { return Ok(None) };
    Ok(Some(reconcile_node(app_repo, server_repo, network_repo, firewall_rule_repo, sessions, server_id).await?))
}

/// Persists a manual rule then attempts to apply it live right away
/// (best-effort - the same "a connectivity hiccup must never block an
/// otherwise-valid change" stance `services::application_service::
/// sync_firewall_best_effort` already takes for Application ports). The
/// row is the source of truth either way; a failed live sync here is
/// exactly what the Firewall page's own "Sync now" action is for.
pub async fn add_custom_firewall_rule(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    network_repo: &NodeNetworkRepository,
    firewall_rule_repo: &FirewallRuleRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    input: FirewallCustomRuleInput,
) -> AppResult<crate::models::FirewallCustomRule> {
    let created = firewall_rule_repo.create(server_id, &input)?;
    if let Err(err) = reconcile_node(app_repo, server_repo, network_repo, firewall_rule_repo, sessions, server_id).await {
        log::warn!("firewall sync after adding a custom rule failed (server {server_id}): {err}");
    }
    Ok(created)
}

/// Removes a manual rule and best-effort revokes it live - same shape as
/// `add_custom_firewall_rule`. The row is deleted either way; if the live
/// revoke fails (Node unreachable), the next successful sync (automatic or
/// via "Sync now") picks it up, same as any other obsolete rule.
pub async fn remove_custom_firewall_rule(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    network_repo: &NodeNetworkRepository,
    firewall_rule_repo: &FirewallRuleRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    rule_id: Uuid,
) -> AppResult<()> {
    firewall_rule_repo.delete(rule_id)?;
    if let Err(err) = reconcile_node(app_repo, server_repo, network_repo, firewall_rule_repo, sessions, server_id).await {
        log::warn!("firewall sync after removing a custom rule failed (server {server_id}): {err}");
    }
    Ok(())
}

/// Everything the Firewall page needs in one call - see
/// `NodeFirewallOverview`'s own doc comment for why `backend`/`active`
/// degrade gracefully instead of failing the whole read.
pub async fn node_firewall_overview(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    network_repo: &NodeNetworkRepository,
    firewall_rule_repo: &FirewallRuleRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
) -> AppResult<NodeFirewallOverview> {
    let rules = desired_rules_with_origin(app_repo, server_repo, network_repo, firewall_rule_repo, server_id)?;
    let (backend, active) = match get_or_connect(server_repo, sessions, server_id).await {
        Ok(connection) => match firewall::provider_for(&connection).await {
            Ok(Some(provider)) => {
                let active = provider.is_active(&connection).await.unwrap_or(false);
                (Some(provider.name().to_string()), active)
            }
            _ => (None, false),
        },
        Err(_) => (None, false),
    };
    Ok(NodeFirewallOverview { backend, active, rules })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CreateApplicationInput, PortInput, RuntimeType};

    fn temp_setup() -> (ApplicationRepository, ServerRepository, NodeNetworkRepository, FirewallRuleRepository) {
        let path = std::env::temp_dir().join(format!("vibessh-firewall-service-test-{}.sqlite3", Uuid::new_v4()));
        (
            ApplicationRepository::open(&path).unwrap(),
            ServerRepository::open(&path).unwrap(),
            NodeNetworkRepository::open(&path).unwrap(),
            FirewallRuleRepository::open(&path).unwrap(),
        )
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
        let (app_repo, server_repo, network_repo, firewall_rule_repo) = temp_setup();
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

        let rules = desired_rules(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, server.id).unwrap();
        assert_eq!(rules[0], rule(2222, PortProtocol::Tcp), "the Node's own SSH port must always be first");
        assert!(rules.contains(&rule(25565, PortProtocol::Tcp)));
        assert_eq!(rules.len(), 2);
    }

    #[test]
    fn desired_rules_ignores_a_port_with_no_external_port_set() {
        let (app_repo, server_repo, network_repo, firewall_rule_repo) = temp_setup();
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

        let rules = desired_rules(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, server.id).unwrap();
        assert_eq!(rules, vec![rule(22, PortProtocol::Tcp)], "an internal-only port must never become a firewall rule");
    }

    #[test]
    fn desired_rules_adds_the_wireguard_port_for_a_mesh_member_and_scopes_a_vibe_network_port_to_the_mesh_cidr() {
        let (app_repo, server_repo, network_repo, firewall_rule_repo) = temp_setup();
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

        let rules = desired_rules(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, server.id).unwrap();
        assert!(rules.contains(&FirewallRule { port: wireguard::LISTEN_PORT, protocol: PortProtocol::Udp, source_cidr: None }));
        assert!(rules.contains(&FirewallRule { port: 8080, protocol: PortProtocol::Tcp, source_cidr: Some(NodeNetworkRepository::MESH_CIDR.to_string()) }));
    }

    fn test_server(server_repo: &ServerRepository, name: &str) -> crate::models::Server {
        server_repo
            .create(&crate::models::ServerInput {
                name: name.into(),
                host: "203.0.113.10".into(),
                ssh_port: 22,
                username: "root".into(),
                authentication_type: crate::models::AuthenticationType::Password,
                private_key_path: None,
                group_id: None,
                password: Some("x".into()),
                key_passphrase: None,
            })
            .unwrap()
    }

    #[test]
    fn desired_rules_includes_a_custom_rule_and_labels_every_rules_origin() {
        let (app_repo, server_repo, network_repo, firewall_rule_repo) = temp_setup();
        let server = test_server(&server_repo, "Test Node");
        firewall_rule_repo
            .create(server.id, &FirewallCustomRuleInput { label: Some("debug port".into()), protocol: PortProtocol::Udp, port: 9999, source_cidr: None })
            .unwrap();

        let views = desired_rules_with_origin(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, server.id).unwrap();
        assert!(matches!(views[0].origin, FirewallRuleOrigin::Ssh), "SSH must still be first with a custom rule present");
        let custom_view = views.iter().find(|v| v.rule.port == 9999).expect("the custom rule must appear in the desired set");
        assert!(matches!(&custom_view.origin, FirewallRuleOrigin::Custom { label, .. } if label.as_deref() == Some("debug port")));

        // The plain rule list (what's actually applied) must include it too.
        let rules = desired_rules(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, server.id).unwrap();
        assert!(rules.contains(&FirewallRule { port: 9999, protocol: PortProtocol::Udp, source_cidr: None }));
    }

    #[tokio::test]
    async fn remove_custom_firewall_rule_deletes_the_row_even_when_the_node_is_unreachable() {
        let (app_repo, server_repo, network_repo, firewall_rule_repo) = temp_setup();
        let server = test_server(&server_repo, "Unreachable Node");
        let created = firewall_rule_repo
            .create(server.id, &FirewallCustomRuleInput { label: None, protocol: PortProtocol::Tcp, port: 8443, source_cidr: None })
            .unwrap();
        let sessions = SshSessionManager::new();

        // The host (203.0.113.10, RFC 5737 TEST-NET-3) is unreachable - the
        // best-effort live revoke inside this call is expected to fail
        // silently, but the row itself must still be gone.
        remove_custom_firewall_rule(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, server.id, created.id).await.unwrap();
        assert!(desired_rules(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, server.id).unwrap().iter().all(|r| r.port != 8443));
    }

    #[test]
    fn parse_ss_output_reads_a_realistic_listing_including_ipv6_and_scoped_addresses() {
        let output = "State  Recv-Q Send-Q Local Address:Port  Peer Address:PortProcess\n\
LISTEN 0      4096         0.0.0.0:22         0.0.0.0:*    users:((\"sshd\",pid=1123,fd=3),(\"systemd\",pid=1,fd=192))\n\
LISTEN 0      4096      127.0.0.53%lo:53         0.0.0.0:*    users:((\"systemd-resolve\",pid=533,fd=15))\n\
LISTEN 0      4096            [::]:22            [::]:*    users:((\"sshd\",pid=1123,fd=4))\n";
        let sockets = parse_ss_output(output);
        assert_eq!(sockets.len(), 3);
        assert_eq!(sockets[0].port, 22);
        assert_eq!(sockets[0].process.as_deref(), Some("sshd"));
        assert_eq!(sockets[1].port, 53);
        assert_eq!(sockets[1].process.as_deref(), Some("systemd-resolve"));
        assert_eq!(sockets[2].port, 22);
    }

    #[test]
    fn parse_ss_output_skips_the_header_row_and_handles_a_socket_with_no_process_column() {
        let output = "State  Recv-Q Send-Q       Local Address:Port  Peer Address:PortProcess\nUNCONN 0      0                  0.0.0.0:54221      0.0.0.0:*                                             \n";
        let sockets = parse_ss_output(output);
        assert_eq!(sockets.len(), 1);
        assert_eq!(sockets[0].port, 54221);
        assert_eq!(sockets[0].process, None);
    }

    #[test]
    fn parse_ss_output_on_empty_input_is_empty() {
        assert!(parse_ss_output("").is_empty());
    }
}
