//! Application backups - a manual "back up now" plus an optional interval
//! schedule, both producing the exact same kind of artifact: a `.zip` of
//! the Application's working directory (minus VibeSSH's own bookkeeping
//! files), written into `.vibessh-backups/` inside that same directory
//! through the Application's own `ApplicationFileProvider` - see
//! `storage::migrations`'s doc comment on why the two backing tables are
//! metadata-only.
//!
//! **The "schedule" has no backend timer.** This app has no long-running
//! daemon/service process the way a server does - `run_due_backups` is a
//! plain function the *frontend* calls on its own timer (same "polling
//! lives in React, not in a spawned Tokio task" convention every other
//! periodic check in this codebase already uses, e.g. `ApplicationDetail`'s
//! own `POLL_INTERVAL_MS`). A schedule only actually produces backups while
//! VibeSSH is open and that timer is running - stated here rather than left
//! for someone to discover the hard way.

use std::path::Path;
use std::sync::Arc;

use chrono::Utc;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::files::archive;
use crate::models::{ApplicationBackup, ApplicationStatus, BackupDestinationConfig, BackupKind, BackupSchedule, SetBackupDestinationInput, SetBackupScheduleInput};
use crate::runtime::local_process::LocalProcessManager;
use crate::s3::S3Client;
use crate::services::application_files_service::resolve_provider;
use crate::services::application_service::refresh_application_status;
use crate::state::{BackupDestinationState, SshSessionManager};
use crate::storage::application_backup_repository::ApplicationBackupRepository;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::server_repository::ServerRepository;
use crate::storage::{backup_destination_config, credentials};

const BACKUPS_DIR: &str = ".vibessh-backups";

fn backup_file_name(kind: BackupKind, at: chrono::DateTime<Utc>) -> String {
    format!("{}-{}.zip", kind.as_str(), at.format("%Y%m%d-%H%M%S"))
}

/// The object key a backup is uploaded under - `{application_id}/{file_name}`,
/// unprefixed (the destination's own `path_prefix` is applied by
/// `S3Client::prefixed_key`, not here) so the same key is stable
/// regardless of which prefix is configured at upload time vs. later.
fn s3_object_key(application_id: Uuid, file_name: &str) -> String {
    format!("{application_id}/{file_name}")
}

pub async fn get_backup_destination(state: &BackupDestinationState) -> BackupDestinationConfig {
    state.get().await
}

/// Persists the new config (file for the non-secret fields, OS keyring for
/// the secret) and updates the live state every backup call reads from -
/// see `state::BackupDestinationState`'s own doc comment for why those are
/// two separate steps rather than one. `secret_access_key` blank means
/// "keep the current secret" (see `SetBackupDestinationInput`'s own doc
/// comment) - if there's no existing secret to keep and one is required
/// (`enabled: true`), this rejects rather than silently enabling with no
/// credentials.
pub async fn set_backup_destination(
    state: &BackupDestinationState,
    config_dir: &Path,
    input: SetBackupDestinationInput,
) -> AppResult<BackupDestinationConfig> {
    if input.enabled {
        if input.endpoint.trim().is_empty() || input.bucket.trim().is_empty() || input.access_key_id.trim().is_empty() {
            return Err(AppError::InvalidInput("endpoint, bucket, and access key ID are all required to enable a backup destination".into()));
        }
        if !input.endpoint.trim().starts_with("http://") && !input.endpoint.trim().starts_with("https://") {
            return Err(AppError::InvalidInput("the endpoint must start with http:// or https://".into()));
        }
    }

    if !input.secret_access_key.is_empty() {
        credentials::store_backup_destination_secret(&input.secret_access_key)?;
    } else if input.enabled && credentials::load_backup_destination_secret()?.is_none() {
        return Err(AppError::InvalidInput("a secret access key is required to enable a backup destination".into()));
    }

    let config = BackupDestinationConfig {
        enabled: input.enabled,
        endpoint: input.endpoint.trim().to_string(),
        region: input.region.trim().to_string(),
        bucket: input.bucket.trim().to_string(),
        access_key_id: input.access_key_id.trim().to_string(),
        path_prefix: input.path_prefix.trim().to_string(),
        path_style: input.path_style,
    };
    backup_destination_config::save_backup_destination(config_dir, &config)?;
    state.set(config.clone()).await;
    Ok(config)
}

/// A small round-trip (upload then delete one marker object) against the
/// *currently saved* destination - lets the user find out their
/// endpoint/bucket/credentials are wrong from the Settings page itself,
/// rather than from a scheduled backup silently falling back to local-only
/// hours later.
pub async fn test_backup_destination(state: &BackupDestinationState) -> AppResult<()> {
    let config = state.get().await;
    if !config.enabled {
        return Err(AppError::InvalidInput("the backup destination isn't enabled".into()));
    }
    let secret = credentials::load_backup_destination_secret()?.ok_or_else(|| AppError::InvalidInput("no secret access key is stored".into()))?;
    let client = S3Client::new(config, secret);
    client.put_object(".vibessh-connection-test", b"ok").await?;
    client.delete_object(".vibessh-connection-test").await
}

/// `None` when no destination is configured/enabled, or its secret isn't
/// stored - every caller treats that as "S3 upload/download/delete isn't
/// available right now," never as an error, since a backup destination is
/// always optional (see `models::BackupDestinationConfig`'s own doc
/// comment).
async fn s3_client(backup_destination: &BackupDestinationState) -> Option<S3Client> {
    let config = backup_destination.get().await;
    if !config.enabled {
        return None;
    }
    let secret = credentials::load_backup_destination_secret().ok().flatten()?;
    Some(S3Client::new(config, secret))
}

pub async fn list_backups(backup_repo: &ApplicationBackupRepository, application_id: Uuid) -> AppResult<Vec<ApplicationBackup>> {
    backup_repo.list(application_id)
}

pub async fn create_backup(
    app_repo: &ApplicationRepository,
    backup_repo: &ApplicationBackupRepository,
    server_repo: &ServerRepository,
    backup_destination: &BackupDestinationState,
    sessions: &SshSessionManager,
    application_id: Uuid,
    kind: BackupKind,
) -> AppResult<ApplicationBackup> {
    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;

    // Everything at the working directory's own root, except VibeSSH's own
    // bookkeeping (the backups folder itself, and Remote Process's
    // `.vibessh-app-<id>.{pid,log,stdin}`) - a backup of a backup folder
    // would nest forever, and the pid/log/fifo files are meaningless
    // outside the exact process that wrote them.
    let entries = provider.list_directory(".").await?;
    let paths: Vec<String> = entries.iter().filter(|e| !e.name.starts_with(".vibessh-")).map(|e| e.name.clone()).collect();

    let _ = provider.create_directory(BACKUPS_DIR).await;

    let now = Utc::now();
    let file_name = backup_file_name(kind, now);
    let destination = format!("{BACKUPS_DIR}/{file_name}");

    archive::create_zip(provider.as_ref(), &paths, &destination).await?;
    let size_bytes = provider.metadata(&destination).await?.size;

    let created = backup_repo.create(application_id, &file_name, size_bytes, kind)?;

    // Best-effort, on top of the local copy that already exists either way
    // (see `models::ApplicationBackup::s3_key`'s own doc comment for why a
    // failed/skipped upload here never fails the backup itself - the whole
    // point of a *local* backup succeeding is that it doesn't depend on a
    // remote destination being reachable right now).
    if let Some(client) = s3_client(backup_destination).await {
        let key = s3_object_key(application_id, &file_name);
        match provider.read_file(&destination).await {
            Ok(bytes) => match client.put_object(&key, &bytes).await {
                Ok(()) => {
                    if let Err(err) = backup_repo.set_s3_key(created.id, &key) {
                        log::warn!("backup {} uploaded to S3 but couldn't record its key: {err}", created.id);
                    }
                }
                Err(err) => log::warn!("backup {} failed to upload to the configured backup destination: {err}", created.id),
            },
            Err(err) => log::warn!("backup {} couldn't be read back for S3 upload: {err}", created.id),
        }
    }

    Ok(created)
}

pub async fn delete_backup(
    app_repo: &ApplicationRepository,
    backup_repo: &ApplicationBackupRepository,
    server_repo: &ServerRepository,
    backup_destination: &BackupDestinationState,
    sessions: &SshSessionManager,
    application_id: Uuid,
    backup_id: Uuid,
) -> AppResult<()> {
    // Read before delete - `ApplicationBackupRepository::delete` only ever
    // returns the file name (see its own doc comment), and the S3 key is
    // needed here too to also remove the remote copy, if there is one.
    let s3_key = backup_repo.get(backup_id)?.and_then(|backup| backup.s3_key);
    let Some(file_name) = backup_repo.delete(backup_id)? else {
        return Ok(());
    };
    // Best-effort on the actual file - the DB row (the source of truth for
    // what the UI lists) is already gone either way, and a stale zip left
    // behind on disk after a failed delete is a much smaller problem than
    // a delete the user can never complete because e.g. the Node is
    // temporarily unreachable.
    if let Ok((_, provider)) = resolve_provider(app_repo, server_repo, sessions, application_id).await {
        let _ = provider.delete(&format!("{BACKUPS_DIR}/{file_name}")).await;
    }
    if let Some(key) = s3_key {
        if let Some(client) = s3_client(backup_destination).await {
            let _ = client.delete_object(&key).await;
        }
    }
    Ok(())
}

/// Requires the application to be stopped first - extracting on top of
/// files a running process has open/is actively writing is how you get a
/// half-restored, corrupted working directory, not a clean rollback.
pub async fn restore_backup(
    app_repo: &ApplicationRepository,
    backup_repo: &ApplicationBackupRepository,
    server_repo: &ServerRepository,
    backup_destination: &BackupDestinationState,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    application_id: Uuid,
    backup_id: Uuid,
) -> AppResult<u32> {
    let backup = backup_repo.get(backup_id)?.ok_or_else(|| AppError::NotFound(format!("backup {backup_id}")))?;
    if backup.application_id != application_id {
        return Err(AppError::NotFound(format!("backup {backup_id}")));
    }

    let status = refresh_application_status(app_repo, server_repo, sessions, local_process_manager, application_id).await?;
    if matches!(status, ApplicationStatus::Running | ApplicationStatus::Starting | ApplicationStatus::Stopping) {
        return Err(AppError::InvalidInput("stop the application before restoring a backup".into()));
    }

    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;
    let local_path = format!("{BACKUPS_DIR}/{}", backup.file_name);
    let archive_bytes = match provider.read_file(&local_path).await {
        Ok(bytes) => bytes,
        // The local copy is gone (a rebuilt Node, a wiped disk - exactly
        // what a local-only backup can't survive) - fall back to the S3
        // copy if this backup has one, rather than failing outright.
        Err(local_err) => {
            let Some(key) = &backup.s3_key else { return Err(local_err) };
            let Some(client) = s3_client(backup_destination).await else { return Err(local_err) };
            let bytes = client.get_object(key).await?;
            // Best-effort: heals the local copy too, so the *next* restore
            // (or a future prune) doesn't need S3 again.
            let _ = provider.write_file(&local_path, &bytes).await;
            bytes
        }
    };
    archive::extract_zip(provider.as_ref(), &archive_bytes, ".").await
}

pub fn get_backup_schedule(backup_repo: &ApplicationBackupRepository, application_id: Uuid) -> AppResult<BackupSchedule> {
    Ok(backup_repo.get_schedule(application_id)?.unwrap_or_default())
}

pub fn set_backup_schedule(backup_repo: &ApplicationBackupRepository, application_id: Uuid, input: SetBackupScheduleInput) -> AppResult<BackupSchedule> {
    if input.interval_hours == 0 {
        return Err(AppError::InvalidInput("the backup interval must be at least 1 hour".into()));
    }
    if input.retention_count == 0 {
        return Err(AppError::InvalidInput("keep at least 1 backup".into()));
    }
    backup_repo.set_schedule(application_id, &input)?;
    Ok(BackupSchedule {
        enabled: input.enabled,
        interval_hours: input.interval_hours,
        retention_count: input.retention_count,
        retention_max_age_days: input.retention_max_age_days,
        retention_max_total_bytes: input.retention_max_total_bytes,
    })
}

/// Applies all three retention rules independently - a backup is pruned
/// once it fails *any* of them (count, age, or running total size),
/// evaluated newest-first so "keep the N most recent"/"keep the most
/// recent total under N bytes" both mean what they sound like. Each
/// rule is skipped entirely when unset (`None`) - see
/// `models::SetBackupScheduleInput`'s own doc comment.
async fn prune_old_backups(
    app_repo: &ApplicationRepository,
    backup_repo: &ApplicationBackupRepository,
    server_repo: &ServerRepository,
    backup_destination: &BackupDestinationState,
    sessions: &SshSessionManager,
    application_id: Uuid,
    schedule: &BackupSchedule,
) -> AppResult<()> {
    let backups = backup_repo.list(application_id)?; // newest first
    let now = Utc::now();
    let mut kept_bytes: u64 = 0;
    let mut to_prune = Vec::new();
    for (index, backup) in backups.into_iter().enumerate() {
        let over_count = index as u32 >= schedule.retention_count;
        let over_age = schedule
            .retention_max_age_days
            .is_some_and(|days| now.signed_duration_since(backup.created_at) > chrono::Duration::days(days as i64));
        let over_size = schedule.retention_max_total_bytes.is_some_and(|max| kept_bytes + backup.size_bytes > max);
        if over_count || over_age || over_size {
            to_prune.push(backup);
        } else {
            kept_bytes += backup.size_bytes;
        }
    }
    if to_prune.is_empty() {
        return Ok(());
    }

    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;
    let client = s3_client(backup_destination).await;
    for old in to_prune {
        backup_repo.delete(old.id)?;
        let _ = provider.delete(&format!("{BACKUPS_DIR}/{}", old.file_name)).await;
        if let (Some(key), Some(client)) = (&old.s3_key, &client) {
            let _ = client.delete_object(key).await;
        }
    }
    Ok(())
}

/// Called by the frontend on its own timer (see this module's own doc
/// comment) - one Application failing (Node unreachable, etc.) doesn't
/// abort the sweep for the rest, and isn't surfaced as an error to the
/// caller either, since a silent skip-and-retry-next-tick is the right
/// behavior for something with no user watching it happen in the moment.
/// Returns how many backups were actually created, for an optional toast.
pub async fn run_due_backups(
    app_repo: &ApplicationRepository,
    backup_repo: &ApplicationBackupRepository,
    server_repo: &ServerRepository,
    backup_destination: &BackupDestinationState,
    sessions: &SshSessionManager,
) -> AppResult<u32> {
    let mut created = 0u32;
    for (application_id, schedule) in backup_repo.list_enabled_schedules()? {
        let due = match backup_repo.latest_backup_at(application_id)? {
            None => true,
            Some(last) => Utc::now().signed_duration_since(last) >= chrono::Duration::hours(schedule.interval_hours as i64),
        };
        if !due {
            continue;
        }
        if create_backup(app_repo, backup_repo, server_repo, backup_destination, sessions, application_id, BackupKind::Scheduled).await.is_ok() {
            created += 1;
            let _ = prune_old_backups(app_repo, backup_repo, server_repo, backup_destination, sessions, application_id, &schedule).await;
        }
    }
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CreateApplicationInput, RuntimeType};
    use crate::storage::server_repository::ServerRepository;

    /// A real Local application against an isolated temp working directory
    /// (never the shared system temp dir directly - `list_directory(".")`
    /// would otherwise sweep up whatever else happens to be in there) - end
    /// to end through the same `ApplicationFileProvider` production uses,
    /// not a mock.
    fn temp_setup() -> (ApplicationRepository, ApplicationBackupRepository, ServerRepository, BackupDestinationState, SshSessionManager, std::path::PathBuf)
    {
        let db_path = std::env::temp_dir().join(format!("vibessh-backup-service-test-{}.sqlite3", Uuid::new_v4()));
        let working_directory = std::env::temp_dir().join(format!("vibessh-backup-service-test-app-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&working_directory).unwrap();
        (
            ApplicationRepository::open(&db_path).unwrap(),
            ApplicationBackupRepository::open(&db_path).unwrap(),
            ServerRepository::open(&db_path).unwrap(),
            // Disabled by default - no test here exercises the real S3
            // path, that's `s3::tests`' own job (a signing-only concern,
            // no live destination available in CI/dev anyway).
            BackupDestinationState::new(BackupDestinationConfig::default()),
            SshSessionManager::new(),
            working_directory,
        )
    }

    #[tokio::test]
    async fn create_backup_then_restore_brings_a_deleted_file_back() {
        let (app_repo, backup_repo, server_repo, backup_destination, sessions, working_directory) = temp_setup();
        std::fs::write(working_directory.join("server.properties"), b"motd=hello").unwrap();
        std::fs::create_dir_all(working_directory.join("plugins")).unwrap();
        std::fs::write(working_directory.join("plugins/example.jar"), b"not a real jar").unwrap();

        let application_id = app_repo
            .create(&CreateApplicationInput {
                server_id: None,
                name: "Backup Test App".into(),
                description: None,
                blueprint_id: "generic".into(),
                blueprint_version: 1,
                runtime_type: RuntimeType::LocalProcess,
                working_directory: working_directory.to_string_lossy().into_owned(),
                environment: vec![],
                ports: vec![],
                runtime_config: serde_json::json!({ "command": "true", "args": [] }),
                metadata: serde_json::json!({}),
            })
            .unwrap()
            .application
            .id;

        let backup = create_backup(&app_repo, &backup_repo, &server_repo, &backup_destination, &sessions, application_id, BackupKind::Manual).await.unwrap();
        assert!(backup.size_bytes > 0);
        assert_eq!(list_backups(&backup_repo, application_id).await.unwrap().len(), 1);
        assert!(working_directory.join(BACKUPS_DIR).join(&backup.file_name).exists());

        std::fs::remove_file(working_directory.join("server.properties")).unwrap();
        std::fs::remove_dir_all(working_directory.join("plugins")).unwrap();
        assert!(!working_directory.join("server.properties").exists());

        restore_backup(&app_repo, &backup_repo, &server_repo, &backup_destination, &sessions, &Arc::new(LocalProcessManager::new()), application_id, backup.id)
            .await
            .unwrap();

        assert_eq!(std::fs::read_to_string(working_directory.join("server.properties")).unwrap(), "motd=hello");
        assert_eq!(std::fs::read(working_directory.join("plugins/example.jar")).unwrap(), b"not a real jar");
    }

    #[tokio::test]
    async fn a_backup_never_includes_the_backups_folder_itself() {
        let (app_repo, backup_repo, server_repo, backup_destination, sessions, working_directory) = temp_setup();
        std::fs::write(working_directory.join("readme.txt"), b"hi").unwrap();

        let application_id = app_repo
            .create(&CreateApplicationInput {
                server_id: None,
                name: "Backup Test App".into(),
                description: None,
                blueprint_id: "generic".into(),
                blueprint_version: 1,
                runtime_type: RuntimeType::LocalProcess,
                working_directory: working_directory.to_string_lossy().into_owned(),
                environment: vec![],
                ports: vec![],
                runtime_config: serde_json::json!({ "command": "true", "args": [] }),
                metadata: serde_json::json!({}),
            })
            .unwrap()
            .application
            .id;

        create_backup(&app_repo, &backup_repo, &server_repo, &backup_destination, &sessions, application_id, BackupKind::Manual).await.unwrap();
        // A second backup, taken after the first one already exists on disk,
        // must not fold `.vibessh-backups/` into itself - each ~doubling in
        // size on every run would be an obvious, silent bug.
        let second = create_backup(&app_repo, &backup_repo, &server_repo, &backup_destination, &sessions, application_id, BackupKind::Manual).await.unwrap();

        let bytes = std::fs::read(working_directory.join(BACKUPS_DIR).join(&second.file_name)).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        for i in 0..zip.len() {
            let entry = zip.by_index(i).unwrap();
            assert!(!entry.name().starts_with(BACKUPS_DIR), "backup contained its own backups folder: {}", entry.name());
        }
    }

    #[tokio::test]
    async fn delete_backup_removes_both_the_row_and_the_file() {
        let (app_repo, backup_repo, server_repo, backup_destination, sessions, working_directory) = temp_setup();
        std::fs::write(working_directory.join("readme.txt"), b"hi").unwrap();

        let application_id = app_repo
            .create(&CreateApplicationInput {
                server_id: None,
                name: "Backup Test App".into(),
                description: None,
                blueprint_id: "generic".into(),
                blueprint_version: 1,
                runtime_type: RuntimeType::LocalProcess,
                working_directory: working_directory.to_string_lossy().into_owned(),
                environment: vec![],
                ports: vec![],
                runtime_config: serde_json::json!({ "command": "true", "args": [] }),
                metadata: serde_json::json!({}),
            })
            .unwrap()
            .application
            .id;

        let backup = create_backup(&app_repo, &backup_repo, &server_repo, &backup_destination, &sessions, application_id, BackupKind::Manual).await.unwrap();
        let file_path = working_directory.join(BACKUPS_DIR).join(&backup.file_name);
        assert!(file_path.exists());

        delete_backup(&app_repo, &backup_repo, &server_repo, &backup_destination, &sessions, application_id, backup.id).await.unwrap();

        assert!(list_backups(&backup_repo, application_id).await.unwrap().is_empty());
        assert!(!file_path.exists());
    }
}
