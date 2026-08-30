use tauri::State;
use uuid::Uuid;

use crate::errors::AppResult;
use crate::services;
use crate::state::SshSessionManager;
use crate::storage::server_repository::ServerRepository;
use crate::transport::ServiceSummary;

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
