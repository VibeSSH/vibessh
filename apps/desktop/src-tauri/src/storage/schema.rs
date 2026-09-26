//! Everything that has to be true about the schema *before* a repository is
//! allowed to use it: that this build understands the database it just
//! opened, that a previous version of it is recoverable, and that no
//! already-applied migration has been quietly rewritten underneath it.
//!
//! All nine repositories used to call `migrations().to_latest()` directly, so
//! each of them independently decided what "the database is ready" meant, and
//! none of them checked any of the three things above. This is the one place
//! that decides now.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::Connection;
use sha2::{Digest, Sha256};

use crate::errors::{AppError, AppResult};
use crate::storage::migrations::{latest_version, migrations, step_sql};

/// Brings `conn`'s database up to the schema this build expects.
///
/// Runs at most once per database file per process. Nine repositories open
/// the same file during startup, and before this they all raced to run the
/// same migrations at once - survivable only because `open_connection` sets a
/// busy timeout, and needlessly expensive besides. The first caller does the
/// work; the rest see a database that is already current.
///
/// The order matters and is not arbitrary:
///
/// 1. **Stamp a pre-framework database.** This used to live in
///    `ServerRepository::open` alone, which made startup depend on that
///    repository being constructed first - migration 1 is a bare
///    `CREATE TABLE servers`, so any other repository winning the race on a
///    legacy database would have failed with "table already exists". Nothing
///    said so; it worked because of the order of nine lines in `lib.rs`.
/// 2. **Refuse a database from the future**, with a sentence naming the
///    cause. Otherwise this is a raw `rusqlite_migration` error that reads
///    like corruption.
/// 3. **Copy the file** before changing its shape, so a release that
///    migrates badly can be undone by hand even where `down` cannot express
///    it.
/// 4. Migrate.
/// 5. **Verify checksums**, so an already-shipped migration edited in place
///    is a loud failure rather than a schema that differs between a fresh
///    install and an upgraded one.
pub fn migrate(conn: &mut Connection, db_path: &Path, what: &str) -> AppResult<()> {
    let mut done = migrated_paths()
        .lock()
        .map_err(|_| AppError::Storage("the schema migration lock was poisoned by an earlier failure".into()))?;
    if done.contains(db_path) {
        return Ok(());
    }

    stamp_legacy_schema(conn)?;
    refuse_if_ahead(conn, what)?;
    back_up_before_upgrade(conn, db_path)?;
    migrations()
        .to_latest(conn)
        .map_err(|err| AppError::Storage(format!("failed to migrate the {what} database: {err}")))?;
    verify_checksums(conn)?;

    done.insert(db_path.to_path_buf());
    Ok(())
}

fn migrated_paths() -> &'static Mutex<HashSet<PathBuf>> {
    static MIGRATED: std::sync::OnceLock<Mutex<HashSet<PathBuf>>> = std::sync::OnceLock::new();
    MIGRATED.get_or_init(|| Mutex::new(HashSet::new()))
}

/// A database written before migrations existed carries `user_version = 0`
/// and already has the tables migration 1 would create. Stamp it to 1 so
/// `to_latest` treats migration 1 as done rather than re-running its
/// `CREATE TABLE`.
fn stamp_legacy_schema(conn: &Connection) -> AppResult<()> {
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", (), |row| row.get(0))
        .map_err(|err| AppError::Storage(format!("failed to read the database's user_version: {err}")))?;
    if user_version != 0 {
        return Ok(());
    }
    let has_servers_table: i64 = conn
        .query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'servers'", (), |row| row.get(0))
        .map_err(|err| AppError::Storage(format!("failed to inspect the database's existing tables: {err}")))?;
    if has_servers_table == 0 {
        return Ok(());
    }
    conn.execute_batch("PRAGMA user_version = 1")
        .map_err(|err| AppError::Storage(format!("failed to stamp the legacy database's schema version: {err}")))
}

/// Stops an older build from touching a database a newer one has already
/// migrated.
///
/// This is the failure an operator actually hits: run a new release, go back
/// to the previous one, and `to_latest` refuses a `user_version` it does not
/// recognise - with a message about migration definitions that reads like the
/// database is broken. It is not broken, nothing has been lost, and the fix
/// is to go forward again. That has to be what the error says.
fn refuse_if_ahead(conn: &Connection, what: &str) -> AppResult<()> {
    let user_version: usize = conn
        .query_row("PRAGMA user_version", (), |row| row.get::<_, i64>(0))
        .map_err(|err| AppError::Storage(format!("failed to read the {what} database's user_version: {err}")))?
        .try_into()
        .unwrap_or(0);
    let latest = latest_version();
    if user_version <= latest {
        return Ok(());
    }
    Err(AppError::DatabaseFromNewerVersion {
        what: what.to_string(),
        found: user_version,
        supported: latest,
    })
}

/// Copies the database file next to itself before any migration changes its
/// shape.
///
/// This is the rollback path `down` migrations are usually assumed to be and
/// mostly are not: a `down` has to have been written correctly, and to have
/// been *run*, for a bad release to be recoverable, and one of the sixteen
/// steps here cannot express one at all. A copy of the file works regardless
/// of any of that.
///
/// Named for the version being left behind, so several upgrades leave several
/// restore points rather than one. Never overwrites an existing backup: the
/// oldest copy of a given version is the one worth keeping, since a later
/// upgrade attempt may already have damaged the newer state.
///
/// A failure here does not stop the migration. Refusing to start because a
/// spare copy could not be made - a full disk, a read-only directory - would
/// turn a precaution into an outage.
fn back_up_before_upgrade(conn: &Connection, db_path: &Path) -> AppResult<()> {
    let user_version: usize = conn
        .query_row("PRAGMA user_version", (), |row| row.get::<_, i64>(0))
        .map_err(|err| AppError::Storage(format!("failed to read the database's user_version: {err}")))?
        .try_into()
        .unwrap_or(0);
    // Nothing to preserve: a database with no schema yet, or one already at
    // the version this build wants.
    if user_version == 0 || user_version >= latest_version() {
        return Ok(());
    }

    let backup_path = db_path.with_extension(format!("v{user_version}.bak"));
    if backup_path.exists() {
        return Ok(());
    }
    // `VACUUM INTO` rather than a file copy: the database is open, in WAL
    // mode, with nine connections on it, so the bytes on disk are not a
    // complete database on their own - the -wal file holds the rest. This
    // writes one consistent, self-contained file instead.
    let quoted = backup_path.to_string_lossy().replace('\'', "''");
    match conn.execute(&format!("VACUUM INTO '{quoted}'"), ()) {
        Ok(_) => log::info!("wrote a pre-migration backup of the database to {}", backup_path.display()),
        Err(err) => log::warn!("couldn't write a pre-migration backup to {}: {err}", backup_path.display()),
    }
    Ok(())
}

/// Catches an already-shipped migration that has been edited in place.
///
/// `rusqlite_migration` keys on `user_version` alone, so changing the SQL of
/// a migration a user's database has already recorded does *nothing* on their
/// machine while working perfectly on a fresh install. The two then have
/// different schemas and only one of them matches the code. This codebase has
/// already been bitten by exactly that shape of bug once - see
/// `files::sudo_user::ensure_helper_installed`, whose doc comment describes
/// it for a script rather than a schema.
///
/// The hashes are recorded the first time this runs, so it can only vouch for
/// changes made from that point on: a step edited *before* this existed is
/// invisible to it, and pretending otherwise would be worse than saying so.
/// Everything from migration 17 onwards is covered from the moment it ships.
fn verify_checksums(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migration_checksums (
            version  INTEGER PRIMARY KEY,
            checksum TEXT NOT NULL
        );",
    )
    .map_err(|err| AppError::Storage(format!("failed to prepare the migration checksum table: {err}")))?;

    for (index, sql) in step_sql().enumerate() {
        let version = index + 1;
        let checksum = checksum_of(sql);
        let recorded: Option<String> = conn
            .query_row("SELECT checksum FROM schema_migration_checksums WHERE version = ?1", [version], |row| row.get(0))
            .ok();
        match recorded {
            None => {
                conn.execute("INSERT INTO schema_migration_checksums (version, checksum) VALUES (?1, ?2)", rusqlite::params![version, checksum])
                    .map_err(|err| AppError::Storage(format!("failed to record the checksum for migration {version}: {err}")))?;
            }
            Some(recorded) if recorded == checksum => {}
            Some(_) => {
                // Deliberately fatal. The alternative is running against a
                // schema that differs from what the code was written for,
                // which fails later, somewhere else, as something that does
                // not look like a migration problem at all.
                return Err(AppError::Storage(format!(
                    "migration {version} has been changed since it was applied to this database. An already-shipped migration \
                     must never be edited in place - a database that has recorded it as applied will never re-run it, so this \
                     install and a fresh one would end up with different schemas. Add a new migration instead."
                )));
            }
        }
    }
    Ok(())
}

fn checksum_of(sql: &str) -> String {
    // Whitespace-insensitive: reindenting a migration to satisfy a formatter
    // does not change the schema it produces, and a guard that fires on
    // `cargo fmt` gets switched off rather than fixed.
    let normalised: String = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    hex::encode(Sha256::digest(normalised.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> PathBuf {
        std::env::temp_dir().join(format!("vibessh-schema-test-{}.sqlite3", uuid::Uuid::new_v4()))
    }

    #[test]
    fn a_database_from_the_future_is_refused_by_name() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&format!("PRAGMA user_version = {}", latest_version() + 3)).unwrap();

        let err = refuse_if_ahead(&conn, "server").unwrap_err();
        let message = err.to_string();
        // The two facts an operator needs: which way round the mismatch is,
        // and that their data is fine.
        assert!(message.contains("newer version of VibeSSH"), "{message}");
        assert!(message.contains("Nothing has been lost"), "{message}");
    }

    #[test]
    fn a_database_at_the_current_version_is_not_refused() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&format!("PRAGMA user_version = {}", latest_version())).unwrap();
        refuse_if_ahead(&conn, "server").unwrap();
    }

    #[test]
    fn editing_an_applied_migration_is_caught() {
        let conn = Connection::open_in_memory().unwrap();
        verify_checksums(&conn).unwrap();
        // Simulate migration 2's SQL having been rewritten after shipping.
        conn.execute("UPDATE schema_migration_checksums SET checksum = 'tampered' WHERE version = 2", ()).unwrap();

        let err = verify_checksums(&conn).unwrap_err();
        assert!(err.to_string().contains("migration 2 has been changed"), "{err}");
    }

    #[test]
    fn checksums_ignore_reindentation_but_not_content() {
        assert_eq!(checksum_of("CREATE TABLE t (a INT);"), checksum_of("CREATE   TABLE t\n  (a INT);"));
        assert_ne!(checksum_of("CREATE TABLE t (a INT);"), checksum_of("CREATE TABLE t (b INT);"));
    }

    #[test]
    fn a_pre_framework_database_is_stamped_rather_than_rebuilt() {
        let conn = Connection::open_in_memory().unwrap();
        // The shape a database written before migrations existed has: the
        // table, and no version.
        conn.execute_batch("CREATE TABLE servers (id TEXT PRIMARY KEY);").unwrap();
        stamp_legacy_schema(&conn).unwrap();

        let version: i64 = conn.query_row("PRAGMA user_version", (), |row| row.get(0)).unwrap();
        assert_eq!(version, 1);
    }

    #[test]
    fn a_fresh_database_is_not_stamped() {
        let conn = Connection::open_in_memory().unwrap();
        stamp_legacy_schema(&conn).unwrap();
        let version: i64 = conn.query_row("PRAGMA user_version", (), |row| row.get(0)).unwrap();
        assert_eq!(version, 0);
    }

    /// The point of the backup: after it runs there is a file next to the
    /// database holding the schema as it was *before* the upgrade.
    #[test]
    fn upgrading_leaves_a_restore_point_for_the_version_it_left() {
        let path = temp_db();
        let mut conn = crate::storage::open_connection(&path, "test").unwrap();
        // Put the database at an older version, the way an install from a
        // previous release would be.
        crate::storage::migrations::migrations().to_version(&mut conn, 9).unwrap();

        back_up_before_upgrade(&conn, &path).unwrap();

        let backup = path.with_extension("v9.bak");
        assert!(backup.exists(), "expected a pre-migration backup at {}", backup.display());
        let restored = Connection::open(&backup).unwrap();
        let version: i64 = restored.query_row("PRAGMA user_version", (), |row| row.get(0)).unwrap();
        assert_eq!(version, 9);

        drop(restored);
        drop(conn);
        let _ = std::fs::remove_file(&backup);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_already_current_database_is_not_backed_up() {
        let path = temp_db();
        let mut conn = crate::storage::open_connection(&path, "test").unwrap();
        migrate(&mut conn, &path, "test").unwrap();

        // Nothing to preserve, so nothing is written - otherwise every launch
        // would leave another copy of the database on disk.
        let stray: Vec<_> = std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().to_string_lossy().starts_with(&path.to_string_lossy().to_string()) && entry.path() != path)
            .collect();
        assert!(stray.iter().all(|e| !e.path().to_string_lossy().ends_with(".bak")), "unexpected backup files");

        drop(conn);
        let _ = std::fs::remove_file(&path);
    }
}
