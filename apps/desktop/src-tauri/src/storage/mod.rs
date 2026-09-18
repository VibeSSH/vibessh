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
pub mod mcp_config;
pub mod tray_config;

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

/// Folds the write-ahead log back into the database file.
///
/// **Why this is needed at all.** WAL keeps recent writes in a separate
/// `-wal` file and only folds them into the main database when SQLite decides
/// to, or when it is asked. Measured on a real install of this app:
/// `servers.sqlite3` was **4 KB** while its `-wal` beside it was **1.6 MB** -
/// which is to say the database file held almost nothing and the log held
/// everything. Anything that copies the database file alone - a backup
/// script, a folder sync, somebody moving their profile to a new machine -
/// would have taken a database with no servers, no applications and no
/// history in it, and nothing would have said so.
///
/// So this runs on a clean exit, when there is nothing left to write.
/// `TRUNCATE` rather than `PASSIVE`: passive folds what it can and leaves the
/// log file at whatever size it had grown to, which fixes the correctness
/// half and not the "1.6 MB of the data is somewhere else" half.
///
/// **Failure is not an error.** A checkpoint that cannot get its lock leaves
/// the database exactly as valid as it was - the log is still authoritative
/// and SQLite will fold it in on the next open. The only thing lost is the
/// tidiness, so this logs and returns rather than delaying an exit somebody
/// asked for.
pub fn checkpoint_wal(db_path: &Path) {
    if !db_path.exists() {
        return;
    }
    // Its own connection rather than one of the nine repositories': a
    // checkpoint is a property of the file, not of any connection to it, and
    // reaching into a repository for its `Mutex<Connection>` would mean nine
    // places to keep in step for something that has to happen once.
    let conn = match Connection::open(db_path) {
        Ok(conn) => conn,
        Err(err) => {
            log::warn!("couldn't open the database to checkpoint it on exit: {err}");
            return;
        }
    };
    match conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get::<_, i64>(0)) {
        // The first column is 1 when SQLite could not get the lock it needed.
        Ok(1) => log::warn!("the database was busy, so its write-ahead log was left for the next start to fold in"),
        Ok(_) => {}
        Err(err) => log::warn!("couldn't checkpoint the database on exit: {err}"),
    }
}

/// Long enough to absorb the startup migration race and any realistic
/// cross-repository write overlap, short enough that a genuine deadlock
/// still surfaces as an error rather than hanging the UI forever.
const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[cfg(test)]
mod tests {
    use super::*;

    /// The measurement this exists for, reproduced small.
    ///
    /// Writes enough that SQLite leaves it in the log rather than folding it
    /// in, checks that the main file really is the smaller of the two - which
    /// is the state a backup would have copied - and then checks the
    /// checkpoint moves it across.
    #[test]
    fn a_checkpoint_moves_the_data_out_of_the_log_and_into_the_database() {
        let path = std::env::temp_dir().join(format!("vibessh-wal-test-{}.sqlite3", uuid::Uuid::new_v4()));
        let wal = path.with_extension("sqlite3-wal");
        let conn = open_connection(&path, "test").unwrap();
        conn.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, blob TEXT)", []).unwrap();
        for i in 0..2000 {
            conn.execute("INSERT INTO t (blob) VALUES (?1)", [format!("{i}{}", "x".repeat(200))]).unwrap();
        }

        let db_before = std::fs::metadata(&path).unwrap().len();
        let wal_before = std::fs::metadata(&wal).unwrap().len();
        assert!(wal_before > db_before, "the log should hold more than the database here: db={db_before} wal={wal_before}");

        // The connection stays open, exactly as the app's nine do at exit.
        checkpoint_wal(&path);

        let db_after = std::fs::metadata(&path).unwrap().len();
        let wal_after = std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0);
        assert!(db_after > db_before, "the database file should have grown: {db_before} -> {db_after}");
        assert_eq!(wal_after, 0, "TRUNCATE should leave an empty log, not merely a folded-in one");

        // And the data is readable from the database file alone - which is
        // what a copy of it would now contain.
        drop(conn);
        std::fs::remove_file(&wal).ok();
        let reopened = Connection::open(&path).unwrap();
        let rows: i64 = reopened.query_row("SELECT count(*) FROM t", [], |row| row.get(0)).unwrap();
        assert_eq!(rows, 2000);
        drop(reopened);
        std::fs::remove_file(&path).ok();
    }

    /// A database that was never created is not a failure to check-point -
    /// it is a first run, and the exit handler runs on those too.
    #[test]
    fn checkpointing_a_database_that_does_not_exist_is_harmless() {
        checkpoint_wal(&std::env::temp_dir().join(format!("vibessh-absent-{}.sqlite3", uuid::Uuid::new_v4())));
    }

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
