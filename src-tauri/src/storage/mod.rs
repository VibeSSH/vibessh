//! `credentials` exists early because Etap E's pairing flow produces a
//! secret the moment it lands - "don't persist secrets in plaintext" isn't
//! optional just because the full server repository wasn't built yet.
//! `server_repository` (Etap 2) is the SQLite-backed store for server
//! records themselves; it never touches secrets, see `credentials`.

pub mod ai_config;
pub mod application_backup_repository;
pub mod application_repository;
pub mod application_template_config;
pub mod builtin_templates;
pub mod backup_destination_config;
pub mod cloud_config;
pub mod credentials;
pub mod database_repository;
pub mod dns_config;
pub mod dns_repository;
pub mod firewall_rule_repository;
pub mod log_capture;
pub mod migrations;
pub mod node_network_repository;
pub mod node_state_repository;
pub mod registry_credential_repository;
pub mod schema;
pub mod server_repository;

use std::path::Path;

use rusqlite::Connection;

use crate::errors::{AppError, AppResult};

/// Opens the one SQLite file every repository shares, with the pragmas that
/// make nine independent connections to it safe.
///
/// **Nine connections, not one.** `lib.rs` manages nine repositories, each
/// holding its own `Connection` behind its own `Mutex`, all pointing at the
/// same file. Those mutexes serialize nothing *across* repositories, so
/// concurrent access is genuinely concurrent at the SQLite level - and the
/// two pragmas below are what stop that from surfacing as failures:
///
/// - **`busy_timeout`.** SQLite's default is `0`: a connection that finds
///   the database locked returns `SQLITE_BUSY` immediately rather than
///   waiting. Two effects, both of which were reachable in normal use. At
///   startup, all nine repositories ran the migrations at once on a fresh
///   database; the losers of that race failed outright and `lib.rs`
///   propagated the error, so **the app intermittently refused to launch**.
///   (`schema::migrate` now does the work once per file per process, so that
///   particular race is gone at the source - but the timeout is still what
///   makes nine connections on one file safe.) At runtime, any write in one
///   repository concurrent with a
///   write in another surfaced to the user as
///   `storage error: database is locked`. A timeout turns both into a short
///   wait, which is what SQLite's locking model expects callers to do.
///
/// - **`journal_mode=WAL`.** In the default rollback journal, a writer
///   blocks every reader. Since this application reads constantly (status
///   polling, the dashboard, every list view) while writing occasionally,
///   that is exactly the wrong trade. WAL lets readers proceed during a
///   write. It is a persistent property of the database file, so setting it
///   on every open is harmless and self-healing for a file created before
///   this existed.
///
/// `foreign_keys` is per-connection and off by default in SQLite, so it has
/// to be set here too or every `ON DELETE CASCADE`/`RESTRICT` in the schema
/// is inert documentation rather than an enforced constraint.
pub fn open_connection(db_path: &Path, what: &str) -> AppResult<Connection> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| AppError::Storage(format!("failed to create the {what} database directory: {err}")))?;
    }
    let conn = Connection::open(db_path).map_err(|err| AppError::Storage(format!("failed to open the {what} database: {err}")))?;

    // `journal_mode` returns the resulting mode as a row, so it needs a
    // query rather than `pragma_update` (which rejects statements that
    // produce results).
    conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get::<_, String>(0))
        .map_err(|err| AppError::Storage(format!("failed to enable WAL on the {what} database: {err}")))?;
    conn.busy_timeout(BUSY_TIMEOUT)
        .map_err(|err| AppError::Storage(format!("failed to set the {what} database busy timeout: {err}")))?;
    conn.pragma_update(None, "foreign_keys", true)
        .map_err(|err| AppError::Storage(format!("failed to enable foreign key enforcement on the {what} database: {err}")))?;
    Ok(conn)
}

/// Long enough to absorb the startup migration race and any realistic
/// cross-repository write overlap, short enough that a genuine deadlock
/// still surfaces as an error rather than hanging the UI forever.
const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[cfg(test)]
mod tests {
    use super::*;

    /// The regression test for the startup race: opening the same database
    /// from several connections at once, the way `lib.rs` does, must not
    /// produce `SQLITE_BUSY`.
    #[test]
    fn many_connections_to_one_file_all_open_successfully() {
        let path = std::env::temp_dir().join(format!("vibessh-open-connection-test-{}.sqlite3", uuid::Uuid::new_v4()));
        let connections: Vec<Connection> = (0..9).map(|_| open_connection(&path, "test").expect("every open should succeed")).collect();
        assert_eq!(connections.len(), 9);

        // WAL is a persistent property of the file, so every connection
        // sees it, including ones opened after the first.
        for conn in &connections {
            let mode: String = conn.query_row("PRAGMA journal_mode", [], |row| row.get(0)).unwrap();
            assert_eq!(mode.to_lowercase(), "wal");
        }
        drop(connections);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn foreign_keys_are_enforced_on_every_connection() {
        let path = std::env::temp_dir().join(format!("vibessh-open-connection-fk-test-{}.sqlite3", uuid::Uuid::new_v4()));
        let conn = open_connection(&path, "test").unwrap();
        let enabled: bool = conn.query_row("PRAGMA foreign_keys", [], |row| row.get(0)).unwrap();
        assert!(enabled);
        drop(conn);
        let _ = std::fs::remove_file(&path);
    }
}
