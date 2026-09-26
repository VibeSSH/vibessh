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

    // Not checked: an existing directory is the common case and reports an
    // error on some providers, and if it genuinely could not be created the
    // `create_zip` immediately below fails with a message that names the
    // real problem.
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
        if let Err(err) = provider.delete(&format!("{BACKUPS_DIR}/{file_name}")).await {
            // Best-effort, but never silent: the UI stops listing this
            // backup, and an operator who deleted it to reclaim disk - or
            // because of what it contains - is entitled to know the file is
            // still on the Node.
            log::warn!("the '{file_name}' backup row was deleted but the file is still on the node: {err}");
        }
    }
    if let Some(key) = s3_key {
        if let Some(client) = s3_client(backup_destination).await {
            if let Err(err) = client.delete_object(&key).await {
                log::warn!("the '{file_name}' backup row was deleted but the object is still in the bucket: {err}");
            }
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

    // Streamed to a local scratch file rather than read into a `Vec<u8>`.
    // A restore archive is the largest thing this application ever moves -
    // a world save, a database volume - and holding all of it in memory is
    // how a restore turned into an out-of-memory abort instead of a
    // restore. `download_file` already streams; `extract_zip_from_file`
    // then reads one entry at a time.
    let scratch = std::env::temp_dir().join(format!("vibessh-restore-{}.zip", Uuid::new_v4()));
    let mut no_progress = |_: u64| {};
    let fetched = match provider.download_file(&local_path, &scratch, &mut no_progress).await {
        Ok(()) => Ok(()),
        // The local copy is gone (a rebuilt Node, a wiped disk - exactly
        // what a local-only backup can't survive) - fall back to the S3
        // copy if this backup has one, rather than failing outright.
        Err(local_err) => match (&backup.s3_key, s3_client(backup_destination).await) {
            (Some(key), Some(client)) => {
                let bytes = client.get_object(key).await?;
                tokio::fs::write(&scratch, &bytes)
                    .await
                    .map_err(|err| AppError::Internal(format!("couldn't stage the downloaded backup: {err}")))?;
                // Best-effort: heals the local copy too, so the *next*
                // restore (or a future prune) doesn't need S3 again. The
                // restore itself proceeds from `scratch` either way, so this
                // failing costs a slower next restore, not this one.
                if let Err(err) = provider.write_file(&local_path, &bytes).await {
                    log::warn!("restored from the remote copy, but couldn't re-create the local one: {err}");
                }
                Ok(())
            }
            _ => Err(local_err),
        },
    };

    let result = match fetched {
        Ok(()) => archive::extract_zip_from_file(provider.as_ref(), &scratch, ".").await,
        Err(err) => Err(err),
    };
    let _ = tokio::fs::remove_file(&scratch).await;
    result
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

/// Which backups the retention rules remove - all three rules applied
/// independently, so a backup goes once it fails *any* of them (count, age,
/// or running total size), evaluated newest-first so "keep the N most
/// recent" and "keep the most recent total under N bytes" mean what they
/// sound like. A rule left unset (`None`) is skipped.
///
/// Two things are never removed, and both used to be:
/// - **A manual backup.** Somebody made it on purpose; a schedule's rules
///   are about the schedule's own backups. `list` returns both kinds, so a
///   manual one taken before an upgrade was pruned like any other.
/// - **The newest scheduled backup.** With a size limit below one backup's
///   size, the one just made failed "0 + its size > limit" and was deleted
///   with its file - and since that left nothing recent, the next tick
///   fifteen minutes later made another and deleted it too, forever, each
///   time toasting "backup created". The newest one is kept whatever its
///   size; the limit then prunes everything older.
fn backups_to_prune(backups: Vec<ApplicationBackup>, schedule: &BackupSchedule, now: chrono::DateTime<Utc>) -> Vec<ApplicationBackup> {
    let mut kept_bytes: u64 = 0;
    let mut to_prune = Vec::new();
    let scheduled = backups.into_iter().filter(|backup| backup.kind == BackupKind::Scheduled);
    for (index, backup) in scheduled.enumerate() {
        if index == 0 {
            kept_bytes += backup.size_bytes;
            continue;
        }
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
    to_prune
}

async fn prune_old_backups(
    app_repo: &ApplicationRepository,
    backup_repo: &ApplicationBackupRepository,
    server_repo: &ServerRepository,
    backup_destination: &BackupDestinationState,
    sessions: &SshSessionManager,
    application_id: Uuid,
    schedule: &BackupSchedule,
) -> AppResult<()> {
    let to_prune = backups_to_prune(backup_repo.list(application_id)?, schedule, Utc::now()); // list is newest first
    if to_prune.is_empty() {
        return Ok(());
    }

    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;
    let client = s3_client(backup_destination).await;
    for old in to_prune {
        backup_repo.delete(old.id)?;
        // Retention is a promise about disk, not about rows. A prune that
        // drops the row and leaves the file means "keep at most N bytes"
        // quietly stops being true while the UI shows it working.
        if let Err(err) = provider.delete(&format!("{BACKUPS_DIR}/{}", old.file_name)).await {
            log::warn!("retention removed the '{}' backup but its file is still on the node: {err}", old.file_name);
        }
        if let (Some(key), Some(client)) = (&old.s3_key, &client) {
            if let Err(err) = client.delete_object(key).await {
                log::warn!("retention removed the '{}' backup but its object is still in the bucket: {err}", old.file_name);
            }
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
        let made = create_backup(app_repo, backup_repo, server_repo, backup_destination, sessions, application_id, BackupKind::Scheduled).await;
        if let Err(err) = &made {
            // Nobody is watching the sweep, and a schedule that never
            // succeeds used to look exactly like one that works: no toast,
            // no log line. It is retried next tick; this is the trace.
            log::warn!("the scheduled backup of application {application_id} failed, trying again next time: {err}");
        }
        if made.is_ok() {
            created += 1;
            if let Err(err) = prune_old_backups(app_repo, backup_repo, server_repo, backup_destination, sessions, application_id, &schedule).await {
                // Nobody is watching this sweep, so it must not fail the
                // run - but a retention rule that has silently stopped
                // running is exactly how a Node fills its disk overnight.
                log::warn!("couldn't apply the retention rules for application {application_id}: {err}");
            }
        }
    }
    Ok(created)
}

#[cfg(test)]
mod retention_tests {
    use super::*;

    fn backup(kind: BackupKind, size_bytes: u64, age_days: i64, now: chrono::DateTime<Utc>) -> ApplicationBackup {
        ApplicationBackup {
            id: Uuid::new_v4(),
            application_id: Uuid::nil(),
            file_name: format!("{}.zip", Uuid::new_v4()),
            size_bytes,
            kind,
            s3_key: None,
            created_at: now - chrono::Duration::days(age_days),
        }
    }

    fn schedule(count: u32, max_age_days: Option<u32>, max_total_bytes: Option<u64>) -> BackupSchedule {
        BackupSchedule { enabled: true, interval_hours: 24, retention_count: count, retention_max_age_days: max_age_days, retention_max_total_bytes: max_total_bytes }
    }

    /// The loop: one backup bigger than the size limit deleted itself, every
    /// fifteen minutes, forever. The newest is kept; older ones go.
    #[test]
    fn the_newest_backup_survives_a_size_limit_smaller_than_itself() {
        let now = Utc::now();
        let newest = backup(BackupKind::Scheduled, 5_000, 0, now);
        let older = backup(BackupKind::Scheduled, 5_000, 1, now);
        let pruned = backups_to_prune(vec![newest.clone(), older.clone()], &schedule(10, None, Some(1_000)), now);
        let ids: Vec<Uuid> = pruned.iter().map(|b| b.id).collect();
        assert!(!ids.contains(&newest.id), "the backup just made was pruned");
        assert!(ids.contains(&older.id));
    }

    /// A manual backup is never retention's to remove, however old or big.
    #[test]
    fn a_manual_backup_is_never_pruned() {
        let now = Utc::now();
        let manual = backup(BackupKind::Manual, 50_000, 400, now);
        let scheduled = [backup(BackupKind::Scheduled, 10, 0, now), backup(BackupKind::Scheduled, 10, 1, now)];
        let all = vec![scheduled[0].clone(), manual.clone(), scheduled[1].clone()];
        let pruned = backups_to_prune(all, &schedule(1, Some(30), Some(100)), now);
        assert!(!pruned.iter().any(|b| b.id == manual.id));
        assert_eq!(pruned.len(), 1, "only the older scheduled backup is over the count");
        assert_eq!(pruned[0].id, scheduled[1].id);
    }

    /// The rules still do their job on the schedule's own backups.
    #[test]
    fn count_and_age_still_prune_older_scheduled_backups() {
        let now = Utc::now();
        let backups: Vec<ApplicationBackup> = (0..5).map(|day| backup(BackupKind::Scheduled, 10, day * 10, now)).collect();
        let by_count = backups_to_prune(backups.clone(), &schedule(3, None, None), now);
        assert_eq!(by_count.len(), 2);
        let by_age = backups_to_prune(backups, &schedule(10, Some(15), None), now);
        assert_eq!(by_age.len(), 3, "20, 30 and 40 days old are over 15");
    }
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
