use tauri::State;
use uuid::Uuid;

use crate::errors::AppResult;
use crate::services;
use crate::state::SshSessionManager;
use crate::storage::server_repository::ServerRepository;
use crate::transport::{ContainerSummary, ServiceSummary};

#[tauri::command]
pub async fn list_server_services(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
) -> AppResult<Vec<ServiceSummary>> {
    services::list_server_services(&repo, &sessions, server_id).await
}

#[tauri::command]
pub async fn restart_server_service(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
    service_name: String,
) -> AppResult<()> {
    services::restart_server_service(&repo, &sessions, server_id, &service_name).await
}

#[tauri::command]
pub async fn start_server_service(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
    service_name: String,
) -> AppResult<()> {
    services::start_server_service(&repo, &sessions, server_id, &service_name).await
}

#[tauri::command]
pub async fn stop_server_service(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
    service_name: String,
) -> AppResult<()> {
    services::stop_server_service(&repo, &sessions, server_id, &service_name).await
}

#[tauri::command]
pub async fn enable_server_service(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
    service_name: String,
) -> AppResult<()> {
    services::enable_server_service(&repo, &sessions, server_id, &service_name).await
}

#[tauri::command]
pub async fn disable_server_service(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
    service_name: String,
) -> AppResult<()> {
    services::disable_server_service(&repo, &sessions, server_id, &service_name).await
}

#[tauri::command]
pub async fn list_server_containers(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
) -> AppResult<Vec<ContainerSummary>> {
    services::list_server_containers(&repo, &sessions, server_id).await
}

#[tauri::command]
pub async fn restart_server_container(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
    container: String,
) -> AppResult<()> {
    services::restart_server_container(&repo, &sessions, server_id, &container).await
}

#[tauri::command]
pub async fn start_server_container(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
    container: String,
) -> AppResult<()> {
    services::start_server_container(&repo, &sessions, server_id, &container).await
}

#[tauri::command]
pub async fn stop_server_container(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
    container: String,
) -> AppResult<()> {
    services::stop_server_container(&repo, &sessions, server_id, &container).await
}

#[tauri::command]
pub async fn remove_server_container(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
    container: String,
) -> AppResult<()> {
    services::remove_server_container(&repo, &sessions, server_id, &container).await
}

#[tauri::command]
pub async fn get_server_container_logs(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
    container: String,
    tail: u32,
) -> AppResult<String> {
    services::server_container_logs(&repo, &sessions, server_id, &container, tail).await
}
