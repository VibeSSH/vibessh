//! SQLite-backed server records (Etap 2). Deliberately holds only the
//! non-secret `Server` shape - passwords and key passphrases never reach
//! this module, see `credentials.rs` for where those actually go.

use std::path::Path;
use std::sync::Mutex;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{AgentStatus, AuthenticationType, ConnectionMode, Server, ServerInput};
use crate::storage::migrations::migrations;

pub struct ServerRepository {
    conn: Mutex<Connection>,
}

/// A database written by a pre-migration-framework build already has the
/// `servers`/`ssh_known_hosts` tables (created via the old bare
/// `CREATE TABLE IF NOT EXISTS` calls) but SQLite's `user_version` is still
/// its default of 0 - indistinguishable, as far as `user_version` alone is
/// concerned, from a brand new empty database. Running migration 1's
/// `CREATE TABLE` against it would fail with "table already exists" instead
/// of recognizing the schema is already there. If `servers` exists and
/// `user_version` is still 0, this stamps it to 1 directly (no SQL
/// re-executed - the schema already matches migration 1 verbatim) so
/// `to_latest` sees a fully-migrated database and does nothing. Runs once
/// per real upgrade, the very first time an existing user's database is
/// opened by a build that has the migration framework.
fn bootstrap_legacy_schema(conn: &Connection) -> AppResult<()> {
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", (), |row| row.get(0))
        .map_err(|err| AppError::Storage(format!("failed to read the database's user_version: {err}")))?;
    if user_version != 0 {
        return Ok(());
    }

    let already_has_servers_table: bool = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'servers'",
            (),
            |row| row.get::<_, i64>(0),
        )
        .map_err(|err| AppError::Storage(format!("failed to inspect the database's existing tables: {err}")))?
        > 0;
    if !already_has_servers_table {
        return Ok(());
    }

    conn.execute_batch("PRAGMA user_version = 1")
        .map_err(|err| AppError::Storage(format!("failed to stamp the legacy database's schema version: {err}")))?;
    Ok(())
}

impl ServerRepository {
    pub fn open(db_path: &Path) -> AppResult<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                AppError::Storage(format!("failed to create the server database directory: {err}"))
            })?;
        }
        let mut conn = Connection::open(db_path)
            .map_err(|err| AppError::Storage(format!("failed to open the server database: {err}")))?;
        bootstrap_legacy_schema(&conn)?;

        // `servers` (server metadata) and `ssh_known_hosts` (Etap 3's TOFU
        // host key store, kept as its own table rather than a column on
        // `servers` since it's connection-security bookkeeping that's fine
        // to be absent or to change independently of the server's own
        // fields) are both defined in storage::migrations - see there for
        // how future schema changes get added.
        migrations()
            .to_latest(&mut conn)
            .map_err(|err| AppError::Storage(format!("failed to migrate the server database: {err}")))?;

        Ok(Self { conn: Mutex::new(conn) })
    }

    /// `None` means no connection has ever succeeded for this server - the
    /// next `ssh::connect` call trusts whatever host key it sees and this
    /// becomes the baseline every later connection is checked against.
    pub fn get_known_host_fingerprint(&self, server_id: Uuid) -> AppResult<Option<String>> {
        self.lock()
            .query_row(
                "SELECT fingerprint FROM ssh_known_hosts WHERE server_id = ?1",
                params![server_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|err| AppError::Storage(format!("failed to read the known host key: {err}")))
    }

    pub fn set_known_host_fingerprint(&self, server_id: Uuid, fingerprint: &str) -> AppResult<()> {
        self.lock()
            .execute(
                "INSERT INTO ssh_known_hosts (server_id, fingerprint) VALUES (?1, ?2)
                 ON CONFLICT(server_id) DO UPDATE SET fingerprint = excluded.fingerprint",
                params![server_id.to_string(), fingerprint],
            )
            .map_err(|err| AppError::Storage(format!("failed to record the known host key: {err}")))?;
        Ok(())
    }

    pub fn create(&self, input: &ServerInput) -> AppResult<Server> {
        let now = Utc::now();
        let server = Server {
            id: Uuid::new_v4(),
            name: input.name.clone(),
            host: input.host.clone(),
            ssh_port: input.ssh_port,
            username: input.username.clone(),
            authentication_type: input.authentication_type,
            private_key_path: input.private_key_path.clone(),
            connection_mode: ConnectionMode::Ssh,
            agent_id: None,
            agent_status: None,
            group_id: input.group_id,
            created_at: now,
            updated_at: now,
        };
        self.insert(&server)?;
        Ok(server)
    }

    fn insert(&self, server: &Server) -> AppResult<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO servers (
                id, name, host, ssh_port, username, authentication_type,
                private_key_path, connection_mode, agent_id, agent_status,
                group_id, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                server.id.to_string(),
                server.name,
                server.host,
                server.ssh_port,
                server.username,
                auth_type_to_str(server.authentication_type),
                server.private_key_path,
                connection_mode_to_str(server.connection_mode),
                server.agent_id.map(|id| id.to_string()),
                server.agent_status.map(agent_status_to_str),
                server.group_id.map(|id| id.to_string()),
                server.created_at.to_rfc3339(),
                server.updated_at.to_rfc3339(),
            ],
        )
        .map_err(|err| AppError::Storage(format!("failed to insert server: {err}")))?;
        Ok(())
    }

    /// Full replace, not a partial patch - simplest correct semantics for
    /// a form that always submits the whole record. `id`/`created_at` are
    /// preserved from the existing row; everything else in `input` wins.
    pub fn update(&self, id: Uuid, input: &ServerInput) -> AppResult<Server> {
        let existing = self.get(id)?.ok_or_else(|| AppError::NotFound(format!("server {id}")))?;
        let updated = Server {
            id,
            name: input.name.clone(),
            host: input.host.clone(),
            ssh_port: input.ssh_port,
            username: input.username.clone(),
            authentication_type: input.authentication_type,
            private_key_path: input.private_key_path.clone(),
            group_id: input.group_id,
            created_at: existing.created_at,
            updated_at: Utc::now(),
            // Agent pairing state isn't something an SSH-form edit should
            // ever touch - carried over as-is.
            connection_mode: existing.connection_mode,
            agent_id: existing.agent_id,
            agent_status: existing.agent_status,
        };

        let conn = self.lock();
        conn.execute(
            "UPDATE servers SET
                name = ?2, host = ?3, ssh_port = ?4, username = ?5,
                authentication_type = ?6, private_key_path = ?7,
                group_id = ?8, updated_at = ?9
             WHERE id = ?1",
            params![
                updated.id.to_string(),
                updated.name,
                updated.host,
                updated.ssh_port,
                updated.username,
                auth_type_to_str(updated.authentication_type),
                updated.private_key_path,
                updated.group_id.map(|id| id.to_string()),
                updated.updated_at.to_rfc3339(),
            ],
        )
        .map_err(|err| AppError::Storage(format!("failed to update server: {err}")))?;

        // A previously recorded host key belongs to the old host:port - if
        // either changed, it would otherwise be compared against a
        // different machine's key on the next connection and get rejected
        // as a "mismatch" that isn't actually one.
        if existing.host != updated.host || existing.ssh_port != updated.ssh_port {
            conn.execute("DELETE FROM ssh_known_hosts WHERE server_id = ?1", params![id.to_string()])
                .map_err(|err| AppError::Storage(format!("failed to clear the stale known host key: {err}")))?;
        }
        drop(conn);

        Ok(updated)
    }

    pub fn delete(&self, id: Uuid) -> AppResult<()> {
        let conn = self.lock();
        let affected = conn
            .execute("DELETE FROM servers WHERE id = ?1", params![id.to_string()])
            .map_err(|err| AppError::Storage(format!("failed to delete server: {err}")))?;
        if affected == 0 {
            return Err(AppError::NotFound(format!("server {id}")));
        }
        conn.execute("DELETE FROM ssh_known_hosts WHERE server_id = ?1", params![id.to_string()])
            .map_err(|err| AppError::Storage(format!("failed to clear the known host key: {err}")))?;
        Ok(())
    }

    pub fn get(&self, id: Uuid) -> AppResult<Option<Server>> {
        let conn = self.lock();
        conn.query_row(&format!("{SELECT_COLUMNS} FROM servers WHERE id = ?1"), params![id.to_string()], row_to_server)
            .optional()
            .map_err(|err| AppError::Storage(format!("failed to load server: {err}")))
    }

    pub fn list(&self) -> AppResult<Vec<Server>> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare(&format!("{SELECT_COLUMNS} FROM servers ORDER BY name COLLATE NOCASE"))
            .map_err(|err| AppError::Storage(format!("failed to prepare server list query: {err}")))?;
        let rows = stmt
            .query_map((), row_to_server)
            .map_err(|err| AppError::Storage(format!("failed to list servers: {err}")))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|err| AppError::Storage(format!("failed to read a server row: {err}")))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("server database mutex poisoned")
    }
}

const SELECT_COLUMNS: &str = "SELECT id, name, host, ssh_port, username, authentication_type, \
     private_key_path, connection_mode, agent_id, agent_status, group_id, created_at, updated_at";

fn row_to_server(row: &rusqlite::Row) -> rusqlite::Result<Server> {
    Ok(Server {
        id: parse_uuid(row.get::<_, String>(0)?),
        name: row.get(1)?,
        host: row.get(2)?,
        ssh_port: row.get(3)?,
        username: row.get(4)?,
        authentication_type: auth_type_from_str(&row.get::<_, String>(5)?),
        private_key_path: row.get(6)?,
        connection_mode: connection_mode_from_str(&row.get::<_, String>(7)?),
        agent_id: row.get::<_, Option<String>>(8)?.map(parse_uuid),
        agent_status: row.get::<_, Option<String>>(9)?.as_deref().map(agent_status_from_str),
        group_id: row.get::<_, Option<String>>(10)?.map(parse_uuid),
        created_at: parse_timestamp(row.get::<_, String>(11)?),
        updated_at: parse_timestamp(row.get::<_, String>(12)?),
    })
}

fn parse_uuid(value: String) -> Uuid {
    Uuid::parse_str(&value).expect("stored UUID column is always well-formed")
}

fn parse_timestamp(value: String) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(&value)
        .expect("stored timestamp column is always well-formed")
        .with_timezone(&Utc)
}

fn auth_type_to_str(value: AuthenticationType) -> &'static str {
    match value {
        AuthenticationType::Password => "password",
        AuthenticationType::PrivateKey => "private_key",
    }
}

fn auth_type_from_str(value: &str) -> AuthenticationType {
    match value {
        "private_key" => AuthenticationType::PrivateKey,
        _ => AuthenticationType::Password,
    }
}

fn connection_mode_to_str(value: ConnectionMode) -> &'static str {
    match value {
        ConnectionMode::Ssh => "ssh",
        ConnectionMode::Agent => "agent",
    }
}

fn connection_mode_from_str(value: &str) -> ConnectionMode {
    match value {
        "agent" => ConnectionMode::Agent,
        _ => ConnectionMode::Ssh,
    }
}

fn agent_status_to_str(value: AgentStatus) -> &'static str {
    match value {
        AgentStatus::Pairing => "pairing",
        AgentStatus::Connected => "connected",
        AgentStatus::Disconnected => "disconnected",
        AgentStatus::Incompatible => "incompatible",
    }
}

fn agent_status_from_str(value: &str) -> AgentStatus {
    match value {
        "connected" => AgentStatus::Connected,
        "disconnected" => AgentStatus::Disconnected,
        "incompatible" => AgentStatus::Incompatible,
        _ => AgentStatus::Pairing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_input(name: &str) -> ServerInput {
        ServerInput {
            name: name.to_string(),
            host: "203.0.113.10".to_string(),
            ssh_port: 22,
            username: "root".to_string(),
            authentication_type: AuthenticationType::Password,
            private_key_path: None,
            group_id: None,
            password: Some("hunter2".to_string()),
            key_passphrase: None,
        }
    }

    fn temp_repository() -> ServerRepository {
        let path = std::env::temp_dir().join(format!("vibessh-test-{}.sqlite3", Uuid::new_v4()));
        ServerRepository::open(&path).unwrap()
    }

    #[test]
    fn create_then_get_round_trips_every_field() {
        let repo = temp_repository();
        let created = repo.create(&test_input("Production")).unwrap();

        let loaded = repo.get(created.id).unwrap().expect("server should exist");
        assert_eq!(loaded.id, created.id);
        assert_eq!(loaded.name, "Production");
        assert_eq!(loaded.host, "203.0.113.10");
        assert_eq!(loaded.ssh_port, 22);
        assert_eq!(loaded.username, "root");
        assert_eq!(loaded.authentication_type, AuthenticationType::Password);
        assert_eq!(loaded.connection_mode, ConnectionMode::Ssh);
        assert!(loaded.agent_id.is_none());
    }

    #[test]
    fn list_is_sorted_by_name_case_insensitively() {
        let repo = temp_repository();
        repo.create(&test_input("zebra")).unwrap();
        repo.create(&test_input("Apple")).unwrap();
        repo.create(&test_input("banana")).unwrap();

        let names: Vec<String> = repo.list().unwrap().into_iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["Apple", "banana", "zebra"]);
    }

    #[test]
    fn update_preserves_id_and_created_at_but_changes_the_rest() {
        let repo = temp_repository();
        let created = repo.create(&test_input("Original")).unwrap();

        let mut update = test_input("Renamed");
        update.host = "198.51.100.20".to_string();
        let updated = repo.update(created.id, &update).unwrap();

        assert_eq!(updated.id, created.id);
        assert_eq!(updated.created_at, created.created_at);
        assert_eq!(updated.name, "Renamed");
        assert_eq!(updated.host, "198.51.100.20");
        assert!(updated.updated_at >= created.updated_at);
    }

    #[test]
    fn update_of_a_missing_server_is_not_found() {
        let repo = temp_repository();
        let err = repo.update(Uuid::new_v4(), &test_input("Ghost")).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn opening_a_pre_migration_framework_database_preserves_its_data() {
        // Reproduces a real upgrade: a database written by the old
        // CREATE TABLE IF NOT EXISTS code (user_version left at the SQLite
        // default of 0) must open cleanly under the new migration-based
        // code, with its existing row intact - not fail with "table already
        // exists", and not silently drop the data.
        let path = std::env::temp_dir().join(format!("vibessh-legacy-test-{}.sqlite3", Uuid::new_v4()));
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute(
                "CREATE TABLE IF NOT EXISTS servers (
                    id TEXT PRIMARY KEY, name TEXT NOT NULL, host TEXT NOT NULL,
                    ssh_port INTEGER NOT NULL, username TEXT NOT NULL,
                    authentication_type TEXT NOT NULL, private_key_path TEXT,
                    connection_mode TEXT NOT NULL, agent_id TEXT, agent_status TEXT,
                    group_id TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
                )",
                (),
            )
            .unwrap();
            conn.execute(
                "CREATE TABLE IF NOT EXISTS ssh_known_hosts (server_id TEXT PRIMARY KEY, fingerprint TEXT NOT NULL)",
                (),
            )
            .unwrap();
            conn.execute(
                "INSERT INTO servers (id, name, host, ssh_port, username, authentication_type, connection_mode, created_at, updated_at)
                 VALUES ('11111111-1111-1111-1111-111111111111', 'Legacy', 'legacy.example.com', 22, 'root', 'password', 'ssh', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
                (),
            )
            .unwrap();
        }

        let repo = ServerRepository::open(&path).expect("opening a legacy database should not fail");
        let servers = repo.list().unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "Legacy");

        // Re-opening it again (every launch after the upgrade) must also
        // stay a no-op migration, not re-trigger the bootstrap path.
        drop(repo);
        let repo_again = ServerRepository::open(&path).expect("re-opening the now-migrated database should not fail");
        assert_eq!(repo_again.list().unwrap().len(), 1);
    }

    #[test]
    fn delete_removes_the_row_and_is_not_found_afterward() {
        let repo = temp_repository();
        let created = repo.create(&test_input("Temporary")).unwrap();

        repo.delete(created.id).unwrap();
        assert!(repo.get(created.id).unwrap().is_none());

        let err = repo.delete(created.id).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }
}
