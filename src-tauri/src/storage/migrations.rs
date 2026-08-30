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
}
