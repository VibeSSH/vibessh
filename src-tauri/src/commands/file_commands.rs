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

/// Covers both "Rename" (same parent, new name) and "Move" (new parent) -
/// see `ApplicationFileProvider::rename`'s own doc comment for why there's
/// only one primitive for both; the frontend builds a different `to` for
/// each.
#[tauri::command]
pub async fn rename_remote_path(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
    from: String,
    to: String,
) -> AppResult<()> {
    services::rename_remote_path(&repo, &sessions, server_id, &from, &to).await
}

/// Recursive for a directory.
#[tauri::command]
pub async fn delete_remote_path(repo: State<'_, ServerRepository>, sessions: State<'_, SshSessionManager>, server_id: Uuid, path: String) -> AppResult<()> {
    services::delete_remote_path(&repo, &sessions, server_id, &path).await
}

#[tauri::command]
pub async fn set_remote_permissions(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
    path: String,
    mode: u32,
) -> AppResult<()> {
    services::set_remote_permissions(&repo, &sessions, server_id, &path, mode).await
}

/// Extracts an already-uploaded `.zip` at `archive_path` into `destination`
/// - returns the number of files written.
#[tauri::command]
pub async fn extract_remote_archive(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
    archive_path: String,
    destination: String,
) -> AppResult<u32> {
    services::extract_remote_archive(&repo, &sessions, server_id, &archive_path, &destination).await
}

/// Compresses `paths` into a new `.zip` written to `destination_path`.
#[tauri::command]
pub async fn compress_remote_paths(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
    paths: Vec<String>,
    destination_path: String,
) -> AppResult<()> {
    services::compress_remote_paths(&repo, &sessions, server_id, &paths, &destination_path).await
}
