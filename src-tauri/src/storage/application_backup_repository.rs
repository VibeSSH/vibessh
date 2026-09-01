//! SQLite-backed storage for `application_backups` (history) and
//! `application_backup_schedules` (per-Application config) - see
//! `storage::migrations`'s own doc comment on both tables, and
//! `models::application_backup` for the DTOs. A separate repository, own
//! connection to the same db file, same pattern every other per-concern
//! repository here already follows (`DnsRepository`, `NodeNetworkRepository`, ...).

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{ApplicationBackup, BackupKind, BackupSchedule, SetBackupScheduleInput};
use crate::storage::migrations::migrations;

pub struct ApplicationBackupRepository {
    conn: Mutex<Connection>,
}

impl ApplicationBackupRepository {
    pub fn open(db_path: &Path) -> AppResult<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| AppError::Storage(format!("failed to create the backups database directory: {err}")))?;
        }
        let mut conn = Connection::open(db_path).map_err(|err| AppError::Storage(format!("failed to open the backups database: {err}")))?;
        conn.pragma_update(None, "foreign_keys", true)
            .map_err(|err| AppError::Storage(format!("failed to enable foreign key enforcement: {err}")))?;
        migrations().to_latest(&mut conn).map_err(|err| AppError::Storage(format!("failed to migrate the backups database: {err}")))?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().expect("backups repository connection mutex poisoned")
    }

    pub fn list(&self, application_id: Uuid) -> AppResult<Vec<ApplicationBackup>> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare("SELECT id, application_id, file_name, size_bytes, kind, s3_key, created_at FROM application_backups WHERE application_id = ?1 ORDER BY created_at DESC")
            .map_err(|err| AppError::Storage(format!("failed to prepare the backup list query: {err}")))?;
        let rows = stmt
            .query_map(params![application_id.to_string()], row_to_backup)
            .map_err(|err| AppError::Storage(format!("failed to list backups: {err}")))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|err| AppError::Storage(format!("failed to read a backup row: {err}")))
    }

    pub fn get(&self, id: Uuid) -> AppResult<Option<ApplicationBackup>> {
        self.lock()
            .query_row(
                "SELECT id, application_id, file_name, size_bytes, kind, s3_key, created_at FROM application_backups WHERE id = ?1",
                params![id.to_string()],
                row_to_backup,
            )
            .optional()
            .map_err(|err| AppError::Storage(format!("failed to read the backup: {err}")))
    }

    pub fn create(&self, application_id: Uuid, file_name: &str, size_bytes: u64, kind: BackupKind) -> AppResult<ApplicationBackup> {
        let id = Uuid::new_v4();
        let created_at = Utc::now();
        self.lock()
            .execute(
                "INSERT INTO application_backups (id, application_id, file_name, size_bytes, kind, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id.to_string(), application_id.to_string(), file_name, size_bytes, kind.as_str(), created_at.to_rfc3339()],
            )
            .map_err(|err| AppError::Storage(format!("failed to record the backup: {err}")))?;
        Ok(ApplicationBackup { id, application_id, file_name: file_name.to_string(), size_bytes, kind, s3_key: None, created_at })
    }

    /// Records that `id` was successfully uploaded to the configured S3
    /// destination under `s3_key` - called once, right after the upload
    /// actually succeeds (see `services::application_backup_service::create_backup`).
    /// A no-op, not an error, if the backup row is already gone by the time
    /// this runs (deleted mid-upload) - nothing left to annotate.
    pub fn set_s3_key(&self, id: Uuid, s3_key: &str) -> AppResult<()> {
        self.lock()
            .execute("UPDATE application_backups SET s3_key = ?2 WHERE id = ?1", params![id.to_string(), s3_key])
            .map_err(|err| AppError::Storage(format!("failed to record the backup's S3 key: {err}")))?;
        Ok(())
    }

    /// Returns the deleted row's `file_name` (so the caller can also remove
    /// the archive itself from the Application's filesystem) - `None` if it
    /// was already gone.
    pub fn delete(&self, id: Uuid) -> AppResult<Option<String>> {
        let conn = self.lock();
        let file_name: Option<String> = conn
            .query_row("SELECT file_name FROM application_backups WHERE id = ?1", params![id.to_string()], |row| row.get(0))
            .optional()
            .map_err(|err| AppError::Storage(format!("failed to look up the backup before deleting it: {err}")))?;
        if file_name.is_some() {
            conn.execute("DELETE FROM application_backups WHERE id = ?1", params![id.to_string()])
                .map_err(|err| AppError::Storage(format!("failed to delete the backup record: {err}")))?;
        }
        Ok(file_name)
    }

    /// `None` = no schedule row yet, i.e. never configured - callers apply
    /// `BackupSchedule::default()` (`enabled: false`) themselves, same
    /// "absent means unset" idiom the migration's own doc comment describes.
    pub fn get_schedule(&self, application_id: Uuid) -> AppResult<Option<BackupSchedule>> {
        self.lock()
            .query_row(
                "SELECT enabled, interval_hours, retention_count, retention_max_age_days, retention_max_total_bytes
                 FROM application_backup_schedules WHERE application_id = ?1",
                params![application_id.to_string()],
                |row| row_to_schedule_at(row, 0),
            )
            .optional()
            .map_err(|err| AppError::Storage(format!("failed to read the backup schedule: {err}")))
    }

    pub fn set_schedule(&self, application_id: Uuid, input: &SetBackupScheduleInput) -> AppResult<()> {
        self.lock()
            .execute(
                "INSERT INTO application_backup_schedules
                    (application_id, enabled, interval_hours, retention_count, retention_max_age_days, retention_max_total_bytes, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(application_id) DO UPDATE SET enabled = excluded.enabled, interval_hours = excluded.interval_hours,
                    retention_count = excluded.retention_count, retention_max_age_days = excluded.retention_max_age_days,
                    retention_max_total_bytes = excluded.retention_max_total_bytes, updated_at = excluded.updated_at",
                params![
                    application_id.to_string(),
                    input.enabled,
                    input.interval_hours,
                    input.retention_count,
                    input.retention_max_age_days,
                    input.retention_max_total_bytes.map(|v| v as i64),
                    Utc::now().to_rfc3339(),
                ],
            )
            .map_err(|err| AppError::Storage(format!("failed to save the backup schedule: {err}")))?;
        Ok(())
    }

    /// Every Application with a schedule row where `enabled = 1` - the
    /// service layer (`application_backup_service::run_due_backups`) is what
    /// actually decides which of these are *due* right now (needs each
    /// Application's most recent backup timestamp too, via `list`), this
    /// just narrows down to "worth checking at all".
    pub fn list_enabled_schedules(&self) -> AppResult<Vec<(Uuid, BackupSchedule)>> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare(
                "SELECT application_id, enabled, interval_hours, retention_count, retention_max_age_days, retention_max_total_bytes
                 FROM application_backup_schedules WHERE enabled = 1",
            )
            .map_err(|err| AppError::Storage(format!("failed to prepare the enabled-schedules query: {err}")))?;
        let rows = stmt
            .query_map((), |row| {
                let application_id = Uuid::parse_str(&row.get::<_, String>(0)?).expect("stored UUID column is always well-formed");
                Ok((application_id, row_to_schedule_at(row, 1)?))
            })
            .map_err(|err| AppError::Storage(format!("failed to list enabled backup schedules: {err}")))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|err| AppError::Storage(format!("failed to read an enabled-schedule row: {err}")))
    }

    /// `None` when this Application has never been backed up.
    pub fn latest_backup_at(&self, application_id: Uuid) -> AppResult<Option<DateTime<Utc>>> {
        self.lock()
            .query_row(
                "SELECT created_at FROM application_backups WHERE application_id = ?1 ORDER BY created_at DESC LIMIT 1",
                params![application_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|err| AppError::Storage(format!("failed to read the latest backup timestamp: {err}")))?
            .map(|raw| {
                chrono::DateTime::parse_from_rfc3339(&raw).map(|dt| dt.with_timezone(&Utc)).map_err(|err| AppError::Storage(format!("stored backup timestamp was invalid: {err}")))
            })
            .transpose()
    }
}

fn row_to_backup(row: &rusqlite::Row) -> rusqlite::Result<ApplicationBackup> {
    Ok(ApplicationBackup {
        id: Uuid::parse_str(&row.get::<_, String>(0)?).expect("stored UUID column is always well-formed"),
        application_id: Uuid::parse_str(&row.get::<_, String>(1)?).expect("stored UUID column is always well-formed"),
        file_name: row.get(2)?,
        size_bytes: row.get::<_, i64>(3)? as u64,
        kind: BackupKind::parse(&row.get::<_, String>(4)?),
        s3_key: row.get(5)?,
        created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(6)?)
            .expect("stored timestamp column is always well-formed")
            .with_timezone(&Utc),
    })
}

/// Reads a `BackupSchedule` starting at column `offset` - `get_schedule`'s
/// own query has it at 0, `list_enabled_schedules`'s query has
/// `application_id` first, pushing it to 1. Same "one row-mapper, callers
/// vary only where it starts" shape as `ApplicationRepository`'s own
/// `row_to_application`.
fn row_to_schedule_at(row: &rusqlite::Row, offset: usize) -> rusqlite::Result<BackupSchedule> {
    Ok(BackupSchedule {
        enabled: row.get::<_, i64>(offset)? != 0,
        interval_hours: row.get(offset + 1)?,
        retention_count: row.get(offset + 2)?,
        retention_max_age_days: row.get(offset + 3)?,
        retention_max_total_bytes: row.get::<_, Option<i64>>(offset + 4)?.map(|v| v as u64),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CreateApplicationInput, RuntimeType};
    use crate::storage::application_repository::ApplicationRepository;

    fn temp_repository() -> (ApplicationBackupRepository, ApplicationRepository) {
        let path = std::env::temp_dir().join(format!("vibessh-backups-test-{}.sqlite3", Uuid::new_v4()));
        (ApplicationBackupRepository::open(&path).unwrap(), ApplicationRepository::open(&path).unwrap())
    }

    fn create_test_application(app_repo: &ApplicationRepository) -> Uuid {
        app_repo
            .create(&CreateApplicationInput {
                server_id: None,
                name: "App".into(),
                description: None,
                blueprint_id: "generic".into(),
                blueprint_version: 1,
                runtime_type: RuntimeType::LocalProcess,
                working_directory: "/srv/app".into(),
                environment: vec![],
                ports: vec![],
                runtime_config: serde_json::json!({}),
                metadata: serde_json::json!({}),
            })
            .unwrap()
            .application
            .id
    }

    #[test]
    fn create_then_list_orders_newest_first() {
        let (repo, app_repo) = temp_repository();
        let app_id = create_test_application(&app_repo);
        let first = repo.create(app_id, "a.zip", 100, BackupKind::Manual).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = repo.create(app_id, "b.zip", 200, BackupKind::Scheduled).unwrap();

        let listed = repo.list(app_id).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, second.id);
        assert_eq!(listed[1].id, first.id);
    }

    #[test]
    fn delete_returns_the_file_name_and_removes_the_row() {
        let (repo, app_repo) = temp_repository();
        let app_id = create_test_application(&app_repo);
        let backup = repo.create(app_id, "a.zip", 100, BackupKind::Manual).unwrap();

        assert_eq!(repo.delete(backup.id).unwrap(), Some("a.zip".to_string()));
        assert!(repo.list(app_id).unwrap().is_empty());
        assert_eq!(repo.delete(backup.id).unwrap(), None, "deleting an already-gone backup is a no-op, not an error");
    }

    #[test]
    fn schedule_defaults_to_absent_then_round_trips_through_set() {
        let (repo, app_repo) = temp_repository();
        let app_id = create_test_application(&app_repo);
        assert!(repo.get_schedule(app_id).unwrap().is_none());

        repo.set_schedule(app_id, &SetBackupScheduleInput { enabled: true, interval_hours: 12, retention_count: 3, retention_max_age_days: None, retention_max_total_bytes: None }).unwrap();
        let schedule = repo.get_schedule(app_id).unwrap().unwrap();
        assert!(schedule.enabled);
        assert_eq!(schedule.interval_hours, 12);
        assert_eq!(schedule.retention_count, 3);

        assert_eq!(repo.list_enabled_schedules().unwrap().len(), 1);

        repo.set_schedule(app_id, &SetBackupScheduleInput { enabled: false, interval_hours: 12, retention_count: 3, retention_max_age_days: None, retention_max_total_bytes: None }).unwrap();
        assert!(repo.list_enabled_schedules().unwrap().is_empty(), "disabling updates the existing row rather than inserting a second one");
    }

    #[test]
    fn latest_backup_at_is_none_until_the_first_backup_exists() {
        let (repo, app_repo) = temp_repository();
        let app_id = create_test_application(&app_repo);
        assert!(repo.latest_backup_at(app_id).unwrap().is_none());

        repo.create(app_id, "a.zip", 100, BackupKind::Manual).unwrap();
        assert!(repo.latest_backup_at(app_id).unwrap().is_some());
    }
}
