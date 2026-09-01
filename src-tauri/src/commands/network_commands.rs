use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::errors::AppResult;
use crate::models::{DnsRecord, NodeNetworkMember};
use crate::services::{self, MeshReconcileResult, NodeEndpoint, NodeMeshStatus, VibeNetworkSyncResult};
use crate::state::{DnsSuffixState, SshSessionManager};
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::dns_repository::DnsRepository;
use crate::storage::firewall_rule_repository::FirewallRuleRepository;
use crate::storage::node_network_repository::NodeNetworkRepository;
use crate::storage::server_repository::ServerRepository;

#[tauri::command]
pub fn list_network_members(network_repo: State<NodeNetworkRepository>) -> AppResult<Vec<NodeNetworkMember>> {
    services::list_network_members(&network_repo)
}

#[tauri::command]
pub async fn join_vibe_network(
    network_repo: State<'_, NodeNetworkRepository>,
    server_repo: State<'_, ServerRepository>,
    app_repo: State<'_, ApplicationRepository>,
    dns_repo: State<'_, DnsRepository>,
    dns_suffix: State<'_, DnsSuffixState>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
) -> AppResult<NodeNetworkMember> {
    let member = services::join_node(&network_repo, &server_repo, &sessions, server_id).await?;
    // Best-effort: the new Node's own `<name><suffix>` alias (and every
    // existing service alias) becomes reachable to/from it right away,
    // without a separate manual "Synchronizuj" click - a DNS push failing
    // for some reason must never undo a join that otherwise succeeded, same
    // stance `services::migration_service` already takes for the identical
    // call after a migration.
    let _ = services::sync_dns(&dns_suffix.get(), &network_repo, &server_repo, &app_repo, &dns_repo, &sessions).await;
    Ok(member)
}

#[tauri::command]
pub async fn leave_vibe_network(
    network_repo: State<'_, NodeNetworkRepository>,
    server_repo: State<'_, ServerRepository>,
    app_repo: State<'_, ApplicationRepository>,
    dns_repo: State<'_, DnsRepository>,
    dns_suffix: State<'_, DnsSuffixState>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
) -> AppResult<()> {
    services::leave_node(&network_repo, &server_repo, &sessions, server_id).await?;
    // Best-effort, same reasoning as `join_vibe_network` - cleans the
    // departed Node's alias out of every remaining member's view.
    let _ = services::sync_dns(&dns_suffix.get(), &network_repo, &server_repo, &app_repo, &dns_repo, &sessions).await;
    Ok(())
}

#[tauri::command]
pub async fn reconcile_vibe_mesh(
    network_repo: State<'_, NodeNetworkRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
) -> AppResult<Vec<MeshReconcileResult>> {
    services::reconcile_mesh(&network_repo, &server_repo, &sessions).await
}

#[tauri::command]
pub async fn get_vibe_network_status(
    network_repo: State<'_, NodeNetworkRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
) -> AppResult<Vec<NodeMeshStatus>> {
    services::mesh_status(&network_repo, &server_repo, &sessions).await
}

#[tauri::command]
pub fn list_node_endpoints(app_repo: State<ApplicationRepository>, server_id: Uuid) -> AppResult<Vec<NodeEndpoint>> {
    services::list_node_endpoints(&app_repo, server_id)
}

#[tauri::command]
pub fn list_dns_records(dns_repo: State<DnsRepository>) -> AppResult<Vec<DnsRecord>> {
    services::list_dns_records(&dns_repo)
}

/// Creates the alias, then immediately pushes a full `sync_dns` to every
/// mesh member - the whole point of an alias is for it to actually resolve,
/// so unlike `join_vibe_network`'s best-effort push, a sync failure here is
/// real information the caller gets back (`DnsAliasWithSync::sync_results`),
/// not swallowed. The alias itself still saves regardless of whether the
/// sync that follows fully succeeds - a Node being briefly unreachable
/// must never make the whole "add an alias" action fail.
#[tauri::command]
pub async fn create_dns_alias(
    network_repo: State<'_, NodeNetworkRepository>,
    server_repo: State<'_, ServerRepository>,
    app_repo: State<'_, ApplicationRepository>,
    dns_repo: State<'_, DnsRepository>,
    dns_suffix: State<'_, DnsSuffixState>,
    sessions: State<'_, SshSessionManager>,
    application_id: Uuid,
    hostname: String,
) -> AppResult<services::DnsAliasWithSync> {
    let suffix = dns_suffix.get();
    let alias = services::create_dns_alias(&suffix, &dns_repo, application_id, &hostname)?;
    let sync_results = services::sync_dns(&suffix, &network_repo, &server_repo, &app_repo, &dns_repo, &sessions).await?;
    Ok(services::DnsAliasWithSync { alias: Some(alias), sync_results })
}

#[tauri::command]
pub async fn update_dns_alias(
    network_repo: State<'_, NodeNetworkRepository>,
    server_repo: State<'_, ServerRepository>,
    app_repo: State<'_, ApplicationRepository>,
    dns_repo: State<'_, DnsRepository>,
    dns_suffix: State<'_, DnsSuffixState>,
    sessions: State<'_, SshSessionManager>,
    id: Uuid,
    hostname: String,
) -> AppResult<services::DnsAliasWithSync> {
    let suffix = dns_suffix.get();
    let alias = services::update_dns_alias(&suffix, &dns_repo, id, &hostname)?;
    let sync_results = services::sync_dns(&suffix, &network_repo, &server_repo, &app_repo, &dns_repo, &sessions).await?;
    Ok(services::DnsAliasWithSync { alias: Some(alias), sync_results })
}

#[tauri::command]
pub async fn delete_dns_alias(
    network_repo: State<'_, NodeNetworkRepository>,
    server_repo: State<'_, ServerRepository>,
    app_repo: State<'_, ApplicationRepository>,
    dns_repo: State<'_, DnsRepository>,
    dns_suffix: State<'_, DnsSuffixState>,
    sessions: State<'_, SshSessionManager>,
    id: Uuid,
) -> AppResult<Vec<services::DnsSyncResult>> {
    services::delete_dns_alias(&dns_repo, id)?;
    services::sync_dns(&dns_suffix.get(), &network_repo, &server_repo, &app_repo, &dns_repo, &sessions).await
}

#[tauri::command]
pub async fn sync_vibe_dns(
    network_repo: State<'_, NodeNetworkRepository>,
    server_repo: State<'_, ServerRepository>,
    app_repo: State<'_, ApplicationRepository>,
    dns_repo: State<'_, DnsRepository>,
    dns_suffix: State<'_, DnsSuffixState>,
    sessions: State<'_, SshSessionManager>,
) -> AppResult<Vec<services::DnsSyncResult>> {
    services::sync_dns(&dns_suffix.get(), &network_repo, &server_repo, &app_repo, &dns_repo, &sessions).await
}

/// A real check: SSHes into a live mesh member and asks it to resolve
/// `hostname` itself via `getent hosts`, comparing against `expectedIp` -
/// see `services::dns_service::verify_alias`'s own doc comment.
#[tauri::command]
pub async fn verify_dns_alias(
    network_repo: State<'_, NodeNetworkRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    hostname: String,
    expected_ip: String,
) -> AppResult<bool> {
    services::verify_dns_alias(&network_repo, &server_repo, &sessions, &hostname, &expected_ip).await
}

#[tauri::command]
pub fn resolve_dns_view(
    network_repo: State<NodeNetworkRepository>,
    server_repo: State<ServerRepository>,
    app_repo: State<ApplicationRepository>,
    dns_repo: State<DnsRepository>,
    dns_suffix: State<DnsSuffixState>,
) -> AppResult<Vec<crate::models::DnsView>> {
    services::resolve_dns_view(&dns_suffix.get(), &network_repo, &server_repo, &app_repo, &dns_repo)
}

/// "Synchronizuj Vibe Network" - the one combined action the spec asks for:
/// WireGuard peers + firewall + DNS, per-Node OK/OUT OF SYNC.
#[tauri::command]
pub async fn sync_vibe_network(
    network_repo: State<'_, NodeNetworkRepository>,
    server_repo: State<'_, ServerRepository>,
    app_repo: State<'_, ApplicationRepository>,
    dns_repo: State<'_, DnsRepository>,
    dns_suffix: State<'_, DnsSuffixState>,
    firewall_rule_repo: State<'_, FirewallRuleRepository>,
    sessions: State<'_, SshSessionManager>,
) -> AppResult<Vec<VibeNetworkSyncResult>> {
    services::sync_vibe_network(&network_repo, &server_repo, &app_repo, &dns_repo, &dns_suffix.get(), &firewall_rule_repo, &sessions).await
}

/// The Settings page's own read of the Vibe Network's DNS suffix - see
/// `services::dns_service`'s own doc comment for why this is a global,
/// per-install setting, not per-Node/per-Application.
#[tauri::command]
pub fn get_dns_suffix(dns_suffix: State<DnsSuffixState>) -> String {
    services::get_dns_suffix(&dns_suffix)
}

#[tauri::command]
pub fn set_dns_suffix(app: AppHandle, dns_suffix: State<DnsSuffixState>, suffix: String) -> AppResult<String> {
    let config_dir = app.path().app_config_dir().map_err(|err| crate::errors::AppError::Storage(format!("couldn't resolve the app config directory: {err}")))?;
    services::set_dns_suffix(&dns_suffix, &config_dir, &suffix)
}
