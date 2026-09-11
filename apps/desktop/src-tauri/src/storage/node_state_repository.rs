//! SQLite-backed storage for `node_desired_state`/`node_applied_state`
//! (Etap M3) - a separate repository struct from `ServerRepository`, same
//! one-concern-per-repository convention `DatabaseRepository`/
//! `ApplicationRepository` already follow, even though all four open the
//! same physical database file.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{NodeAppliedRecord, NodeSyncStatus};

pub struct NodeStateRepository {
    conn: Mutex<Connection>,
}

impl NodeStateRepository {
    /// `db_path` is the same `servers.sqlite3` every other repository
    /// opens - `schema::migrate` is a no-op once the file's
    /// `user_version` is already current, so it's safe to call from here
    /// too.
    pub fn open(db_path: &Path) -> AppResult<Self> {
        // Pragmas (WAL, busy timeout, foreign keys) live in one place -
        // see `storage::open_connection` for why they matter with nine
        // connections open on the same file.
        let mut conn = super::open_connection(db_path, "node state")?;
        super::schema::migrate(&mut conn, db_path, "node state")?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().expect("node state repository connection mutex poisoned")
    }

    /// Increments (or, for a Node's first-ever reconcile, creates at `1`)
    /// the desired revision and returns the new value - one SQLite
    /// transaction via `INSERT ... ON CONFLICT DO UPDATE`, which is what
    /// makes this safe to call concurrently without a separate read-then-
    /// write race (see the architecture doc's own "which operations need
    /// global serialization" answer: this is one of them, and SQLite's own
    /// single-writer semantics are the serialization point, not
    /// application-level locking).
    pub fn bump_desired_revision(&self, server_id: Uuid) -> AppResult<u64> {
        let now = Utc::now().to_rfc3339();
        self.lock()
            .query_row(
                "INSERT INTO node_desired_state (server_id, desired_revision, desired_state_json, updated_at)
                 VALUES (?1, 1, '{}', ?2)
                 ON CONFLICT(server_id) DO UPDATE SET desired_revision = desired_revision + 1, updated_at = ?2
                 RETURNING desired_revision",
                params![server_id.to_string(), now],
                |row| row.get::<_, i64>(0),
            )
            .map(|revision| revision as u64)
            .map_err(|err| AppError::Storage(format!("failed to bump the desired revision: {err}")))
    }

    pub fn get_desired_revision(&self, server_id: Uuid) -> AppResult<u64> {
        self.lock()
            .query_row("SELECT desired_revision FROM node_desired_state WHERE server_id = ?1", params![server_id.to_string()], |row| {
                row.get::<_, i64>(0)
            })
            .optional()
            .map(|revision| revision.unwrap_or(0) as u64)
            .map_err(|err| AppError::Storage(format!("failed to read the desired revision: {err}")))
    }

    /// Records a real reconcile outcome - `status` is a short machine-
    /// readable tag (`"applied"`/`"failed"`), not user-facing text on its
    /// own; the frontend renders it via i18n keyed on this value, the same
    /// way `ApplicationStatus`/`AgentStatus` string columns already work.
    pub fn set_applied(&self, server_id: Uuid, applied_revision: u64, status: &str, error: Option<&str>) -> AppResult<()> {
        let now = Utc::now().to_rfc3339();
        self.lock()
            .execute(
                "INSERT INTO node_applied_state (server_id, applied_revision, applied_at, last_reconcile_status, last_error)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(server_id) DO UPDATE SET
                    applied_revision = excluded.applied_revision,
                    applied_at = excluded.applied_at,
                    last_reconcile_status = excluded.last_reconcile_status,
                    last_error = excluded.last_error",
                params![server_id.to_string(), applied_revision as i64, now, status, error],
            )
            .map_err(|err| AppError::Storage(format!("failed to record the applied state: {err}")))?;
        Ok(())
    }

    /// Records a reconcile attempt that did NOT result in a confirmed
    /// applied state - either it never reached the Node at all (no live
    /// session, `status = "offline_pending"`) or it reached the Node but
    /// the Agent's own ack said it couldn't apply it (`status = "failed"`).
    /// `applied_revision` is deliberately untouched either way - there is
    /// nothing new to confirm as applied, so a Node's last *confirmed*
    /// revision is never overwritten by an attempt that didn't succeed.
    pub fn record_reconcile_failure(&self, server_id: Uuid, status: &str, error: &str) -> AppResult<()> {
        let now = Utc::now().to_rfc3339();
        self.lock()
            .execute(
                "INSERT INTO node_applied_state (server_id, applied_revision, applied_at, last_reconcile_status, last_error)
                 VALUES (?1, 0, ?2, ?3, ?4)
                 ON CONFLICT(server_id) DO UPDATE SET last_reconcile_status = excluded.last_reconcile_status, last_error = excluded.last_error",
                params![server_id.to_string(), now, status, error],
            )
            .map_err(|err| AppError::Storage(format!("failed to record the reconcile failure: {err}")))?;
        Ok(())
    }

    pub fn get_applied(&self, server_id: Uuid) -> AppResult<Option<NodeAppliedRecord>> {
        self.lock()
            .query_row(
                "SELECT server_id, applied_revision, applied_at, last_reconcile_status, last_error FROM node_applied_state WHERE server_id = ?1",
                params![server_id.to_string()],
                row_to_applied_record,
            )
            .optional()
            .map_err(|err| AppError::Storage(format!("failed to read the applied state: {err}")))
    }

    /// Never `NotFound` for a Node that's never had a reconcile attempt -
    /// `desired_revision`/`applied_revision` both default to `0`, which
    /// correctly reads as "in sync" (nothing has ever been asked for, so
    /// nothing can be pending) rather than an error the UI has to special-
    /// case.
    pub fn sync_status(&self, server_id: Uuid) -> AppResult<NodeSyncStatus> {
        let desired_revision = self.get_desired_revision(server_id)?;
        let applied_revision = self.get_applied(server_id)?.map(|record| record.applied_revision).unwrap_or(0);
        Ok(NodeSyncStatus { server_id, desired_revision, applied_revision, in_sync: desired_revision == applied_revision })
    }
}

fn row_to_applied_record(row: &rusqlite::Row) -> rusqlite::Result<NodeAppliedRecord> {
    Ok(NodeAppliedRecord {
        server_id: Uuid::parse_str(&row.get::<_, String>(0)?).expect("stored UUID column is always well-formed"),
        applied_revision: row.get::<_, i64>(1)? as u64,
        applied_at: row
            .get::<_, Option<String>>(2)?
            .map(|value| chrono::DateTime::parse_from_rfc3339(&value).expect("stored timestamp column is always well-formed").with_timezone(&Utc)),
        last_reconcile_status: row.get(3)?,
        last_error: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AuthenticationType, ServerInput};
    use crate::storage::server_repository::ServerRepository;

    /// `node_desired_state`/`node_applied_state` both foreign-key into
    /// `servers`, so every test needs a real Server row to reference, not
    /// just a random UUID - `ServerRepository` opens the same physical file
    /// as its own independent connection, same pattern every other
    /// repository's own tests already use when they need a real server
    /// present (see `storage::application_repository`'s tests).
    fn temp_repository() -> (NodeStateRepository, ServerRepository) {
        let path = std::env::temp_dir().join(format!("vibessh-node-state-test-{}.sqlite3", Uuid::new_v4()));
        (NodeStateRepository::open(&path).unwrap(), ServerRepository::open(&path).unwrap())
    }

    fn create_test_server(server_repo: &ServerRepository) -> Uuid {
        server_repo
            .create(&ServerInput {
                name: "Test Node".into(),
                host: "203.0.113.10".into(),
                ssh_port: 22,
                username: "root".into(),
                authentication_type: AuthenticationType::Password,
                private_key_path: None,
                group_id: None,
                password: Some("x".into()),
                key_passphrase: None,
            })
            .unwrap()
            .id
    }

    #[test]
    fn a_never_reconciled_node_reads_as_in_sync_at_zero() {
        let (repo, server_repo) = temp_repository();
        let server_id = create_test_server(&server_repo);
        let status = repo.sync_status(server_id).unwrap();
        assert_eq!(status.desired_revision, 0);
        assert_eq!(status.applied_revision, 0);
        assert!(status.in_sync);
    }

    #[test]
    fn bump_desired_revision_starts_at_one_and_increments_on_repeated_calls() {
        let (repo, server_repo) = temp_repository();
        let server_id = create_test_server(&server_repo);

        assert_eq!(repo.bump_desired_revision(server_id).unwrap(), 1);
        assert_eq!(repo.bump_desired_revision(server_id).unwrap(), 2);
        assert_eq!(repo.bump_desired_revision(server_id).unwrap(), 3);
        assert_eq!(repo.get_desired_revision(server_id).unwrap(), 3);
    }

    #[test]
    fn set_applied_makes_the_node_read_as_in_sync_once_it_matches_the_desired_revision() {
        let (repo, server_repo) = temp_repository();
        let server_id = create_test_server(&server_repo);

        let desired = repo.bump_desired_revision(server_id).unwrap();
        assert!(!repo.sync_status(server_id).unwrap().in_sync, "a fresh desired bump with no ack yet must be out of sync");

        repo.set_applied(server_id, desired, "applied", None).unwrap();
        let status = repo.sync_status(server_id).unwrap();
        assert!(status.in_sync);
        assert_eq!(status.applied_revision, desired);

        let record = repo.get_applied(server_id).unwrap().unwrap();
        assert_eq!(record.last_reconcile_status.as_deref(), Some("applied"));
        assert!(record.applied_at.is_some());
    }

    #[test]
    fn record_reconcile_failure_leaves_the_applied_revision_untouched() {
        let (repo, server_repo) = temp_repository();
        let server_id = create_test_server(&server_repo);
        let desired = repo.bump_desired_revision(server_id).unwrap();
        repo.set_applied(server_id, desired, "applied", None).unwrap();

        // A later desired bump that never reaches the Node must not
        // silently mark the *old* applied_revision as if it were the new
        // one - and must not fabricate a match with the new desired_revision.
        let new_desired = repo.bump_desired_revision(server_id).unwrap();
        repo.record_reconcile_failure(server_id, "offline_pending", "no live session").unwrap();

        let status = repo.sync_status(server_id).unwrap();
        assert_eq!(status.applied_revision, desired, "applied_revision must stay at the last real ack");
        assert_eq!(status.desired_revision, new_desired);
        assert!(!status.in_sync);

        let record = repo.get_applied(server_id).unwrap().unwrap();
        assert_eq!(record.last_reconcile_status.as_deref(), Some("offline_pending"));
    }

    #[test]
    fn state_survives_a_reopen() {
        let path = std::env::temp_dir().join(format!("vibessh-node-state-reopen-test-{}.sqlite3", Uuid::new_v4()));
        let server_id = create_test_server(&ServerRepository::open(&path).unwrap());
        {
            let repo = NodeStateRepository::open(&path).unwrap();
            let revision = repo.bump_desired_revision(server_id).unwrap();
            repo.set_applied(server_id, revision, "applied", None).unwrap();
        }

        let repo = NodeStateRepository::open(&path).unwrap();
        let status = repo.sync_status(server_id).unwrap();
        assert_eq!(status.desired_revision, 1);
        assert_eq!(status.applied_revision, 1);
        assert!(status.in_sync);
    }
}
