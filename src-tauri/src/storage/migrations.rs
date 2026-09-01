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
        // Migration 4: Application Databases - Phase 11 *foundation only*
        // (docs/APPLICATIONS_ARCHITECTURE.md Section 12 / Section 10 phase
        // list). Schema + types land here; the actual provisioning
        // (`mysql`/`mariadb` CLI execution over SSH), the phpMyAdmin
        // Blueprint, and the Databases tab UI are deliberately not built
        // yet - these tables exist so that later work has real, tested
        // storage rather than designing it from scratch. `server_id`
        // nullable (a shared/external DB host that isn't itself a VibeSSH
        // Server stays representable) and ON DELETE RESTRICT (same
        // reasoning `applications.server_id` already uses - a Server with a
        // database host attached must not become deletable out from under
        // it by accident). `admin_password`/the generated per-database
        // user's password both live in the OS keyring
        // (storage::credentials, SecretKind::DatabaseHostAdmin /
        // SecretKind::ApplicationDatabaseUser), keyed by each row's own id -
        // never a column here, same rule every other secret in this
        // codebase follows.
        M::up(
            "CREATE TABLE database_hosts (
                id                         TEXT PRIMARY KEY,
                server_id                  TEXT REFERENCES servers(id) ON DELETE RESTRICT,
                name                       TEXT NOT NULL,
                engine                     TEXT NOT NULL,
                host                       TEXT NOT NULL,
                port                       INTEGER NOT NULL DEFAULT 3306,
                admin_username             TEXT NOT NULL,
                phpmyadmin_application_id  TEXT REFERENCES applications(id) ON DELETE SET NULL,
                created_at                 TEXT NOT NULL,
                updated_at                 TEXT NOT NULL
            );
            CREATE INDEX database_hosts_server_id_idx ON database_hosts (server_id);

            CREATE TABLE application_databases (
                id                TEXT PRIMARY KEY,
                application_id    TEXT NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
                database_host_id  TEXT NOT NULL REFERENCES database_hosts(id) ON DELETE RESTRICT,
                database_name     TEXT NOT NULL,
                username          TEXT NOT NULL,
                connections_from  TEXT NOT NULL DEFAULT '%',
                created_at        TEXT NOT NULL,
                UNIQUE(database_host_id, database_name)
            );
            CREATE INDEX application_databases_application_id_idx ON application_databases (application_id);",
        ),
        // Migration 5 (Etap M1): persisted Node capability detection.
        // `node_capabilities_json` mirrors `NodeCapabilities` (currently just
        // `{"docker": bool}`) - NULL means "never probed", not "no
        // capabilities", so a Node added before this migration (or an
        // SSH-mode Node nobody has probed yet) reads back as unknown rather
        // than a false "Docker not available". A JSON blob rather than a
        // real `docker BOOLEAN` column deliberately mirrors how
        // `application_runtime_config.config_json` already stores a
        // per-row-varying capability set - this shape is expected to grow
        // (a `firewall` flag once Etap M2 needs it) without another
        // migration.
        M::up("ALTER TABLE servers ADD COLUMN node_capabilities_json TEXT;"),
        // Migration 6 (Etap M3): desired/applied state revisioning.
        // `desired_revision`/`applied_revision` are compared as plain
        // integers to answer "is this Node in sync" - see
        // `services::node_state_service`'s own doc comment for the full
        // reconcile flow. Deliberately two tables, not one: a Node's
        // *desired* state is authored by Desktop the instant something
        // changes (today: only a manual "Reconcile" click bumps it, since
        // Etap M3 has no real desired-state payload yet - see
        // `vibessh_protocol::NodeDesiredState`'s own doc comment), while its
        // *applied* state is only ever written back from a real ack the
        // Agent sent - keeping them separate means "what we asked for" and
        // "what's actually confirmed running" can never accidentally be
        // conflated into one write.
        M::up(
            "CREATE TABLE node_desired_state (
                server_id           TEXT PRIMARY KEY REFERENCES servers(id) ON DELETE CASCADE,
                desired_revision    INTEGER NOT NULL DEFAULT 0,
                desired_state_json  TEXT NOT NULL,
                updated_at          TEXT NOT NULL
            );
            CREATE TABLE node_applied_state (
                server_id             TEXT PRIMARY KEY REFERENCES servers(id) ON DELETE CASCADE,
                applied_revision      INTEGER NOT NULL DEFAULT 0,
                applied_at            TEXT,
                last_reconcile_status TEXT,
                last_error            TEXT
            );",
        ),
        // Migration 7 (Etap M4): Vibe Network (WireGuard mesh) membership.
        // Desktop is the sole IPAM authority - `wireguard_ip` is allocated
        // sequentially in a fixed CIDR (see `network::wireguard`'s own doc
        // comment) and UNIQUE enforces that at the database level, not just
        // in application logic. Only the Node's own PUBLIC key is stored
        // here - the private key never leaves the Node itself, see
        // `services::network_service`'s own doc comment for the full
        // reasoning.
        M::up(
            "CREATE TABLE node_network_members (
                server_id            TEXT PRIMARY KEY REFERENCES servers(id) ON DELETE CASCADE,
                wireguard_ip         TEXT NOT NULL UNIQUE,
                wireguard_public_key TEXT NOT NULL,
                joined_at            TEXT NOT NULL
            );",
        ),
        // Migration 8 (Etap M4): the user-facing "Application Network"
        // intent behind a port - see `models::PortVisibility`'s own doc
        // comment. Existing ports default to `'public'`, matching their
        // actual behavior today (unconditional `-p` publishing).
        M::up("ALTER TABLE application_ports ADD COLUMN visibility TEXT NOT NULL DEFAULT 'public';"),
        // Migration 9 (Etap M4): Private DNS. `application_id` is UNIQUE -
        // one alias per service. The IP a Node renders for `hostname` is
        // resolved at render time via `applications.server_id ->
        // node_network_members.wireguard_ip`, never baked into this row -
        // see `services::dns_service`'s own doc comment for why that's
        // what makes a service's DNS name survive moving to another Node.
        M::up(
            "CREATE TABLE dns_records (
                id              TEXT PRIMARY KEY,
                application_id  TEXT NOT NULL UNIQUE REFERENCES applications(id) ON DELETE CASCADE,
                hostname        TEXT NOT NULL UNIQUE,
                created_at      TEXT NOT NULL
            );",
        ),
        // Migration 10: Application backups. Two tables, not columns bolted
        // onto `applications` - same reasoning `node_desired_state`/
        // `node_applied_state` already established for keeping a growable,
        // optional concern out of the one row every other Application read
        // already selects. `application_backups` is the history (one row per
        // archive actually written); `application_backup_schedules` is
        // per-Application config, one row max (absent = never configured,
        // same "NULL means unset" idiom `servers.node_capabilities_json`
        // uses) rather than a boolean-plus-nullable-columns trio on
        // `applications` itself. The archive bytes themselves live on the
        // Application's own filesystem (`.vibessh-backups/<file>.zip` inside
        // its working directory, written through the same
        // `ApplicationFileProvider` the Files tab already uses) - these
        // tables are metadata only, so a backup is always exactly where the
        // Files tab would also show it, not a second hidden copy elsewhere.
        // `kind` is `'manual'` or `'scheduled'`, not enforced by a CHECK
        // constraint - same policy every other free-text enum-shaped column
        // in this schema (e.g. `application_databases.engine`) already
        // follows, validated in Rust instead.
        M::up(
            "CREATE TABLE application_backups (
                id              TEXT PRIMARY KEY,
                application_id  TEXT NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
                file_name       TEXT NOT NULL,
                size_bytes      INTEGER NOT NULL,
                kind            TEXT NOT NULL,
                created_at      TEXT NOT NULL
            );
            CREATE INDEX application_backups_application_id_idx ON application_backups (application_id);

            CREATE TABLE application_backup_schedules (
                application_id   TEXT PRIMARY KEY REFERENCES applications(id) ON DELETE CASCADE,
                enabled          INTEGER NOT NULL DEFAULT 0,
                interval_hours   INTEGER NOT NULL DEFAULT 24,
                retention_count  INTEGER NOT NULL DEFAULT 5,
                updated_at       TEXT NOT NULL
            );",
        ),
        // Migration 11: marks an environment variable as a secret. A
        // secret row's `value` column is never the real value - the real
        // value lives in the OS credential store, keyed by
        // `(application_id, key)` (see `storage::credentials::
        // store_environment_secret`), the same "never plaintext in SQLite"
        // rule `database_repository` already applies to database host/user
        // passwords. Existing rows default to `0` (not secret) - they were
        // already plaintext in this same column, so nothing changes for
        // them.
        M::up("ALTER TABLE application_environment ADD COLUMN is_secret INTEGER NOT NULL DEFAULT 0;"),
        // Migration 12: manual firewall rules - a port a user wants open on
        // a Node for a reason that isn't tied to any Application's own
        // published port (the design doc's own "full configuration" ask).
        // A real table, not a JSON blob: `services::firewall_service::
        // desired_rules` already builds its rule list by querying real
        // tables (`application_ports`, `node_network_members`), and this
        // needs the exact same per-row CRUD - add one, remove one, list
        // them for one Node - a JSON column can't do without parsing
        // client-side first. `source_cidr` nullable (`NULL` = open to
        // anywhere) mirrors `FirewallRule::source_cidr`'s own shape exactly,
        // no translation needed between the stored row and the applied rule.
        M::up(
            "CREATE TABLE firewall_custom_rules (
                id          TEXT PRIMARY KEY,
                server_id   TEXT NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
                label       TEXT,
                protocol    TEXT NOT NULL,
                port        INTEGER NOT NULL,
                source_cidr TEXT,
                created_at  TEXT NOT NULL
            );
            CREATE INDEX firewall_custom_rules_server_id_idx ON firewall_custom_rules (server_id);",
        ),
        // Migration 13: S3-compatible backup destination support. `s3_key`
        // records which backups actually made it to the configured
        // destination (`NULL` = local-only, either because no destination
        // was configured at the time or the upload failed) - see
        // `models::ApplicationBackup::s3_key`'s own doc comment for why
        // `restore_backup` needs this. The two new retention columns are
        // nullable the same way `application_ports.external_port` already
        // is: `NULL` means "this rule is off," not zero - see
        // `models::SetBackupScheduleInput`'s own doc comment.
        M::up(
            "ALTER TABLE application_backups ADD COLUMN s3_key TEXT;
            ALTER TABLE application_backup_schedules ADD COLUMN retention_max_age_days INTEGER;
            ALTER TABLE application_backup_schedules ADD COLUMN retention_max_total_bytes INTEGER;",
        ),
        // Migration 14: private Docker registry credentials - so an
        // Application's image can come from a private Docker Hub repo,
        // ghcr.io, or any other authenticated registry, not just public
        // images. `password` is never a real column, same "never plaintext
        // in SQLite" rule migration 11 already applies - the real secret
        // lives in the OS credential store, keyed by this row's own `id`
        // (see `storage::credentials::store_registry_credential_password`).
        // One row per registry host, not per Application: the same
        // credential is reused by every Application that pulls from that
        // registry.
        M::up(
            "CREATE TABLE registry_credentials (
                id         TEXT PRIMARY KEY,
                registry   TEXT NOT NULL,
                username   TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE UNIQUE INDEX registry_credentials_registry_idx ON registry_credentials (registry);",
        ),
        // Migration 15: trust-on-first-use pin for an Agent-mode Node's
        // TLS certificate. The agent generates a self-signed certificate on
        // the Node and persists it, so chain validation can never apply -
        // identity has to come from remembering what we saw the first time,
        // exactly as `ssh_known_hosts` already does for SSH host keys.
        // NULL means "not pinned yet"; the first successful handshake
        // fills it in. Not a secret (a certificate fingerprint is public by
        // construction), so unlike credentials this belongs in the database
        // rather than the OS keyring.
        M::up("ALTER TABLE servers ADD COLUMN agent_certificate_fingerprint TEXT;"),
        // Migration 16: explicit Application-to-Application connections.
        // Until this existed every container joined one shared
        // `vibessh-net` bridge with a resolvable alias, so any Application
        // could reach any other Application's *unpublished* ports by name -
        // the one isolation guarantee the architecture claims that was not
        // actually enforced (`AUDIT_REPORT.md` S-018). Reachability is now
        // default-deny and this table is the allow-list.
        //
        // A row is an unordered pair, not an arrow: `runtime::docker`
        // implements a connection by putting both containers on a private
        // two-member Docker network, and a bridge network is inherently
        // bidirectional. The CHECK is what stops a caller storing a
        // one-way link the network layer would silently make two-way -
        // better to be unable to express it than to display a direction
        // that isn't real.
        M::up(
            "CREATE TABLE application_links (
                application_id TEXT NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
                peer_id        TEXT NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
                created_at     TEXT NOT NULL,
                PRIMARY KEY (application_id, peer_id),
                CHECK (application_id < peer_id)
            );
            CREATE INDEX application_links_peer_idx ON application_links (peer_id);",
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
    fn migration_4_creates_the_database_tables_with_a_working_uniqueness_constraint() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        migrations().to_latest(&mut conn).unwrap();

        let table_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN ('database_hosts', 'application_databases')",
                (),
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 2);

        conn.execute(
            "INSERT INTO applications (id, name, blueprint_id, blueprint_version, runtime_type, working_directory, created_at, updated_at)
             VALUES ('a1', 'App', 'generic', 1, 'localProcess', '/srv/app', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
            (),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO database_hosts (id, name, engine, host, port, admin_username, created_at, updated_at)
             VALUES ('h1', 'Main DB host', 'mysql', '127.0.0.1', 3306, 'root', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
            (),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO application_databases (id, application_id, database_host_id, database_name, username, connections_from, created_at)
             VALUES ('d1', 'a1', 'h1', 'vibessh_app1', 'vibessh_app1_user', '%', '2024-01-01T00:00:00Z')",
            (),
        )
        .unwrap();

        // Same database_host_id + database_name again must be rejected - the
        // UNIQUE constraint is what `database_repository` relies on instead
        // of doing its own pre-check race.
        let duplicate = conn.execute(
            "INSERT INTO application_databases (id, application_id, database_host_id, database_name, username, connections_from, created_at)
             VALUES ('d2', 'a1', 'h1', 'vibessh_app1', 'vibessh_app1_user_2', '%', '2024-01-01T00:00:00Z')",
            (),
        );
        assert!(duplicate.is_err());
    }

    #[test]
    fn migration_5_adds_a_nullable_node_capabilities_column() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        migrations().to_latest(&mut conn).unwrap();

        conn.execute(
            "INSERT INTO servers (id, name, host, ssh_port, username, authentication_type, connection_mode, created_at, updated_at)
             VALUES ('s1', 'Test', 'example.com', 22, 'root', 'password', 'ssh', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
            (),
        )
        .unwrap();

        let capabilities: Option<String> =
            conn.query_row("SELECT node_capabilities_json FROM servers WHERE id = 's1'", (), |row| row.get(0)).unwrap();
        assert_eq!(capabilities, None);

        conn.execute("UPDATE servers SET node_capabilities_json = '{\"docker\":true}' WHERE id = 's1'", ()).unwrap();
        let capabilities: Option<String> =
            conn.query_row("SELECT node_capabilities_json FROM servers WHERE id = 's1'", (), |row| row.get(0)).unwrap();
        assert_eq!(capabilities, Some("{\"docker\":true}".to_string()));
    }

    #[test]
    fn migration_6_creates_the_node_state_tables_scoped_to_one_row_per_server() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrations().to_latest(&mut conn).unwrap();

        let table_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN ('node_desired_state', 'node_applied_state')",
                (),
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 2);

        conn.execute(
            "INSERT INTO servers (id, name, host, ssh_port, username, authentication_type, connection_mode, created_at, updated_at)
             VALUES ('s1', 'Test', 'example.com', 22, 'root', 'password', 'agent', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
            (),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO node_desired_state (server_id, desired_revision, desired_state_json, updated_at) VALUES ('s1', 1, '{}', '2024-01-01T00:00:00Z')",
            (),
        )
        .unwrap();
        conn.execute("INSERT INTO node_applied_state (server_id, applied_revision) VALUES ('s1', 0)", ()).unwrap();

        // One row per server_id, not a growing history - a second insert
        // for the same server must collide on the primary key, the same
        // "upsert, never append" shape the repository relies on.
        let duplicate = conn.execute(
            "INSERT INTO node_desired_state (server_id, desired_revision, desired_state_json, updated_at) VALUES ('s1', 2, '{}', '2024-01-01T00:00:00Z')",
            (),
        );
        assert!(duplicate.is_err());

        // Deleting the Server cascades - no orphaned state left behind.
        conn.execute("DELETE FROM servers WHERE id = 's1'", ()).unwrap();
        let remaining: i64 = conn.query_row("SELECT count(*) FROM node_desired_state", (), |row| row.get(0)).unwrap();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn migration_7_creates_node_network_members_with_a_unique_ip_constraint() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrations().to_latest(&mut conn).unwrap();

        conn.execute(
            "INSERT INTO servers (id, name, host, ssh_port, username, authentication_type, connection_mode, created_at, updated_at)
             VALUES ('s1', 'Node A', 'a.example.com', 22, 'root', 'password', 'ssh', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z'),
                    ('s2', 'Node B', 'b.example.com', 22, 'root', 'password', 'ssh', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
            (),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO node_network_members (server_id, wireguard_ip, wireguard_public_key, joined_at) VALUES ('s1', '10.77.0.1', 'pubkeyA', '2024-01-01T00:00:00Z')",
            (),
        )
        .unwrap();

        // The same IP for a second Node must be rejected - IPAM uniqueness
        // is enforced by the schema, not just application logic.
        let duplicate_ip = conn.execute(
            "INSERT INTO node_network_members (server_id, wireguard_ip, wireguard_public_key, joined_at) VALUES ('s2', '10.77.0.1', 'pubkeyB', '2024-01-01T00:00:00Z')",
            (),
        );
        assert!(duplicate_ip.is_err());

        conn.execute(
            "INSERT INTO node_network_members (server_id, wireguard_ip, wireguard_public_key, joined_at) VALUES ('s2', '10.77.0.2', 'pubkeyB', '2024-01-01T00:00:00Z')",
            (),
        )
        .unwrap();

        // Deleting the Server cascades - no orphaned membership left behind.
        conn.execute("DELETE FROM servers WHERE id = 's1'", ()).unwrap();
        let remaining: i64 = conn.query_row("SELECT count(*) FROM node_network_members", (), |row| row.get(0)).unwrap();
        assert_eq!(remaining, 1);
    }

    #[test]
    fn migration_8_defaults_existing_ports_to_public_visibility() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        migrations().to_latest(&mut conn).unwrap();

        conn.execute(
            "INSERT INTO applications (id, name, blueprint_id, blueprint_version, runtime_type, working_directory, created_at, updated_at)
             VALUES ('a1', 'App', 'generic', 1, 'localProcess', '/srv/app', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
            (),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO application_ports (id, application_id, name, protocol, bind_address, internal_port, created_at, updated_at)
             VALUES ('p1', 'a1', 'game', 'tcp', '0.0.0.0', 25565, '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
            (),
        )
        .unwrap();

        let visibility: String = conn.query_row("SELECT visibility FROM application_ports WHERE id = 'p1'", (), |row| row.get(0)).unwrap();
        assert_eq!(visibility, "public");
    }

    #[test]
    fn migration_9_creates_dns_records_with_unique_hostname_and_one_alias_per_application() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrations().to_latest(&mut conn).unwrap();

        conn.execute(
            "INSERT INTO applications (id, name, blueprint_id, blueprint_version, runtime_type, working_directory, created_at, updated_at)
             VALUES ('a1', 'App', 'generic', 1, 'localProcess', '/srv/app', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
            (),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO dns_records (id, application_id, hostname, created_at) VALUES ('d1', 'a1', 'db01.vibe', '2024-01-01T00:00:00Z')",
            (),
        )
        .unwrap();

        // A second alias for the same application must be rejected - one
        // alias per service.
        let duplicate_app = conn.execute(
            "INSERT INTO dns_records (id, application_id, hostname, created_at) VALUES ('d2', 'a1', 'db01-alt.vibe', '2024-01-01T00:00:00Z')",
            (),
        );
        assert!(duplicate_app.is_err());
    }

    #[test]
    fn a_database_host_with_a_provisioned_database_cannot_be_deleted() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrations().to_latest(&mut conn).unwrap();

        conn.execute(
            "INSERT INTO applications (id, name, blueprint_id, blueprint_version, runtime_type, working_directory, created_at, updated_at)
             VALUES ('a1', 'App', 'generic', 1, 'localProcess', '/srv/app', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
            (),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO database_hosts (id, name, engine, host, port, admin_username, created_at, updated_at)
             VALUES ('h1', 'Main DB host', 'mysql', '127.0.0.1', 3306, 'root', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
            (),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO application_databases (id, application_id, database_host_id, database_name, username, connections_from, created_at)
             VALUES ('d1', 'a1', 'h1', 'vibessh_app1', 'vibessh_app1_user', '%', '2024-01-01T00:00:00Z')",
            (),
        )
        .unwrap();

        let err = conn.execute("DELETE FROM database_hosts WHERE id = 'h1'", ()).unwrap_err();
        let rusqlite::Error::SqliteFailure(sqlite_err, message) = &err else {
            panic!("expected a SqliteFailure, got {err:?}");
        };
        assert_eq!(sqlite_err.code, rusqlite::ErrorCode::ConstraintViolation);
        assert!(message.as_deref().unwrap_or_default().contains("FOREIGN KEY"), "unexpected message: {message:?}");
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
