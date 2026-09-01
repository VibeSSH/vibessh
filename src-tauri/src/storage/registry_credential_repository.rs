//! SQLite-backed storage for `registry_credentials` - the non-secret half
//! (registry host, username) of a Docker registry login; the real password
//! lives in the OS credential store, keyed by each row's own `id` - see
//! `models::RegistryCredential`'s own doc comment.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::RegistryCredential;

pub struct RegistryCredentialRepository {
    conn: Mutex<Connection>,
}

impl RegistryCredentialRepository {
    pub fn open(db_path: &Path) -> AppResult<Self> {
        // Pragmas (WAL, busy timeout, foreign keys) live in one place -
        // see `storage::open_connection` for why they matter with nine
        // connections open on the same file.
        let mut conn = super::open_connection(db_path, "registry credential")?;
        super::schema::migrate(&mut conn, db_path, "registry credential")?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().expect("registry credential repository connection mutex poisoned")
    }

    pub fn list(&self) -> AppResult<Vec<RegistryCredential>> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare("SELECT id, registry, username, created_at FROM registry_credentials ORDER BY registry")
            .map_err(|err| AppError::Storage(format!("failed to prepare the registry credential list query: {err}")))?;
        let rows = stmt
            .query_map([], row_to_credential)
            .map_err(|err| AppError::Storage(format!("failed to list registry credentials: {err}")))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|err| AppError::Storage(format!("failed to read a registry credential row: {err}")))
    }

    /// `None` if no credential is stored for this exact registry host -
    /// `services::application_service`'s pre-pull login step treats that as
    /// "pull anonymously," not an error.
    pub fn find_by_registry(&self, registry: &str) -> AppResult<Option<RegistryCredential>> {
        self.lock()
            .query_row("SELECT id, registry, username, created_at FROM registry_credentials WHERE registry = ?1", params![registry], row_to_credential)
            .optional()
            .map_err(|err| AppError::Storage(format!("failed to look up the registry credential: {err}")))
    }

    pub fn create(&self, registry: &str, username: &str) -> AppResult<RegistryCredential> {
        let conn = self.lock();
        let id = Uuid::new_v4();
        let created_at = Utc::now();
        conn.execute(
            "INSERT INTO registry_credentials (id, registry, username, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![id.to_string(), registry, username, created_at.to_rfc3339()],
        )
        .map_err(|err| AppError::Storage(format!("failed to create the registry credential: {err}")))?;
        Ok(RegistryCredential { id, registry: registry.to_string(), username: username.to_string(), created_at })
    }

    pub fn update_username(&self, id: Uuid, username: &str) -> AppResult<()> {
        let affected = self
            .lock()
            .execute("UPDATE registry_credentials SET username = ?2 WHERE id = ?1", params![id.to_string(), username])
            .map_err(|err| AppError::Storage(format!("failed to update the registry credential: {err}")))?;
        if affected == 0 {
            return Err(AppError::NotFound(format!("registry credential {id}")));
        }
        Ok(())
    }

    pub fn delete(&self, id: Uuid) -> AppResult<()> {
        let affected = self
            .lock()
            .execute("DELETE FROM registry_credentials WHERE id = ?1", params![id.to_string()])
            .map_err(|err| AppError::Storage(format!("failed to delete the registry credential: {err}")))?;
        if affected == 0 {
            return Err(AppError::NotFound(format!("registry credential {id}")));
        }
        Ok(())
    }
}

fn row_to_credential(row: &rusqlite::Row) -> rusqlite::Result<RegistryCredential> {
    Ok(RegistryCredential {
        id: Uuid::parse_str(&row.get::<_, String>(0)?).expect("stored UUID column is always well-formed"),
        registry: row.get(1)?,
        username: row.get(2)?,
        created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(3)?)
            .expect("stored timestamp column is always well-formed")
            .with_timezone(&Utc),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_repository() -> RegistryCredentialRepository {
        let path = std::env::temp_dir().join(format!("vibessh-registry-credential-test-{}.sqlite3", Uuid::new_v4()));
        RegistryCredentialRepository::open(&path).unwrap()
    }

    #[test]
    fn create_then_list_round_trips_every_field() {
        let repo = temp_repository();
        let created = repo.create("ghcr.io", "octocat").unwrap();
        assert_eq!(created.registry, "ghcr.io");
        assert_eq!(created.username, "octocat");

        let listed = repo.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, created.id);
    }

    #[test]
    fn find_by_registry_returns_none_when_nothing_is_stored_for_that_host() {
        let repo = temp_repository();
        assert!(repo.find_by_registry("docker.io").unwrap().is_none());
    }

    #[test]
    fn find_by_registry_returns_the_matching_row() {
        let repo = temp_repository();
        repo.create("docker.io", "someuser").unwrap();
        let found = repo.find_by_registry("docker.io").unwrap().unwrap();
        assert_eq!(found.username, "someuser");
    }

    #[test]
    fn a_second_credential_for_the_same_registry_is_rejected_by_the_unique_index() {
        let repo = temp_repository();
        repo.create("docker.io", "first").unwrap();
        assert!(repo.create("docker.io", "second").is_err());
    }

    #[test]
    fn update_username_changes_the_row_in_place() {
        let repo = temp_repository();
        let created = repo.create("docker.io", "old-name").unwrap();
        repo.update_username(created.id, "new-name").unwrap();
        let found = repo.find_by_registry("docker.io").unwrap().unwrap();
        assert_eq!(found.id, created.id);
        assert_eq!(found.username, "new-name");
    }

    #[test]
    fn delete_removes_the_row() {
        let repo = temp_repository();
        let created = repo.create("docker.io", "someuser").unwrap();
        repo.delete(created.id).unwrap();
        assert!(repo.list().unwrap().is_empty());
    }

    #[test]
    fn delete_of_an_unknown_id_is_not_found() {
        let repo = temp_repository();
        let err = repo.delete(Uuid::new_v4()).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }
}
