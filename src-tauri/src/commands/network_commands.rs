use tauri::State;
use uuid::Uuid;

use crate::errors::AppResult;
use crate::models::{DnsRecord, NodeNetworkMember};
use crate::services::{self, MeshReconcileResult, NodeEndpoint, NodeMeshStatus, VibeNetworkSyncResult};
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::dns_repository::DnsRepository;
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
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
) -> AppResult<NodeNetworkMember> {
    services::join_node(&network_repo, &server_repo, &sessions, server_id).await
}

#[tauri::command]
pub async fn leave_vibe_network(
    network_repo: State<'_, NodeNetworkRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
) -> AppResult<()> {
    services::leave_node(&network_repo, &server_repo, &sessions, server_id).await
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

#[tauri::command]
pub fn create_dns_alias(dns_repo: State<DnsRepository>, application_id: Uuid, hostname: String) -> AppResult<DnsRecord> {
    services::create_dns_alias(&dns_repo, application_id, &hostname)
}

#[tauri::command]
pub fn update_dns_alias(dns_repo: State<DnsRepository>, id: Uuid, hostname: String) -> AppResult<DnsRecord> {
    services::update_dns_alias(&dns_repo, id, &hostname)
}

#[tauri::command]
pub fn delete_dns_alias(dns_repo: State<DnsRepository>, id: Uuid) -> AppResult<()> {
    services::delete_dns_alias(&dns_repo, id)
}

#[tauri::command]
pub async fn sync_vibe_dns(
    network_repo: State<'_, NodeNetworkRepository>,
    server_repo: State<'_, ServerRepository>,
    app_repo: State<'_, ApplicationRepository>,
    dns_repo: State<'_, DnsRepository>,
    sessions: State<'_, SshSessionManager>,
) -> AppResult<Vec<services::DnsSyncResult>> {
    services::sync_dns(&network_repo, &server_repo, &app_repo, &dns_repo, &sessions).await
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
) -> AppResult<Vec<crate::models::DnsView>> {
    services::resolve_dns_view(&network_repo, &server_repo, &app_repo, &dns_repo)
}

/// "Synchronizuj Vibe Network" - the one combined action the spec asks for:
/// WireGuard peers + firewall + DNS, per-Node OK/OUT OF SYNC.
#[tauri::command]
pub async fn sync_vibe_network(
    network_repo: State<'_, NodeNetworkRepository>,
    server_repo: State<'_, ServerRepository>,
    app_repo: State<'_, ApplicationRepository>,
    dns_repo: State<'_, DnsRepository>,
    sessions: State<'_, SshSessionManager>,
) -> AppResult<Vec<VibeNetworkSyncResult>> {
    services::sync_vibe_network(&network_repo, &server_repo, &app_repo, &dns_repo, &sessions).await
}
