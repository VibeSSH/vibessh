//! Versioned schema migrations for the local per-device SQLite store, using
//! SQLite's own `user_version` pragma to track which migrations have run
//! (via the `rusqlite_migration` crate). Before this, `ServerRepository::open`
//! ran bare `CREATE TABLE IF NOT EXISTS` statements on every launch - that
//! only ever works for a schema that never changes shape. Every future
//! schema change (new columns, new tables) is a new `M::up(...)` appended to
//! the end of this list - never edit an already-shipped migration, since a
//! user's existing database has already recorded it as applied.
use rusqlite_migration::{Migrations, M};

pub fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        // Migration 1: the schema as it already shipped (servers +
        // ssh_known_hosts) - captured as-is, not redesigned, so every
        // existing user's database applies it as a no-op structural match
        // and simply gets its user_version stamped to 1. Both tables are one
        // migration, not two - they shipped together as a single existing
        // baseline schema (see bootstrap_legacy_schema in
        // server_repository.rs, which stamps a pre-framework database
        // straight to this version without re-running the SQL).
        M::up(
            "CREATE TABLE servers (
                id                   TEXT PRIMARY KEY,
                name                 TEXT NOT NULL,
                host                 TEXT NOT NULL,
                ssh_port             INTEGER NOT NULL,
                username             TEXT NOT NULL,
                authentication_type  TEXT NOT NULL,
                private_key_path     TEXT,
                connection_mode      TEXT NOT NULL,
                agent_id             TEXT,
                agent_status         TEXT,
                group_id             TEXT,
                created_at           TEXT NOT NULL,
                updated_at           TEXT NOT NULL
            );
            CREATE TABLE ssh_known_hosts (
                server_id    TEXT PRIMARY KEY,
                fingerprint  TEXT NOT NULL
            );",
        ),
        // Migration 2: Applications (see docs/APPLICATIONS_ARCHITECTURE.md).
        // `server_id` is nullable (NULL = a Local application, running on
        // this device rather than a VibeSSH-managed remote server) and
        // ON DELETE RESTRICT rather than CASCADE or SET NULL - a server
        // with applications attached must not become deletable out from
        // under them by accident (see ServerRepository::delete's own
        // foreign-key-violation handling). `blueprint_id`/`blueprint_version`
        // are plain columns, not yet a foreign key to a `blueprints` table -
        // that table doesn't exist until the Blueprint schema phase; these
        // stay a soft reference until then; every read that renders a
        // blueprint name of a still-unknown id falls back to showing the
        // raw id rather than erroring.
        //
        // Ports and environment variables are real tables, not JSON blobs
        // on `applications` - both need real per-row CRUD and (ports)
        // collision queries (`WHERE application_id != ? AND internal_port =
        // ? AND bind_address = ?`), which a JSON column can't do without
        // parsing client-side first. `application_runtime_config` and
        // `application_metadata` stay JSON deliberately - their field set
        // genuinely varies per runtime_type/blueprint, so there's no fixed
        // column set to design against.
        M::up(
            "CREATE TABLE applications (
                id                   TEXT PRIMARY KEY,
                server_id            TEXT REFERENCES servers(id) ON DELETE RESTRICT,
                name                 TEXT NOT NULL,
                description          TEXT,
                blueprint_id         TEXT NOT NULL,
                blueprint_version    INTEGER NOT NULL,
                runtime_type         TEXT NOT NULL,
                working_directory    TEXT NOT NULL,
                status               TEXT NOT NULL DEFAULT 'unknown',
                last_status_check_at TEXT,
                created_at           TEXT NOT NULL,
                updated_at           TEXT NOT NULL
            );
            CREATE INDEX applications_server_id_idx ON applications (server_id);

            CREATE TABLE application_environment (
                application_id TEXT NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
                key            TEXT NOT NULL,
                value          TEXT NOT NULL,
                PRIMARY KEY (application_id, key)
            );

            CREATE TABLE application_ports (
                id             TEXT PRIMARY KEY,
                application_id TEXT NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
                name           TEXT NOT NULL,
                protocol       TEXT NOT NULL,
                bind_address   TEXT NOT NULL,
                internal_port  INTEGER NOT NULL,
                external_port  INTEGER,
                required       INTEGER NOT NULL DEFAULT 0,
                created_at     TEXT NOT NULL,
                updated_at     TEXT NOT NULL
            );
            CREATE INDEX application_ports_application_id_idx ON application_ports (application_id);

            CREATE TABLE application_runtime_config (
                application_id TEXT PRIMARY KEY REFERENCES applications(id) ON DELETE CASCADE,
                config_json    TEXT NOT NULL
            );

            CREATE TABLE application_metadata (
                application_id TEXT PRIMARY KEY REFERENCES applications(id) ON DELETE CASCADE,
                metadata_json  TEXT NOT NULL
            );",
        ),
        // Migration 3: health check configuration, straight on `applications`
        // rather than a new table - it's 1-3 scalar fields per application,
        // not a real per-row CRUD/collision concern the way ports are.
        // `health_check_type` defaults to `'process'` (the only kind every
        // existing row can honestly claim - a plain "is the process still
        // running" check, same as before this migration existed at all).
        // `health_check_port_id` references `application_ports` -
        // `ON DELETE SET NULL` so removing the port a health check pointed
        // at doesn't fail, it just leaves the check unable to run (treated
        // as Unknown, not an error) until reconfigured.
        M::up(
            "ALTER TABLE applications ADD COLUMN health_check_type TEXT NOT NULL DEFAULT 'process';
            ALTER TABLE applications ADD COLUMN health_check_port_id TEXT REFERENCES application_ports(id) ON DELETE SET NULL;
            ALTER TABLE applications ADD COLUMN health_check_http_path TEXT;",
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_internally_consistent() {
        migrations().validate().expect("migration list should validate");
    }

    #[test]
    fn to_latest_creates_both_tables_on_a_fresh_database() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        migrations().to_latest(&mut conn).unwrap();

        let table_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN ('servers', 'ssh_known_hosts')",
                (),
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 2);
    }

    #[test]
    fn to_latest_is_idempotent() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        migrations().to_latest(&mut conn).unwrap();
        // Re-opening an already-migrated database (every app launch after
        // the first) must not try to re-run migration 1 and hit "table
        // already exists".
        migrations().to_latest(&mut conn).unwrap();
    }

    #[test]
    fn migration_2_creates_every_applications_table() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        migrations().to_latest(&mut conn).unwrap();

        let table_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN (
                    'applications', 'application_environment', 'application_ports',
                    'application_runtime_config', 'application_metadata'
                )",
                (),
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 5);
    }

    #[test]
    fn migration_3_adds_health_check_columns_defaulting_to_process() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        migrations().to_latest(&mut conn).unwrap();

        conn.execute(
            "INSERT INTO applications (id, name, blueprint_id, blueprint_version, runtime_type, working_directory, created_at, updated_at)
             VALUES ('a1', 'App', 'generic', 1, 'localProcess', '/srv/app', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
            (),
        )
        .unwrap();

        let (health_check_type, port_id, http_path): (String, Option<String>, Option<String>) = conn
            .query_row("SELECT health_check_type, health_check_port_id, health_check_http_path FROM applications WHERE id = 'a1'", (), |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap();
        assert_eq!(health_check_type, "process");
        assert_eq!(port_id, None);
        assert_eq!(http_path, None);
    }

    #[test]
    fn a_server_with_an_application_attached_cannot_be_deleted() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrations().to_latest(&mut conn).unwrap();

        conn.execute(
            "INSERT INTO servers (id, name, host, ssh_port, username, authentication_type, connection_mode, created_at, updated_at)
             VALUES ('s1', 'Test', 'example.com', 22, 'root', 'password', 'ssh', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
            (),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO applications (id, server_id, name, blueprint_id, blueprint_version, runtime_type, working_directory, created_at, updated_at)
             VALUES ('a1', 's1', 'App', 'generic', 1, 'systemd', '/srv/app', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
            (),
        )
        .unwrap();

        let err = conn.execute("DELETE FROM servers WHERE id = 's1'", ()).unwrap_err();
        // Not `extended_code == SQLITE_CONSTRAINT_FOREIGNKEY` - SQLite's
        // *immediate* RESTRICT check (as opposed to a deferred one, raised
        // at COMMIT) reports as SQLITE_CONSTRAINT_TRIGGER (1811) instead,
        // despite the message correctly saying "FOREIGN KEY constraint
        // failed". Caught by this exact test, not assumed - see
        // ServerRepository::delete and is_foreign_key_violation, which
        // check message text for the same reason.
        let rusqlite::Error::SqliteFailure(sqlite_err, message) = &err else {
            panic!("expected a SqliteFailure, got {err:?}");
        };
        assert_eq!(sqlite_err.code, rusqlite::ErrorCode::ConstraintViolation);
        assert!(message.as_deref().unwrap_or_default().contains("FOREIGN KEY"), "unexpected message: {message:?}");
    }
}
