use std::sync::Arc;

use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::errors::AppResult;
use crate::models::{ApplicationBackup, BackupDestinationConfig, BackupKind, BackupSchedule, SetBackupDestinationInput, SetBackupScheduleInput};
use crate::runtime::local_process::LocalProcessManager;
use crate::services;
use crate::state::{BackupDestinationState, SshSessionManager};
use crate::storage::application_backup_repository::ApplicationBackupRepository;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::server_repository::ServerRepository;

#[tauri::command]
pub async fn list_application_backups(backup_repo: State<'_, ApplicationBackupRepository>, id: Uuid) -> AppResult<Vec<ApplicationBackup>> {
    services::list_backups(&backup_repo, id).await
}

#[tauri::command]
pub async fn create_application_backup(
    repo: State<'_, ApplicationRepository>,
    backup_repo: State<'_, ApplicationBackupRepository>,
    server_repo: State<'_, ServerRepository>,
    backup_destination: State<'_, BackupDestinationState>,
    sessions: State<'_, SshSessionManager>,
    id: Uuid,
) -> AppResult<ApplicationBackup> {
    services::create_backup(&repo, &backup_repo, &server_repo, &backup_destination, &sessions, id, BackupKind::Manual).await
}

#[tauri::command]
pub async fn delete_application_backup(
    repo: State<'_, ApplicationRepository>,
    backup_repo: State<'_, ApplicationBackupRepository>,
    server_repo: State<'_, ServerRepository>,
    backup_destination: State<'_, BackupDestinationState>,
    sessions: State<'_, SshSessionManager>,
    id: Uuid,
    backup_id: Uuid,
) -> AppResult<()> {
    services::delete_backup(&repo, &backup_repo, &server_repo, &backup_destination, &sessions, id, backup_id).await
}

#[tauri::command]
pub async fn restore_application_backup(
    repo: State<'_, ApplicationRepository>,
    backup_repo: State<'_, ApplicationBackupRepository>,
    server_repo: State<'_, ServerRepository>,
    backup_destination: State<'_, BackupDestinationState>,
    sessions: State<'_, SshSessionManager>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    id: Uuid,
    backup_id: Uuid,
) -> AppResult<u32> {
    services::restore_backup(&repo, &backup_repo, &server_repo, &backup_destination, &sessions, &local_process_manager, id, backup_id).await
}

#[tauri::command]
pub fn get_application_backup_schedule(backup_repo: State<ApplicationBackupRepository>, id: Uuid) -> AppResult<BackupSchedule> {
    services::get_backup_schedule(&backup_repo, id)
}

#[tauri::command]
pub fn set_application_backup_schedule(
    backup_repo: State<ApplicationBackupRepository>,
    id: Uuid,
    input: SetBackupScheduleInput,
) -> AppResult<BackupSchedule> {
    services::set_backup_schedule(&backup_repo, id, input)
}

/// Called by the frontend on a timer, not user-triggered - see
/// `services::application_backup_service`'s own doc comment for why the
/// "schedule" lives entirely on this side of the IPC boundary.
#[tauri::command]
pub async fn run_due_application_backups(
    repo: State<'_, ApplicationRepository>,
    backup_repo: State<'_, ApplicationBackupRepository>,
    server_repo: State<'_, ServerRepository>,
    backup_destination: State<'_, BackupDestinationState>,
    sessions: State<'_, SshSessionManager>,
) -> AppResult<u32> {
    services::run_due_backups(&repo, &backup_repo, &server_repo, &backup_destination, &sessions).await
}

/// The Settings page's own read of the S3-compatible backup destination -
/// never includes the secret access key (see `models::BackupDestinationConfig`'s
/// own doc comment), only whether one is currently stored isn't needed
/// here since the frontend already knows from `enabled` whether a working
/// destination should exist.
#[tauri::command]
pub async fn get_backup_destination(backup_destination: State<'_, BackupDestinationState>) -> AppResult<BackupDestinationConfig> {
    Ok(services::get_backup_destination(&backup_destination).await)
}

#[tauri::command]
pub async fn set_backup_destination(
    app: AppHandle,
    backup_destination: State<'_, BackupDestinationState>,
    input: SetBackupDestinationInput,
) -> AppResult<BackupDestinationConfig> {
    let config_dir = app.path().app_config_dir().map_err(|err| crate::errors::AppError::Storage(format!("couldn't resolve the app config directory: {err}")))?;
    services::set_backup_destination(&backup_destination, &config_dir, input).await
}

#[tauri::command]
pub async fn test_backup_destination(backup_destination: State<'_, BackupDestinationState>) -> AppResult<()> {
    services::test_backup_destination(&backup_destination).await
}
