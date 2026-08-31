//! SQLite-backed storage for `dns_records` (Etap M4/M5 - Private DNS). One
//! alias per Application, enforced at the schema level (`UNIQUE`) - see
//! `models::DnsRecord`'s own doc comment.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::DnsRecord;
use crate::storage::migrations::migrations;

pub struct DnsRepository {
    conn: Mutex<Connection>,
}

impl DnsRepository {
    pub fn open(db_path: &Path) -> AppResult<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| AppError::Storage(format!("failed to create the DNS database directory: {err}")))?;
        }
        let mut conn = Connection::open(db_path).map_err(|err| AppError::Storage(format!("failed to open the DNS database: {err}")))?;
        conn.pragma_update(None, "foreign_keys", true)
            .map_err(|err| AppError::Storage(format!("failed to enable foreign key enforcement: {err}")))?;
        migrations().to_latest(&mut conn).map_err(|err| AppError::Storage(format!("failed to migrate the DNS database: {err}")))?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().expect("DNS repository connection mutex poisoned")
    }

    pub fn list(&self) -> AppResult<Vec<DnsRecord>> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare("SELECT id, application_id, hostname, created_at FROM dns_records ORDER BY hostname")
            .map_err(|err| AppError::Storage(format!("failed to prepare the DNS record list query: {err}")))?;
        let rows = stmt.query_map((), row_to_record).map_err(|err| AppError::Storage(format!("failed to list DNS records: {err}")))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|err| AppError::Storage(format!("failed to read a DNS record row: {err}")))
    }

    /// `None` on success with no collision; `Some(existing hostname)` if
    /// `hostname` already belongs to a different record - the caller
    /// (service layer) turns that into a clear `AppError::InvalidInput`
    /// rather than a raw constraint failure, same pattern
    /// `ApplicationRepository::find_port_collision_locked` already uses.
    fn find_hostname_collision(&self, conn: &Connection, excluding_id: Option<Uuid>, hostname: &str) -> AppResult<Option<String>> {
        conn.query_row(
            "SELECT hostname FROM dns_records WHERE hostname = ?1 AND id != ?2",
            params![hostname, excluding_id.map(|id| id.to_string()).unwrap_or_default()],
            |row| row.get(0),
        )
        .optional()
        .map_err(|err| AppError::Storage(format!("failed to check for a hostname collision: {err}")))
    }

    pub fn create(&self, application_id: Uuid, hostname: &str) -> AppResult<DnsRecord> {
        let conn = self.lock();
        if let Some(existing) = self.find_hostname_collision(&conn, None, hostname)? {
            return Err(AppError::InvalidInput(format!("hostname '{existing}' is already in use")));
        }
        let id = Uuid::new_v4();
        let created_at = Utc::now();
        conn.execute(
            "INSERT INTO dns_records (id, application_id, hostname, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![id.to_string(), application_id.to_string(), hostname, created_at.to_rfc3339()],
        )
        .map_err(|err| {
            if is_unique_violation(&err) {
                AppError::InvalidInput(format!("application {application_id} already has a DNS alias"))
            } else {
                AppError::Storage(format!("failed to create the DNS record: {err}"))
            }
        })?;
        Ok(DnsRecord { id, application_id, hostname: hostname.to_string(), created_at })
    }

    pub fn update_hostname(&self, id: Uuid, hostname: &str) -> AppResult<DnsRecord> {
        let conn = self.lock();
        if let Some(existing) = self.find_hostname_collision(&conn, Some(id), hostname)? {
            return Err(AppError::InvalidInput(format!("hostname '{existing}' is already in use")));
        }
        let affected = conn
            .execute("UPDATE dns_records SET hostname = ?2 WHERE id = ?1", params![id.to_string(), hostname])
            .map_err(|err| AppError::Storage(format!("failed to update the DNS record: {err}")))?;
        if affected == 0 {
            return Err(AppError::NotFound(format!("DNS record {id}")));
        }
        conn.query_row("SELECT id, application_id, hostname, created_at FROM dns_records WHERE id = ?1", params![id.to_string()], row_to_record)
            .map_err(|err| AppError::Storage(format!("failed to read the DNS record back: {err}")))
    }

    /// Service migration's DNS cutover step: repoints whichever alias
    /// belongs to `old_application_id` (if any) at `new_application_id` -
    /// the hostname itself never changes, only which Application it now
    /// resolves through (see this module's own doc comment: the IP is
    /// always resolved fresh at render time via `applications.server_id`,
    /// never stored here). `Ok(None)` - not an error - when the migrated
    /// Application had no alias to begin with.
    pub fn repoint_application(&self, old_application_id: Uuid, new_application_id: Uuid) -> AppResult<Option<DnsRecord>> {
        let conn = self.lock();
        let affected = conn
            .execute(
                "UPDATE dns_records SET application_id = ?2 WHERE application_id = ?1",
                params![old_application_id.to_string(), new_application_id.to_string()],
            )
            .map_err(|err| {
                if is_unique_violation(&err) {
                    AppError::InvalidInput(format!("application {new_application_id} already has a DNS alias"))
                } else {
                    AppError::Storage(format!("failed to repoint the DNS record: {err}"))
                }
            })?;
        if affected == 0 {
            return Ok(None);
        }
        conn.query_row(
            "SELECT id, application_id, hostname, created_at FROM dns_records WHERE application_id = ?1",
            params![new_application_id.to_string()],
            row_to_record,
        )
        .optional()
        .map_err(|err| AppError::Storage(format!("failed to read the repointed DNS record back: {err}")))
    }

    pub fn delete(&self, id: Uuid) -> AppResult<()> {
        let affected = self
            .lock()
            .execute("DELETE FROM dns_records WHERE id = ?1", params![id.to_string()])
            .map_err(|err| AppError::Storage(format!("failed to delete the DNS record: {err}")))?;
        if affected == 0 {
            return Err(AppError::NotFound(format!("DNS record {id}")));
        }
        Ok(())
    }
}

fn is_unique_violation(err: &rusqlite::Error) -> bool {
    matches!(err, rusqlite::Error::SqliteFailure(sqlite_err, _) if sqlite_err.code == rusqlite::ErrorCode::ConstraintViolation)
}

fn row_to_record(row: &rusqlite::Row) -> rusqlite::Result<DnsRecord> {
    Ok(DnsRecord {
        id: Uuid::parse_str(&row.get::<_, String>(0)?).expect("stored UUID column is always well-formed"),
        application_id: Uuid::parse_str(&row.get::<_, String>(1)?).expect("stored UUID column is always well-formed"),
        hostname: row.get(2)?,
        created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(3)?)
            .expect("stored timestamp column is always well-formed")
            .with_timezone(&Utc),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CreateApplicationInput, RuntimeType};
    use crate::storage::application_repository::ApplicationRepository;

    fn temp_repository() -> (DnsRepository, ApplicationRepository) {
        let path = std::env::temp_dir().join(format!("vibessh-dns-test-{}.sqlite3", Uuid::new_v4()));
        (DnsRepository::open(&path).unwrap(), ApplicationRepository::open(&path).unwrap())
    }

    fn create_test_application(app_repo: &ApplicationRepository, name: &str) -> Uuid {
        app_repo
            .create(&CreateApplicationInput {
                server_id: None,
                name: name.into(),
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
    fn create_then_list_round_trips() {
        let (repo, app_repo) = temp_repository();
        let app_id = create_test_application(&app_repo, "DB");
        let record = repo.create(app_id, "db01.vibe").unwrap();
        assert_eq!(record.hostname, "db01.vibe");
        assert_eq!(repo.list().unwrap().len(), 1);
    }

    #[test]
    fn create_rejects_a_duplicate_hostname() {
        let (repo, app_repo) = temp_repository();
        let app_a = create_test_application(&app_repo, "A");
        let app_b = create_test_application(&app_repo, "B");
        repo.create(app_a, "db01.vibe").unwrap();
        assert!(repo.create(app_b, "db01.vibe").is_err());
    }

    #[test]
    fn create_rejects_a_second_alias_for_the_same_application() {
        let (repo, app_repo) = temp_repository();
        let app_id = create_test_application(&app_repo, "A");
        repo.create(app_id, "db01.vibe").unwrap();
        assert!(repo.create(app_id, "db02.vibe").is_err());
    }

    #[test]
    fn update_then_delete() {
        let (repo, app_repo) = temp_repository();
        let app_id = create_test_application(&app_repo, "A");
        let record = repo.create(app_id, "db01.vibe").unwrap();

        let updated = repo.update_hostname(record.id, "db01-renamed.vibe").unwrap();
        assert_eq!(updated.hostname, "db01-renamed.vibe");

        repo.delete(record.id).unwrap();
        assert!(repo.list().unwrap().is_empty());
    }

    #[test]
    fn delete_of_an_unknown_id_is_not_found() {
        let (repo, _app_repo) = temp_repository();
        assert!(matches!(repo.delete(Uuid::new_v4()).unwrap_err(), AppError::NotFound(_)));
    }

    #[test]
    fn repoint_application_moves_the_alias_to_the_new_application_id() {
        let (repo, app_repo) = temp_repository();
        let old_app = create_test_application(&app_repo, "old");
        let new_app = create_test_application(&app_repo, "new");
        repo.create(old_app, "db01.vibe").unwrap();

        let repointed = repo.repoint_application(old_app, new_app).unwrap().unwrap();
        assert_eq!(repointed.application_id, new_app);
        assert_eq!(repointed.hostname, "db01.vibe");
        assert_eq!(repo.list().unwrap().len(), 1, "the alias moves, it doesn't duplicate");
    }

    #[test]
    fn repoint_application_is_a_no_op_when_the_old_application_had_no_alias() {
        let (repo, app_repo) = temp_repository();
        let old_app = create_test_application(&app_repo, "old");
        let new_app = create_test_application(&app_repo, "new");
        assert!(repo.repoint_application(old_app, new_app).unwrap().is_none());
    }
}
