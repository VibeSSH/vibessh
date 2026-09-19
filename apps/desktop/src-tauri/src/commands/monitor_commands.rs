use tauri::State;
use uuid::Uuid;

use crate::errors::AppResult;
use crate::services;
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
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

#[tauri::command]
pub async fn get_minecraft_metrics(
    app_repo: State<'_, ApplicationRepository>,
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    application_id: Uuid,
    rcon_port: u16,
) -> AppResult<vibessh_protocol::MinecraftMetrics> {
    services::get_minecraft_metrics(&app_repo, &repo, &sessions, application_id, rcon_port).await
}

/// Stores the RCON password in the OS keyring. Kept off every read path and
/// out of any config file - the frontend sends it here once and never reads
/// it back.
#[tauri::command]
pub async fn set_minecraft_rcon_password(application_id: Uuid, password: String) -> AppResult<()> {
    services::set_rcon_password(application_id, &password)
}
