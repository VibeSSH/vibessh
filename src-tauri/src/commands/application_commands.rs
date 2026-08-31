use std::sync::Arc;

use tauri::State;
use uuid::Uuid;

use crate::blueprints::BlueprintRegistry;
use crate::errors::AppResult;
use crate::models::{
    Application, ApplicationDetail, ApplicationPort, ApplicationStatus, Blueprint, CreateApplicationFromBlueprintInput, PortInput,
    SetHealthCheckInput, SetResourceLimitsInput,
};
use crate::runtime::local_process::LocalProcessManager;
use crate::runtime::{HealthStatus, ResourceUsage};
use crate::services::{self, JavaInstallation};
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::node_network_repository::NodeNetworkRepository;
use crate::storage::server_repository::ServerRepository;

#[tauri::command]
pub fn list_applications(repo: State<ApplicationRepository>) -> AppResult<Vec<Application>> {
    services::list_applications(&repo)
}

#[tauri::command]
pub async fn list_paper_versions() -> AppResult<Vec<String>> {
    services::list_paper_versions().await
}

#[tauri::command]
pub async fn list_velocity_versions() -> AppResult<Vec<String>> {
    services::list_velocity_versions().await
}

#[tauri::command]
pub fn get_application(repo: State<ApplicationRepository>, id: Uuid) -> AppResult<ApplicationDetail> {
    services::get_application(&repo, id)
}

#[tauri::command]
pub fn list_blueprints(registry: State<BlueprintRegistry>) -> Vec<Blueprint> {
    services::list_blueprints(&registry)
}

#[tauri::command]
pub fn list_application_ports(repo: State<ApplicationRepository>, id: Uuid) -> AppResult<Vec<ApplicationPort>> {
    services::list_application_ports(&repo, id)
}

#[tauri::command]
pub async fn add_application_port(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    network_repo: State<'_, NodeNetworkRepository>,
    sessions: State<'_, SshSessionManager>,
    id: Uuid,
    port: PortInput,
) -> AppResult<ApplicationPort> {
    services::add_application_port(&repo, &server_repo, &network_repo, &sessions, id, &port).await
}

#[tauri::command]
pub async fn update_application_port(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    network_repo: State<'_, NodeNetworkRepository>,
    sessions: State<'_, SshSessionManager>,
    id: Uuid,
    port_id: Uuid,
    port: PortInput,
) -> AppResult<ApplicationPort> {
    services::update_application_port(&repo, &server_repo, &network_repo, &sessions, id, port_id, &port).await
}

/// "Sync Firewall" (Etap M2, Ports tab) - manually re-applies the current
/// desired rule set (this Application's Node's own SSH port plus every
/// published port across every Application on it), on top of the automatic
/// best-effort sync `add_application_port`/`update_application_port`
/// already trigger. Useful for a port declared before this feature
/// existed, or after a sync that failed the first time (host unreachable,
/// etc.) - always safe to re-run, additive and idempotent.
#[tauri::command]
pub async fn sync_application_node_firewall(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    network_repo: State<'_, NodeNetworkRepository>,
    sessions: State<'_, SshSessionManager>,
    id: Uuid,
) -> AppResult<Option<services::FirewallSyncResult>> {
    services::sync_application_node_firewall(&repo, &server_repo, &network_repo, &sessions, id).await
}

#[tauri::command]
pub fn remove_application_port(repo: State<ApplicationRepository>, id: Uuid, port_id: Uuid) -> AppResult<()> {
    services::remove_application_port(&repo, id, port_id)
}

#[tauri::command]
pub async fn create_application(
    repo: State<'_, ApplicationRepository>,
    registry: State<'_, BlueprintRegistry>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    input: CreateApplicationFromBlueprintInput,
) -> AppResult<ApplicationDetail> {
    services::create_application(&repo, &registry, &server_repo, &sessions, input).await
}

#[tauri::command]
pub fn delete_application(repo: State<ApplicationRepository>, id: Uuid) -> AppResult<()> {
    services::delete_application(&repo, id)
}

#[tauri::command]
pub async fn start_application(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    services::start_application(&repo, &server_repo, &sessions, &local_process_manager, id).await
}

#[tauri::command]
pub async fn stop_application(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    id: Uuid,
    graceful: bool,
) -> AppResult<ApplicationStatus> {
    services::stop_application(&repo, &server_repo, &sessions, &local_process_manager, id, graceful).await
}

#[tauri::command]
pub async fn restart_application(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    services::restart_application(&repo, &server_repo, &sessions, &local_process_manager, id).await
}

/// "Recreate Container" (Etap M1, Docker only) - tears the container down
/// and creates it again from the Application's current config, so an edited
/// image/command/resource limit/restart policy actually takes effect.
#[tauri::command]
pub async fn recreate_application(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    services::recreate_application(&repo, &server_repo, &sessions, &local_process_manager, id).await
}

#[tauri::command]
pub async fn kill_application(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    services::kill_application(&repo, &server_repo, &sessions, &local_process_manager, id).await
}

#[tauri::command]
pub async fn refresh_application_status(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    services::refresh_application_status(&repo, &server_repo, &sessions, &local_process_manager, id).await
}

#[tauri::command]
pub async fn get_application_resource_usage(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    id: Uuid,
) -> AppResult<ResourceUsage> {
    services::application_resource_usage(&repo, &server_repo, &sessions, &local_process_manager, id).await
}

#[tauri::command]
pub async fn get_application_logs(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    id: Uuid,
    max_lines: u32,
) -> AppResult<Vec<String>> {
    services::application_logs(&repo, &server_repo, &sessions, &local_process_manager, id, max_lines).await
}

#[tauri::command]
pub async fn get_application_health(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    id: Uuid,
) -> AppResult<HealthStatus> {
    services::application_health_check(&repo, &server_repo, &sessions, &local_process_manager, id).await
}

#[tauri::command]
pub fn set_application_health_check(
    repo: State<ApplicationRepository>,
    id: Uuid,
    input: SetHealthCheckInput,
) -> AppResult<ApplicationDetail> {
    services::set_application_health_check(&repo, id, input)
}

#[tauri::command]
pub fn set_application_resource_limits(
    repo: State<ApplicationRepository>,
    id: Uuid,
    input: SetResourceLimitsInput,
) -> AppResult<ApplicationDetail> {
    services::set_application_resource_limits(&repo, id, input)
}

#[tauri::command]
pub async fn detect_java_installations(
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Option<Uuid>,
) -> AppResult<Vec<JavaInstallation>> {
    services::detect_java_installations(&server_repo, &sessions, server_id).await
}
