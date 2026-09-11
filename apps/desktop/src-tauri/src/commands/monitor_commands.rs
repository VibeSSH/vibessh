use tauri::State;
use uuid::Uuid;

use crate::errors::AppResult;
use crate::services;
use crate::state::SshSessionManager;
use crate::storage::server_repository::ServerRepository;
use crate::transport::{ProcessSummary, ServerMetrics};

#[tauri::command]
pub async fn get_server_metrics(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
) -> AppResult<ServerMetrics> {
    services::get_server_metrics(&repo, &sessions, server_id).await
}

#[tauri::command]
pub async fn list_server_processes(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
) -> AppResult<Vec<ProcessSummary>> {
    services::list_server_processes(&repo, &sessions, server_id).await
}
