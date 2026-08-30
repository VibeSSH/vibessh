//! SQLite-backed storage for `DatabaseHost`/`ApplicationDatabase` - Phase 11
//! *foundation only*, see `models::database`'s own doc comment for the full
//! scope note (schema + types + this repository; no provisioning service,
//! no commands, no UI yet). A separate repository struct from
//! `ApplicationRepository`/`ServerRepository`, same one-concern-per-
//! repository convention they already follow, even though all three open
//! the same physical database file.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{ApplicationDatabase, CreateApplicationDatabaseInput, CreateDatabaseHostInput, DatabaseEngine, DatabaseHost};
use crate::storage::migrations::migrations;

pub struct DatabaseRepository {
    conn: Mutex<Connection>,
}

impl DatabaseRepository {
    /// `db_path` is the same `servers.sqlite3` every other repository
    /// opens - `migrations().to_latest()` is a no-op once the file's
    /// `user_version` is already current, so it's safe to call from here
    /// too.
    pub fn open(db_path: &Path) -> AppResult<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| AppError::Storage(format!("failed to create the application database directory: {err}")))?;
        }
        let mut conn =
            Connection::open(db_path).map_err(|err| AppError::Storage(format!("failed to open the application database: {err}")))?;
        conn.pragma_update(None, "foreign_keys", true)
            .map_err(|err| AppError::Storage(format!("failed to enable foreign key enforcement: {err}")))?;
        migrations()
            .to_latest(&mut conn)
            .map_err(|err| AppError::Storage(format!("failed to migrate the application database: {err}")))?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().expect("database repository connection mutex poisoned")
    }

    // ---- DatabaseHost ----

    pub fn create_host(&self, input: &CreateDatabaseHostInput) -> AppResult<DatabaseHost> {
        let conn = self.lock();
        let id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO database_hosts (id, server_id, name, engine, host, port, admin_username, phpmyadmin_application_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, ?8)",
            params![
                id.to_string(),
                input.server_id.map(|s| s.to_string()),
                input.name,
                engine_to_str(input.engine),
                input.host,
                input.port,
                input.admin_username,
                now,
            ],
        )
        .map_err(|err| AppError::Storage(format!("failed to create the database host: {err}")))?;
        drop(conn);
        self.get_host(id)?.ok_or_else(|| AppError::Internal(format!("database host {id} vanished immediately after being created")))
    }

    pub fn get_host(&self, id: Uuid) -> AppResult<Option<DatabaseHost>> {
        let conn = self.lock();
        conn.query_row(&format!("{HOST_COLUMNS} FROM database_hosts WHERE id = ?1"), params![id.to_string()], row_to_host)
            .optional()
            .map_err(|err| AppError::Storage(format!("failed to load the database host: {err}")))
    }

    pub fn list_hosts(&self) -> AppResult<Vec<DatabaseHost>> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare(&format!("{HOST_COLUMNS} FROM database_hosts ORDER BY name COLLATE NOCASE"))
            .map_err(|err| AppError::Storage(format!("failed to prepare the database host query: {err}")))?;
        let rows = stmt.query_map([], row_to_host).map_err(|err| AppError::Storage(format!("failed to list database hosts: {err}")))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|err| AppError::Storage(format!("failed to read a database host row: {err}")))
    }

    /// `ON DELETE RESTRICT` (see `storage::migrations`) means this fails
    /// with a foreign-key violation while any `ApplicationDatabase` still
    /// references this host - surfaced as a clear `InvalidInput`, not a raw
    /// storage error, same pattern `ServerRepository::delete` already uses
    /// for the identical "still has X attached" shape.
    pub fn delete_host(&self, id: Uuid) -> AppResult<()> {
        let conn = self.lock();
        let affected = conn.execute("DELETE FROM database_hosts WHERE id = ?1", params![id.to_string()]).map_err(|err| {
            if is_foreign_key_violation(&err) {
                AppError::InvalidInput(format!("database host {id} still has databases provisioned on it - remove them first"))
            } else {
                AppError::Storage(format!("failed to delete the database host: {err}"))
            }
        })?;
        if affected == 0 {
            return Err(AppError::NotFound(format!("database host {id}")));
        }
        Ok(())
    }

    /// Links (or unlinks, `application_id: None`) the built-in phpMyAdmin
    /// Blueprint instance deployed for this host (Section 12.3) - set once
    /// an admin deploys one, separate from `create_host` since a host is
    /// usually registered before phpMyAdmin exists to point it at.
    pub fn update_phpmyadmin_application(&self, id: Uuid, application_id: Option<Uuid>) -> AppResult<DatabaseHost> {
        let conn = self.lock();
        let affected = conn
            .execute(
                "UPDATE database_hosts SET phpmyadmin_application_id = ?2, updated_at = ?3 WHERE id = ?1",
                params![id.to_string(), application_id.map(|a| a.to_string()), Utc::now().to_rfc3339()],
            )
            .map_err(|err| AppError::Storage(format!("failed to update the database host: {err}")))?;
        if affected == 0 {
            return Err(AppError::NotFound(format!("database host {id}")));
        }
        drop(conn);
        self.get_host(id)?.ok_or_else(|| AppError::NotFound(format!("database host {id}")))
    }

    // ---- ApplicationDatabase ----

    /// `database_name` must be unique per `database_host_id` (the table's
    /// own `UNIQUE(database_host_id, database_name)` constraint, checked
    /// atomically by SQLite itself rather than a separate pre-check race) -
    /// surfaced as `InvalidInput`, not a raw storage error.
    pub fn create_database(&self, input: &CreateApplicationDatabaseInput) -> AppResult<ApplicationDatabase> {
        let conn = self.lock();
        let id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO application_databases (id, application_id, database_host_id, database_name, username, connections_from, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id.to_string(),
                input.application_id.to_string(),
                input.database_host_id.to_string(),
                input.database_name,
                input.username,
                input.connections_from,
                now,
            ],
        )
        .map_err(|err| {
            if is_unique_violation(&err) {
                AppError::InvalidInput(format!("'{}' already exists on this database host", input.database_name))
            } else {
                AppError::Storage(format!("failed to create the database: {err}"))
            }
        })?;
        drop(conn);
        self.get_database(id)?.ok_or_else(|| AppError::Internal(format!("database {id} vanished immediately after being created")))
    }

    pub fn get_database(&self, id: Uuid) -> AppResult<Option<ApplicationDatabase>> {
        let conn = self.lock();
        conn.query_row(&format!("{DATABASE_COLUMNS} FROM application_databases WHERE id = ?1"), params![id.to_string()], row_to_database)
            .optional()
            .map_err(|err| AppError::Storage(format!("failed to load the database: {err}")))
    }

    pub fn list_databases(&self, application_id: Uuid) -> AppResult<Vec<ApplicationDatabase>> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare(&format!("{DATABASE_COLUMNS} FROM application_databases WHERE application_id = ?1 ORDER BY database_name COLLATE NOCASE"))
            .map_err(|err| AppError::Storage(format!("failed to prepare the database query: {err}")))?;
        let rows = stmt
            .query_map(params![application_id.to_string()], row_to_database)
            .map_err(|err| AppError::Storage(format!("failed to list databases: {err}")))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|err| AppError::Storage(format!("failed to read a database row: {err}")))
    }

    pub fn delete_database(&self, id: Uuid) -> AppResult<()> {
        let conn = self.lock();
        let affected = conn
            .execute("DELETE FROM application_databases WHERE id = ?1", params![id.to_string()])
            .map_err(|err| AppError::Storage(format!("failed to delete the database: {err}")))?;
        if affected == 0 {
            return Err(AppError::NotFound(format!("database {id}")));
        }
        Ok(())
    }
}

const HOST_COLUMNS: &str =
    "SELECT id, server_id, name, engine, host, port, admin_username, phpmyadmin_application_id, created_at, updated_at";

fn row_to_host(row: &rusqlite::Row) -> rusqlite::Result<DatabaseHost> {
    Ok(DatabaseHost {
        id: parse_uuid(row.get::<_, String>(0)?),
        server_id: row.get::<_, Option<String>>(1)?.map(parse_uuid),
        name: row.get(2)?,
        engine: engine_from_str(&row.get::<_, String>(3)?),
        host: row.get(4)?,
        port: row.get(5)?,
        admin_username: row.get(6)?,
        phpmyadmin_application_id: row.get::<_, Option<String>>(7)?.map(parse_uuid),
        created_at: parse_timestamp(row.get::<_, String>(8)?),
        updated_at: parse_timestamp(row.get::<_, String>(9)?),
    })
}

const DATABASE_COLUMNS: &str = "SELECT id, application_id, database_host_id, database_name, username, connections_from, created_at";

fn row_to_database(row: &rusqlite::Row) -> rusqlite::Result<ApplicationDatabase> {
    Ok(ApplicationDatabase {
        id: parse_uuid(row.get::<_, String>(0)?),
        application_id: parse_uuid(row.get::<_, String>(1)?),
        database_host_id: parse_uuid(row.get::<_, String>(2)?),
        database_name: row.get(3)?,
        username: row.get(4)?,
        connections_from: row.get(5)?,
        created_at: parse_timestamp(row.get::<_, String>(6)?),
    })
}

fn engine_to_str(value: DatabaseEngine) -> &'static str {
    match value {
        DatabaseEngine::Mysql => "mysql",
        DatabaseEngine::Mariadb => "mariadb",
    }
}

fn engine_from_str(value: &str) -> DatabaseEngine {
    match value {
        "mariadb" => DatabaseEngine::Mariadb,
        _ => DatabaseEngine::Mysql,
    }
}

fn parse_uuid(value: String) -> Uuid {
    Uuid::parse_str(&value).expect("stored UUID column is always well-formed")
}

fn parse_timestamp(value: String) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(&value).expect("stored timestamp column is always well-formed").with_timezone(&Utc)
}

fn is_foreign_key_violation(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(sqlite_err, Some(message))
            if sqlite_err.code == rusqlite::ErrorCode::ConstraintViolation && message.contains("FOREIGN KEY")
    )
}

fn is_unique_violation(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(sqlite_err, Some(message))
            if sqlite_err.code == rusqlite::ErrorCode::ConstraintViolation && message.contains("UNIQUE")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Returns the repository plus the underlying file path, so tests that
    /// need a real `applications` row to satisfy `application_databases`'s
    /// own foreign key (see `create_stub_application`) can open a second,
    /// independent connection to the same file - `DatabaseRepository`
    /// itself has no `ApplicationRepository`-style `create()` and shouldn't
    /// grow one just for this.
    fn temp_repository() -> (DatabaseRepository, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("vibessh-database-repository-test-{}.sqlite3", Uuid::new_v4()));
        (DatabaseRepository::open(&path).unwrap(), path)
    }

    fn create_stub_application(path: &std::path::Path) -> Uuid {
        let conn = Connection::open(path).unwrap();
        let id = Uuid::new_v4();
        conn.execute(
            "INSERT INTO applications (id, name, blueprint_id, blueprint_version, runtime_type, working_directory, created_at, updated_at)
             VALUES (?1, 'Test App', 'generic', 1, 'localProcess', '/srv/app', ?2, ?2)",
            params![id.to_string(), Utc::now().to_rfc3339()],
        )
        .unwrap();
        id
    }

    fn host_input() -> CreateDatabaseHostInput {
        CreateDatabaseHostInput {
            server_id: None,
            name: "Main DB host".to_string(),
            engine: DatabaseEngine::Mysql,
            host: "127.0.0.1".to_string(),
            port: 3306,
            admin_username: "root".to_string(),
            admin_password: "hunter2".to_string(),
        }
    }

    #[test]
    fn create_then_get_a_host_round_trips_every_field() {
        let (repo, _path) = temp_repository();
        let created = repo.create_host(&host_input()).unwrap();

        assert_eq!(created.name, "Main DB host");
        assert_eq!(created.engine, DatabaseEngine::Mysql);
        assert_eq!(created.host, "127.0.0.1");
        assert_eq!(created.port, 3306);
        assert_eq!(created.admin_username, "root");
        assert_eq!(created.server_id, None);
        assert_eq!(created.phpmyadmin_application_id, None);

        let loaded = repo.get_host(created.id).unwrap().unwrap();
        assert_eq!(loaded.id, created.id);
        assert_eq!(loaded.name, created.name);
    }

    #[test]
    fn get_host_returns_none_for_an_unknown_id() {
        let (repo, _path) = temp_repository();
        assert!(repo.get_host(Uuid::new_v4()).unwrap().is_none());
    }

    #[test]
    fn list_hosts_is_sorted_by_name_case_insensitively() {
        let (repo, _path) = temp_repository();
        repo.create_host(&CreateDatabaseHostInput { name: "zeta".to_string(), ..host_input() }).unwrap();
        repo.create_host(&CreateDatabaseHostInput { name: "Alpha".to_string(), ..host_input() }).unwrap();

        let names: Vec<String> = repo.list_hosts().unwrap().into_iter().map(|h| h.name).collect();
        assert_eq!(names, vec!["Alpha".to_string(), "zeta".to_string()]);
    }

    #[test]
    fn delete_host_removes_the_row_and_is_not_found_afterward() {
        let (repo, _path) = temp_repository();
        let created = repo.create_host(&host_input()).unwrap();

        repo.delete_host(created.id).unwrap();
        assert!(repo.get_host(created.id).unwrap().is_none());
    }

    #[test]
    fn delete_host_of_an_unknown_id_is_not_found() {
        let (repo, _path) = temp_repository();
        assert!(matches!(repo.delete_host(Uuid::new_v4()), Err(AppError::NotFound(_))));
    }

    #[test]
    fn update_phpmyadmin_application_links_then_unlinks() {
        let (repo, path) = temp_repository();
        let host = repo.create_host(&host_input()).unwrap();
        let application_id = create_stub_application(&path);

        let linked = repo.update_phpmyadmin_application(host.id, Some(application_id)).unwrap();
        assert_eq!(linked.phpmyadmin_application_id, Some(application_id));

        let unlinked = repo.update_phpmyadmin_application(host.id, None).unwrap();
        assert_eq!(unlinked.phpmyadmin_application_id, None);
    }

    #[test]
    fn update_phpmyadmin_application_of_an_unknown_host_is_not_found() {
        let (repo, _path) = temp_repository();
        assert!(matches!(repo.update_phpmyadmin_application(Uuid::new_v4(), None), Err(AppError::NotFound(_))));
    }

    #[test]
    fn delete_host_with_a_provisioned_database_is_rejected_not_a_raw_storage_error() {
        let (repo, path) = temp_repository();
        let host = repo.create_host(&host_input()).unwrap();
        let application_id = create_stub_application(&path);
        repo.create_database(&CreateApplicationDatabaseInput {
            application_id,
            database_host_id: host.id,
            database_name: "vibessh_app1".to_string(),
            username: "vibessh_app1_user".to_string(),
            connections_from: "%".to_string(),
        })
        .unwrap();

        assert!(matches!(repo.delete_host(host.id), Err(AppError::InvalidInput(_))));
        assert!(repo.get_host(host.id).unwrap().is_some(), "the host must still exist after the rejected delete");
    }

    #[test]
    fn create_then_get_a_database_round_trips_every_field() {
        let (repo, path) = temp_repository();
        let host = repo.create_host(&host_input()).unwrap();
        let application_id = create_stub_application(&path);

        let created = repo
            .create_database(&CreateApplicationDatabaseInput {
                application_id,
                database_host_id: host.id,
                database_name: "vibessh_app1".to_string(),
                username: "vibessh_app1_user".to_string(),
                connections_from: "%".to_string(),
            })
            .unwrap();

        assert_eq!(created.application_id, application_id);
        assert_eq!(created.database_host_id, host.id);
        assert_eq!(created.database_name, "vibessh_app1");
        assert_eq!(created.username, "vibessh_app1_user");
        assert_eq!(created.connections_from, "%");

        let loaded = repo.get_database(created.id).unwrap().unwrap();
        assert_eq!(loaded.database_name, created.database_name);
    }

    #[test]
    fn creating_a_database_with_a_colliding_name_on_the_same_host_is_rejected() {
        let (repo, path) = temp_repository();
        let host = repo.create_host(&host_input()).unwrap();
        let input = CreateApplicationDatabaseInput {
            application_id: create_stub_application(&path),
            database_host_id: host.id,
            database_name: "vibessh_app1".to_string(),
            username: "vibessh_app1_user".to_string(),
            connections_from: "%".to_string(),
        };
        repo.create_database(&input).unwrap();

        let collision = repo.create_database(&CreateApplicationDatabaseInput { username: "different_user".to_string(), ..input });
        assert!(matches!(collision, Err(AppError::InvalidInput(_))));
    }

    #[test]
    fn the_same_database_name_is_allowed_on_two_different_hosts() {
        let (repo, path) = temp_repository();
        let host_a = repo.create_host(&host_input()).unwrap();
        let host_b = repo.create_host(&CreateDatabaseHostInput { name: "Secondary".to_string(), ..host_input() }).unwrap();

        for host in [&host_a, &host_b] {
            repo.create_database(&CreateApplicationDatabaseInput {
                application_id: create_stub_application(&path),
                database_host_id: host.id,
                database_name: "vibessh_app1".to_string(),
                username: "vibessh_app1_user".to_string(),
                connections_from: "%".to_string(),
            })
            .unwrap();
        }
    }

    #[test]
    fn list_databases_only_returns_that_applications_own_databases() {
        let (repo, path) = temp_repository();
        let host = repo.create_host(&host_input()).unwrap();
        let application_a = create_stub_application(&path);
        let application_b = create_stub_application(&path);
        repo.create_database(&CreateApplicationDatabaseInput {
            application_id: application_a,
            database_host_id: host.id,
            database_name: "app_a_db".to_string(),
            username: "app_a_user".to_string(),
            connections_from: "%".to_string(),
        })
        .unwrap();
        repo.create_database(&CreateApplicationDatabaseInput {
            application_id: application_b,
            database_host_id: host.id,
            database_name: "app_b_db".to_string(),
            username: "app_b_user".to_string(),
            connections_from: "%".to_string(),
        })
        .unwrap();

        let a_databases = repo.list_databases(application_a).unwrap();
        assert_eq!(a_databases.len(), 1);
        assert_eq!(a_databases[0].database_name, "app_a_db");
    }

    #[test]
    fn delete_database_removes_the_row_and_is_not_found_afterward() {
        let (repo, path) = temp_repository();
        let host = repo.create_host(&host_input()).unwrap();
        let created = repo
            .create_database(&CreateApplicationDatabaseInput {
                application_id: create_stub_application(&path),
                database_host_id: host.id,
                database_name: "vibessh_app1".to_string(),
                username: "vibessh_app1_user".to_string(),
                connections_from: "%".to_string(),
            })
            .unwrap();

        repo.delete_database(created.id).unwrap();
        assert!(repo.get_database(created.id).unwrap().is_none());
        // Deleting the database frees the host to be removed too.
        repo.delete_host(host.id).unwrap();
    }

    #[test]
    fn delete_database_of_an_unknown_id_is_not_found() {
        let (repo, _path) = temp_repository();
        assert!(matches!(repo.delete_database(Uuid::new_v4()), Err(AppError::NotFound(_))));
    }
}
