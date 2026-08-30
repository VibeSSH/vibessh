use tauri::State;
use uuid::Uuid;

use crate::errors::AppResult;
use crate::services;
use crate::state::SshSessionManager;
use crate::storage::server_repository::ServerRepository;
use crate::transport::RemoteFileEntry;

#[tauri::command]
pub async fn list_remote_directory(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
    path: String,
) -> AppResult<Vec<RemoteFileEntry>> {
    services::list_remote_directory(&repo, &sessions, server_id, &path).await
}

#[tauri::command]
pub async fn read_remote_file(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
    path: String,
) -> AppResult<Vec<u8>> {
    services::read_remote_file(&repo, &sessions, server_id, &path).await
}

#[tauri::command]
pub async fn write_remote_file(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
    path: String,
    contents: Vec<u8>,
) -> AppResult<()> {
    services::write_remote_file(&repo, &sessions, server_id, &path, &contents).await
}
