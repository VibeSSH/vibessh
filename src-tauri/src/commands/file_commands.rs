use std::path::PathBuf;

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
pub async fn create_remote_directory(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
    path: String,
) -> AppResult<()> {
    services::create_remote_directory(&repo, &sessions, server_id, &path).await
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

/// `local_path` comes from a native save-file dialog the frontend already
/// ran, so it's a real, user-chosen filesystem path - never a value that
/// needs shell-safety validation the way a remote service/container name
/// does.
#[tauri::command]
pub async fn download_remote_file(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
    remote_path: String,
    local_path: PathBuf,
) -> AppResult<()> {
    services::download_remote_file(&repo, &sessions, server_id, &remote_path, &local_path).await
}

#[tauri::command]
pub async fn upload_remote_file(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
    local_path: PathBuf,
    remote_path: String,
) -> AppResult<()> {
    services::upload_remote_file(&repo, &sessions, server_id, &local_path, &remote_path).await
}
