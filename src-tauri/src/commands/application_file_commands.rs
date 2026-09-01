//! Tauri command bridge for the per-Application Files tab
//! (`services::application_files_service`) - entirely separate from
//! `commands::file_commands`, which is the older, host-wide Node Files
//! module. Upload/download report progress through a per-transfer event
//! pair (`application-files://{transfer_id}/progress`), the same "one
//! event channel per in-flight thing, not one shared name" convention
//! `commands::terminal_commands` already established for terminals -
//! `transfer_id` is chosen by the frontend (its own transfer-queue row id),
//! not generated here.

use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::services::{self, FileHistoryVersion};
use crate::state::{FileTransferManager, SshSessionManager};
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::server_repository::ServerRepository;
use vibessh_protocol::RemoteFileEntry;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TransferProgress {
    transferred: u64,
    total: u64,
}

#[tauri::command]
pub async fn list_application_files(
    app_repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    application_id: Uuid,
    path: String,
) -> AppResult<Vec<RemoteFileEntry>> {
    services::list_application_files(&app_repo, &server_repo, &sessions, application_id, &path).await
}

#[tauri::command]
pub async fn get_application_file_metadata(
    app_repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    application_id: Uuid,
    path: String,
) -> AppResult<RemoteFileEntry> {
    services::get_application_file_metadata(&app_repo, &server_repo, &sessions, application_id, &path).await
}

/// For the editor - rejects a file over `application_files_service::
/// MAX_EDITABLE_FILE_SIZE` rather than loading it into memory.
#[tauri::command]
pub async fn read_application_file(
    app_repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    application_id: Uuid,
    path: String,
) -> AppResult<Vec<u8>> {
    services::read_application_file(&app_repo, &server_repo, &sessions, application_id, &path).await
}

/// Plain create-or-truncate write - "New File" and similar, not the
/// editor's own Save (see `save_application_file`).
#[tauri::command]
pub async fn write_application_file(
    app_repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    application_id: Uuid,
    path: String,
    contents: Vec<u8>,
) -> AppResult<()> {
    services::write_application_file(&app_repo, &server_repo, &sessions, application_id, &path, &contents).await
}

#[tauri::command]
pub async fn save_application_file(
    app_repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    application_id: Uuid,
    path: String,
    contents: Vec<u8>,
    backup: bool,
) -> AppResult<()> {
    services::save_application_file(&app_repo, &server_repo, &sessions, application_id, &path, &contents, backup).await
}

#[tauri::command]
pub async fn create_application_directory(
    app_repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    application_id: Uuid,
    path: String,
) -> AppResult<()> {
    services::create_application_directory(&app_repo, &server_repo, &sessions, application_id, &path).await
}

#[tauri::command]
pub async fn delete_application_file(
    app_repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    application_id: Uuid,
    path: String,
) -> AppResult<()> {
    services::delete_application_file(&app_repo, &server_repo, &sessions, application_id, &path).await
}

/// Covers both "Rename" and "Move" - see `files::ApplicationFileProvider::
/// rename`'s own doc comment for why.
#[tauri::command]
pub async fn rename_application_file(
    app_repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    application_id: Uuid,
    from: String,
    to: String,
) -> AppResult<()> {
    services::rename_application_file(&app_repo, &server_repo, &sessions, application_id, &from, &to).await
}

#[tauri::command]
pub async fn copy_application_file(
    app_repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    application_id: Uuid,
    from: String,
    to: String,
) -> AppResult<()> {
    services::copy_application_file(&app_repo, &server_repo, &sessions, application_id, &from, &to).await
}

#[tauri::command]
pub async fn set_application_file_permissions(
    app_repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    application_id: Uuid,
    path: String,
    mode: u32,
) -> AppResult<()> {
    services::set_application_file_permissions(&app_repo, &server_repo, &sessions, application_id, &path, mode).await
}

/// `local_dest` is a real filesystem path already chosen by the frontend
/// (the native save dialog, or a drag-out target) - never raw bytes handed
/// across the IPC boundary. Progress streams as
/// `application-files://{transfer_id}/progress` events while this command's
/// own `await` is still pending; `transfer_id` is the frontend's own
/// transfer-queue row id, not generated here, so it can be wired to
/// `cancel_application_file_transfer` even before this command resolves.
#[tauri::command]
pub async fn download_application_file(
    app: AppHandle,
    transfers: State<'_, FileTransferManager>,
    application_id: Uuid,
    path: String,
    local_dest: String,
    transfer_id: String,
) -> AppResult<()> {
    let app_for_task = app.clone();
    let progress_event = format!("application-files://{transfer_id}/progress");
    let local_dest = std::path::PathBuf::from(local_dest);

    let task = tokio::spawn(async move {
        let app_repo = app_for_task.state::<ApplicationRepository>();
        let server_repo = app_for_task.state::<ServerRepository>();
        let sessions = app_for_task.state::<SshSessionManager>();
        let app_for_emit = app_for_task.clone();
        services::download_application_file(&app_repo, &server_repo, &sessions, application_id, &path, &local_dest, move |transferred, total| {
            let _ = app_for_emit.emit(&progress_event, TransferProgress { transferred, total });
        })
        .await
    });

    transfers.register(transfer_id.clone(), task.abort_handle()).await;
    let result = join_transfer(task).await;
    transfers.clear(&transfer_id).await;
    result
}

/// The upload counterpart of `download_application_file` - same real-path,
/// same progress-event, same cancel-via-`transfer_id` shape.
#[tauri::command]
pub async fn upload_application_file(
    app: AppHandle,
    transfers: State<'_, FileTransferManager>,
    application_id: Uuid,
    local_src: String,
    path: String,
    transfer_id: String,
) -> AppResult<()> {
    let app_for_task = app.clone();
    let progress_event = format!("application-files://{transfer_id}/progress");
    let local_src = std::path::PathBuf::from(local_src);

    let task = tokio::spawn(async move {
        let app_repo = app_for_task.state::<ApplicationRepository>();
        let server_repo = app_for_task.state::<ServerRepository>();
        let sessions = app_for_task.state::<SshSessionManager>();
        let app_for_emit = app_for_task.clone();
        services::upload_application_file(&app_repo, &server_repo, &sessions, application_id, &local_src, &path, move |transferred, total| {
            let _ = app_for_emit.emit(&progress_event, TransferProgress { transferred, total });
        })
        .await
    });

    transfers.register(transfer_id.clone(), task.abort_handle()).await;
    let result = join_transfer(task).await;
    transfers.clear(&transfer_id).await;
    result
}

async fn join_transfer(task: tokio::task::JoinHandle<AppResult<()>>) -> AppResult<()> {
    match task.await {
        Ok(result) => result,
        Err(join_err) if join_err.is_cancelled() => Err(AppError::InvalidInput("the transfer was canceled".into())),
        Err(join_err) => Err(AppError::Internal(format!("the transfer task failed: {join_err}"))),
    }
}

/// `false` (not an error) when nothing was actually running under this id -
/// already finished, or never existed.
#[tauri::command]
pub async fn cancel_application_file_transfer(transfers: State<'_, FileTransferManager>, transfer_id: String) -> AppResult<bool> {
    Ok(transfers.cancel(&transfer_id).await)
}

#[tauri::command]
pub async fn extract_application_archive(
    app_repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    application_id: Uuid,
    source_path: String,
    destination: String,
) -> AppResult<u32> {
    services::extract_application_archive(&app_repo, &server_repo, &sessions, application_id, &source_path, &destination).await
}

#[tauri::command]
pub async fn list_application_file_history(
    app_repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    application_id: Uuid,
    path: String,
) -> AppResult<Vec<FileHistoryVersion>> {
    services::list_file_history(&app_repo, &server_repo, &sessions, application_id, &path).await
}

#[tauri::command]
pub async fn restore_application_file_history(
    app_repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    application_id: Uuid,
    path: String,
    timestamp: String,
) -> AppResult<()> {
    services::restore_file_history(&app_repo, &server_repo, &sessions, application_id, &path, &timestamp).await
}

#[tauri::command]
pub async fn clear_application_file_history(
    app_repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    application_id: Uuid,
    path: String,
) -> AppResult<()> {
    services::clear_file_history(&app_repo, &server_repo, &sessions, application_id, &path).await
}
