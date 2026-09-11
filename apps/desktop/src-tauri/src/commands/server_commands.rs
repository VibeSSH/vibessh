use tauri::State;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::firewall::FirewallRule;
use crate::models::{FirewallCustomRule, FirewallCustomRuleInput, NodeCapabilities, Server, ServerInput};
use crate::services::{self, FirewallSyncResult, NodeFirewallOverview};
use crate::state::{PortForwardManager, SshSessionManager};
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::firewall_rule_repository::FirewallRuleRepository;
use crate::storage::node_network_repository::NodeNetworkRepository;
use crate::storage::server_repository::ServerRepository;

#[tauri::command]
pub fn create_server(repo: State<ServerRepository>, input: ServerInput) -> AppResult<Server> {
    services::create_server(&repo, input)
}

#[tauri::command]
pub fn update_server(repo: State<ServerRepository>, id: Uuid, input: ServerInput) -> AppResult<Server> {
    services::update_server(&repo, id, input)
}

/// `icon: None` clears it. The value is a base64 PNG data URL - the frontend
/// re-encodes whatever the user picked through a canvas first, and the
/// repository re-checks that on the way in.
#[tauri::command]
pub fn set_server_icon(repo: State<ServerRepository>, id: Uuid, icon: Option<String>) -> AppResult<Server> {
    services::set_server_icon(&repo, id, icon)
}

#[tauri::command]
pub async fn delete_server(repo: State<'_, ServerRepository>, forwards: State<'_, PortForwardManager>, id: Uuid) -> AppResult<()> {
    services::delete_server(&repo, id)?;
    // Best-effort, same reasoning as the SSH credential cleanup inside
    // services::delete_server itself - a tunnel to a Node that no longer
    // has a Server row shouldn't be left running until it errors out on its
    // own.
    forwards.stop_all_for_server(id).await;
    Ok(())
}

/// Keeps a password for this run only, after the UI has asked for one.
///
/// Deliberately has no "save it" counterpart: a password that can be stored
/// goes to the OS credential store when the Node is created, and this exists
/// for the machines where that is not possible. Writing it anywhere is what
/// `storage::credentials` refuses to do, and adding a second path to disk
/// here would quietly undo that.
#[tauri::command]
pub fn remember_session_password(server_id: Uuid, password: String) -> AppResult<()> {
    if password.is_empty() {
        return Err(AppError::InvalidInput("the password is empty".into()));
    }
    crate::state::session_passwords::remember(server_id, password);
    Ok(())
}

/// Drops a remembered password - called when one turned out to be wrong, so
/// the next attempt asks again instead of failing the same way forever.
#[tauri::command]
pub fn forget_session_password(server_id: Uuid) -> AppResult<()> {
    crate::state::session_passwords::forget(server_id);
    Ok(())
}

/// Lists game servers already sitting in a directory, ready to be adopted.
///
/// Reads and nothing else - these are directories somebody is very likely
/// still running servers out of.
#[tauri::command]
pub async fn scan_for_servers(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Option<Uuid>,
    directory: String,
) -> AppResult<Vec<crate::models::DiscoveredServer>> {
    services::server_discovery_service::scan_for_servers(&repo, &sessions, server_id, &directory).await
}

#[tauri::command]
pub fn get_server(repo: State<ServerRepository>, id: Uuid) -> AppResult<Server> {
    services::get_server(&repo, id)
}

#[tauri::command]
pub fn list_servers(repo: State<ServerRepository>) -> AppResult<Vec<Server>> {
    services::list_servers(&repo)
}

#[tauri::command]
pub fn upsert_agent_server(
    repo: State<ServerRepository>,
    name: String,
    host: String,
    agent_id: Uuid,
    docker_capable: Option<bool>,
) -> AppResult<Server> {
    services::upsert_agent_server(&repo, &name, &host, agent_id, docker_capable)
}

#[tauri::command]
pub fn upgrade_server_to_agent(
    repo: State<ServerRepository>,
    server_id: Uuid,
    agent_id: Uuid,
    docker_capable: Option<bool>,
) -> AppResult<Server> {
    services::upgrade_server_to_agent(&repo, server_id, agent_id, docker_capable)
}

/// SSH-mode only (see `services::probe_node_capabilities`'s own doc
/// comment) - the Create Application wizard calls this per SSH-mode server
/// as its Node picker loads, Etap M1.
#[tauri::command]
pub async fn probe_server_capabilities(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    id: Uuid,
) -> AppResult<NodeCapabilities> {
    services::probe_node_capabilities(&repo, &sessions, id).await
}

/// SSH-mode only, same reasoning as `probe_server_capabilities` - installs
/// Docker via its own official script (see `services::install_docker`'s own
/// doc comment) and returns the freshly re-probed, persisted capabilities.
#[tauri::command]
pub async fn install_docker(repo: State<'_, ServerRepository>, sessions: State<'_, SshSessionManager>, id: Uuid) -> AppResult<NodeCapabilities> {
    services::install_docker(&repo, &sessions, id).await
}

/// SSH-mode only, same reasoning as `install_docker` - installs WireGuard
/// (see `services::install_wireguard`'s own doc comment) and returns the
/// freshly re-probed, persisted capabilities.
#[tauri::command]
pub async fn install_wireguard(repo: State<'_, ServerRepository>, sessions: State<'_, SshSessionManager>, id: Uuid) -> AppResult<NodeCapabilities> {
    services::install_wireguard(&repo, &sessions, id).await
}

/// SSH-mode only, same reasoning as `install_docker` - installs ufw (see
/// `services::install_ufw`'s own doc comment) and returns the freshly
/// re-probed, persisted capabilities. Never enables enforcement itself.
#[tauri::command]
pub async fn install_ufw(repo: State<'_, ServerRepository>, sessions: State<'_, SshSessionManager>, id: Uuid) -> AppResult<NodeCapabilities> {
    services::install_ufw(&repo, &sessions, id).await
}

/// A local-only, no-SSH-round-trip read (see `services::firewall_service::
/// desired_rules`) - what the "Secure this server" confirmation dialog shows
/// *before* anything actually changes on the Node, so the user knows exactly
/// which ports stay reachable (SSH always first) before committing to
/// `enable_server_firewall`.
#[tauri::command]
pub fn preview_server_firewall_rules(
    app_repo: State<ApplicationRepository>,
    server_repo: State<ServerRepository>,
    network_repo: State<NodeNetworkRepository>,
    firewall_rule_repo: State<FirewallRuleRepository>,
    id: Uuid,
) -> AppResult<Vec<FirewallRule>> {
    services::preview_node_firewall_rules(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, id)
}

/// The explicit, user-triggered action that actually turns firewall
/// enforcement on for this Node - see `services::enable_node_firewall`'s own
/// doc comment for the ordering guarantee that makes this safe to call
/// without locking the connecting user out.
#[tauri::command]
pub async fn enable_server_firewall(
    app_repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    network_repo: State<'_, NodeNetworkRepository>,
    firewall_rule_repo: State<'_, FirewallRuleRepository>,
    sessions: State<'_, SshSessionManager>,
    id: Uuid,
) -> AppResult<FirewallSyncResult> {
    services::enable_node_firewall(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, id).await
}

/// "Sync now" - re-applies every desired rule and revokes whatever's
/// obsolete, without touching enforcement (see
/// `services::firewall_service::reconcile_node`'s own doc comment). The
/// Node-scoped counterpart to `sync_application_node_firewall`, for the
/// Firewall page itself rather than a single Application's Ports tab.
#[tauri::command]
pub async fn sync_node_firewall(
    app_repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    network_repo: State<'_, NodeNetworkRepository>,
    firewall_rule_repo: State<'_, FirewallRuleRepository>,
    sessions: State<'_, SshSessionManager>,
    id: Uuid,
) -> AppResult<FirewallSyncResult> {
    services::sync_node_firewall(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, id).await
}

/// Everything the Firewall page needs in one call - see
/// `services::firewall_service::NodeFirewallOverview`'s own doc comment.
#[tauri::command]
pub async fn get_node_firewall_overview(
    app_repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    network_repo: State<'_, NodeNetworkRepository>,
    firewall_rule_repo: State<'_, FirewallRuleRepository>,
    sessions: State<'_, SshSessionManager>,
    id: Uuid,
) -> AppResult<NodeFirewallOverview> {
    services::node_firewall_overview(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, id).await
}

/// Adds a manual firewall rule not tied to any Application's own port - see
/// `services::firewall_service::add_custom_firewall_rule`'s own doc comment.
#[tauri::command]
pub async fn add_firewall_custom_rule(
    app_repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    network_repo: State<'_, NodeNetworkRepository>,
    firewall_rule_repo: State<'_, FirewallRuleRepository>,
    sessions: State<'_, SshSessionManager>,
    id: Uuid,
    input: FirewallCustomRuleInput,
) -> AppResult<FirewallCustomRule> {
    services::add_custom_firewall_rule(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, id, input).await
}

/// Removes a manual firewall rule - see
/// `services::firewall_service::remove_custom_firewall_rule`'s own doc
/// comment.
#[tauri::command]
pub async fn remove_firewall_custom_rule(
    app_repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    network_repo: State<'_, NodeNetworkRepository>,
    firewall_rule_repo: State<'_, FirewallRuleRepository>,
    sessions: State<'_, SshSessionManager>,
    id: Uuid,
    rule_id: Uuid,
) -> AppResult<()> {
    services::remove_custom_firewall_rule(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, id, rule_id).await
}
