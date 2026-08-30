use std::sync::Arc;

use tauri::State;
use uuid::Uuid;

use crate::blueprints::BlueprintRegistry;
use crate::errors::AppResult;
use crate::models::{Application, ApplicationDetail, ApplicationStatus, Blueprint, CreateApplicationFromBlueprintInput};
use crate::runtime::local_process::LocalProcessManager;
use crate::runtime::ResourceUsage;
use crate::services::{self, JavaInstallation};
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
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
pub fn get_application(repo: State<ApplicationRepository>, id: Uuid) -> AppResult<ApplicationDetail> {
    services::get_application(&repo, id)
}

#[tauri::command]
pub fn list_blueprints(registry: State<BlueprintRegistry>) -> Vec<Blueprint> {
    services::list_blueprints(&registry)
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
pub async fn detect_java_installations(
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Option<Uuid>,
) -> AppResult<Vec<JavaInstallation>> {
    services::detect_java_installations(&server_repo, &sessions, server_id).await
}
