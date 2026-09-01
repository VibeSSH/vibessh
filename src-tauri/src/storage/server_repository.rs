//! SQLite-backed server records (Etap 2). Deliberately holds only the
//! non-secret `Server` shape - passwords and key passphrases never reach
//! this module, see `credentials.rs` for where those actually go.

use std::path::Path;
use std::sync::Mutex;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{AgentStatus, AuthenticationType, ConnectionMode, NodeCapabilities, Server, ServerInput};
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
        // Pragmas (WAL, busy timeout, foreign keys) live in one place -
        // see `storage::open_connection` for why they matter with nine
        // connections open on the same file.
        let mut conn = super::open_connection(db_path, "server")?;
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
            node_capabilities: None,
            created_at: now,
            updated_at: now,
            agent_certificate_fingerprint: None,
        };
        self.insert(&server)?;
        Ok(server)
    }

    /// Persists an agent-paired server, the missing piece behind Etap H's
    /// own "still only show for the current session" note (see README) -
    /// the schema already had `connection_mode`/`agent_id`/`agent_status`
    /// columns from day one, this was just never called for anything but
    /// `ConnectionMode::Ssh`. Upserts by `agent_id` rather than always
    /// inserting, so re-pairing an already-known agent updates its existing
    /// row instead of creating a duplicate. `agent_status` is always stored
    /// as `Disconnected` here regardless of the live connection this call
    /// is racing to record - the value that matters at *this* moment lives
    /// in the frontend's session-only Zustand store; what's written to disk
    /// only gets read back after the process (and every live connection
    /// with it) is long gone, so `Connected` would be a stale lie by the
    /// time anything reads it again.
    ///
    /// `capabilities` is whatever the handshake that triggered this call
    /// detected (Etap M1) - the one moment a fresh reading genuinely exists
    /// for an Agent-mode Node today, since there's no persistent Agent
    /// connection outside the pairing flow yet (see `agent_client`'s own doc
    /// comment). `None` leaves a previously-recorded value alone rather than
    /// erasing it - a re-pairing call that didn't carry a fresh capability
    /// reading shouldn't make a known-good one disappear.
    pub fn upsert_agent(&self, name: &str, host: &str, agent_id: Uuid, capabilities: Option<NodeCapabilities>) -> AppResult<Server> {
        if let Some(existing) = self.get_by_agent_id(agent_id)? {
            let updated = Server {
                name: name.to_string(),
                host: host.to_string(),
                agent_status: Some(AgentStatus::Disconnected),
                node_capabilities: capabilities.or(existing.node_capabilities),
                updated_at: Utc::now(),
                ..existing
            };
            let conn = self.lock();
            conn.execute(
                "UPDATE servers SET name = ?2, host = ?3, agent_status = ?4, node_capabilities_json = ?5, updated_at = ?6 WHERE id = ?1",
                params![
                    updated.id.to_string(),
                    updated.name,
                    updated.host,
                    agent_status_to_str(AgentStatus::Disconnected),
                    updated.node_capabilities.map(|c| serde_json::to_string(&c).expect("NodeCapabilities always serializes")),
                    updated.updated_at.to_rfc3339(),
                ],
            )
            .map_err(|err| AppError::Storage(format!("failed to update agent server: {err}")))?;
            return Ok(updated);
        }

        let now = Utc::now();
        let server = Server {
            id: Uuid::new_v4(),
            name: name.to_string(),
            host: host.to_string(),
            // Not meaningful for agent mode - the WebSocket connection this
            // server uses has its own port, negotiated during pairing, not
            // stored per-row. Zero/empty are the same "not applicable"
            // sentinel these NOT NULL columns already use nowhere else.
            ssh_port: 0,
            username: String::new(),
            authentication_type: AuthenticationType::Password,
            private_key_path: None,
            connection_mode: ConnectionMode::Agent,
            agent_id: Some(agent_id),
            agent_status: Some(AgentStatus::Disconnected),
            group_id: None,
            node_capabilities: capabilities,
            created_at: now,
            updated_at: now,
            // A brand-new agent row: nothing has connected to it yet, so
            // the first successful handshake is what pins it.
            agent_certificate_fingerprint: None,
        };
        self.insert(&server)?;
        Ok(server)
    }

    /// Converts an existing SSH-mode Server row to Agent mode *in place* -
    /// same `id`, same `created_at`/`name`/`host`, so every Application/DNS
    /// alias/Firewall membership already foreign-keyed to this server's own
    /// id keeps working unchanged (`connection_mode` is the only thing
    /// anything downstream branches on - see that field's own doc comment
    /// on `models::server::ConnectionMode`). Used by the Setup Page's own
    /// "also install the Vibe Agent" step: the user already has a working
    /// SSH-mode Node, so pairing an Agent for it should upgrade that same
    /// Node, not create a confusing second entry for the same physical
    /// machine (which is what `upsert_agent` above would do here, since it
    /// matches by `agent_id`, not by an already-known server id). The old
    /// SSH credential in the keyring is deliberately left alone, not
    /// deleted - there's no "downgrade" flow that would need it back, but
    /// silently deleting a credential the user might still want is worse
    /// than leaving one harmless, now-unread row behind.
    pub fn upgrade_to_agent(&self, server_id: Uuid, agent_id: Uuid, capabilities: Option<NodeCapabilities>) -> AppResult<Server> {
        let existing = self.get(server_id)?.ok_or_else(|| AppError::NotFound(format!("server {server_id}")))?;
        let updated = Server {
            connection_mode: ConnectionMode::Agent,
            agent_id: Some(agent_id),
            agent_status: Some(AgentStatus::Disconnected),
            node_capabilities: capabilities.or(existing.node_capabilities),
            updated_at: Utc::now(),
            ..existing
        };
        self.lock()
            .execute(
                "UPDATE servers SET connection_mode = ?2, agent_id = ?3, agent_status = ?4, node_capabilities_json = ?5, updated_at = ?6 WHERE id = ?1",
                params![
                    updated.id.to_string(),
                    connection_mode_to_str(updated.connection_mode),
                    updated.agent_id.map(|id| id.to_string()),
                    agent_status_to_str(AgentStatus::Disconnected),
                    updated.node_capabilities.map(|c| serde_json::to_string(&c).expect("NodeCapabilities always serializes")),
                    updated.updated_at.to_rfc3339(),
                ],
            )
            .map_err(|err| AppError::Storage(format!("failed to upgrade server to agent mode: {err}")))?;
        Ok(updated)
    }

    /// Records the result of a real capability probe (Etap M1) - an SSH-mode
    /// `command -v docker` check today, run on demand (see
    /// `services::probe_node_capabilities`), not on a schedule. Doesn't
    /// touch `updated_at`/anything else on the row - a narrow, frequent
    /// write distinct from `update`'s full-record replace semantics.
    pub fn set_node_capabilities(&self, id: Uuid, capabilities: NodeCapabilities) -> AppResult<()> {
        let json = serde_json::to_string(&capabilities).expect("NodeCapabilities always serializes");
        let affected = self
            .lock()
            .execute("UPDATE servers SET node_capabilities_json = ?2 WHERE id = ?1", params![id.to_string(), json])
            .map_err(|err| AppError::Storage(format!("failed to record node capabilities: {err}")))?;
        if affected == 0 {
            return Err(AppError::NotFound(format!("server {id}")));
        }
        Ok(())
    }

    fn get_by_agent_id(&self, agent_id: Uuid) -> AppResult<Option<Server>> {
        self.lock()
            .query_row(&format!("{SELECT_COLUMNS} FROM servers WHERE agent_id = ?1"), params![agent_id.to_string()], row_to_server)
            .optional()
            .map_err(|err| AppError::Storage(format!("failed to look up server by agent id: {err}")))
    }

    fn insert(&self, server: &Server) -> AppResult<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO servers (
                id, name, host, ssh_port, username, authentication_type,
                private_key_path, connection_mode, agent_id, agent_status,
                group_id, node_capabilities_json, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
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
                server.node_capabilities.map(|c| serde_json::to_string(&c).expect("NodeCapabilities always serializes")),
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
            // Same reasoning, and load-bearing: silently dropping the pin
            // on an unrelated edit would make the next connection trust
            // whatever certificate it saw.
            agent_certificate_fingerprint: existing.agent_certificate_fingerprint.clone(),
            // Same reasoning - a probe result, not something a manual edit
            // form has any opinion on.
            node_capabilities: existing.node_capabilities,
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
        let affected = conn.execute("DELETE FROM servers WHERE id = ?1", params![id.to_string()]).map_err(|err| {
            if is_foreign_key_violation(&err) {
                // `applications.server_id` is ON DELETE RESTRICT (see
                // storage::migrations) specifically so this can't happen
                // silently - the caller (server_service::delete_server)
                // turns this into a message naming which applications are
                // still attached, not just "storage error".
                AppError::InvalidInput(format!("server {id} still has applications attached - remove or move them first"))
            } else {
                AppError::Storage(format!("failed to delete server: {err}"))
            }
        })?;
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

    /// Records the TLS certificate fingerprint an Agent-mode Node presented,
    /// the first time it presents one. See migration 15 and
    /// `agent_client::AgentClientConfig::known_fingerprint` for why this is
    /// trust-on-first-use rather than chain validation.
    ///
    /// **Only writes when the column is still NULL.** Once a Node is pinned,
    /// a *different* fingerprint is rejected at connection time - before the
    /// bearer credential is sent - and must never be quietly written over
    /// the old one here, or an interceptor would only need to be present
    /// once to make itself permanently trusted. Re-pinning is a deliberate
    /// operator action, see `clear_agent_certificate_fingerprint`.
    pub fn pin_agent_certificate_fingerprint(&self, id: Uuid, fingerprint: &str) -> AppResult<()> {
        self.lock()
            .execute(
                "UPDATE servers SET agent_certificate_fingerprint = ?1 WHERE id = ?2 AND agent_certificate_fingerprint IS NULL",
                params![fingerprint, id.to_string()],
            )
            .map_err(|err| AppError::Storage(format!("failed to record the agent certificate fingerprint: {err}")))?;
        Ok(())
    }

    /// Forgets the pin, so the next connection trusts whatever it sees -
    /// the operator's escape hatch for an agent that was legitimately
    /// reinstalled.
    pub fn clear_agent_certificate_fingerprint(&self, id: Uuid) -> AppResult<()> {
        self.lock()
            .execute("UPDATE servers SET agent_certificate_fingerprint = NULL WHERE id = ?1", params![id.to_string()])
            .map_err(|err| AppError::Storage(format!("failed to clear the agent certificate fingerprint: {err}")))?;
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("server database mutex poisoned")
    }
}

const SELECT_COLUMNS: &str = "SELECT id, name, host, ssh_port, username, authentication_type, \
     private_key_path, connection_mode, agent_id, agent_status, group_id, node_capabilities_json, created_at, updated_at,      agent_certificate_fingerprint";

fn row_to_server(row: &rusqlite::Row) -> rusqlite::Result<Server> {
    Ok(Server {
        id: parse_uuid(row.get::<_, String>(0)?, 0)?,
        name: row.get(1)?,
        host: row.get(2)?,
        ssh_port: row.get(3)?,
        username: row.get(4)?,
        authentication_type: auth_type_from_str(&row.get::<_, String>(5)?),
        private_key_path: row.get(6)?,
        connection_mode: connection_mode_from_str(&row.get::<_, String>(7)?),
        agent_id: row.get::<_, Option<String>>(8)?.map(|value| parse_uuid(value, 8)).transpose()?,
        agent_status: row.get::<_, Option<String>>(9)?.as_deref().map(agent_status_from_str),
        group_id: row.get::<_, Option<String>>(10)?.map(|value| parse_uuid(value, 10)).transpose()?,
        // Deliberately *not* an error: unlike an id or a timestamp, this
        // column is a cache of what a Node reported about itself over the
        // network, and it is `Option` already. A blob this build cannot
        // parse - most likely because a newer build wrote a field this one
        // does not know - degrades to "capabilities unknown", which the UI
        // already handles, and the next capability probe overwrites it.
        // Failing the whole row here would make a routine version skew
        // stop the Node from loading at all.
        node_capabilities: row.get::<_, Option<String>>(11)?.and_then(|json| match serde_json::from_str(&json) {
            Ok(capabilities) => Some(capabilities),
            Err(err) => {
                log::warn!("ignoring an unreadable node_capabilities_json value: {err}");
                None
            }
        }),
        created_at: parse_timestamp(row.get::<_, String>(12)?, 12)?,
        updated_at: parse_timestamp(row.get::<_, String>(13)?, 13)?,
        agent_certificate_fingerprint: row.get(14)?,
    })
}

/// Distinguishes a `FOREIGN KEY constraint failed` (a real, expected
/// "something still references this row" case - see `delete` above) from
/// every other kind of SQLite failure, which should stay a generic storage
/// error rather than being misreported as this specific, actionable one.
///
/// Deliberately *not* `extended_code == SQLITE_CONSTRAINT_FOREIGNKEY` -
/// proven wrong by a real test failure (see
/// storage::migrations::tests::a_server_with_an_application_attached_cannot_be_deleted):
/// SQLite's *immediate* RESTRICT check (as opposed to a deferred one,
/// raised at COMMIT) reports as `SQLITE_CONSTRAINT_TRIGGER` instead, even
/// though the message correctly says "FOREIGN KEY constraint failed".
/// Checking the primary code + message text is what's actually reliable
/// across both cases.
fn is_foreign_key_violation(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(sqlite_err, Some(message))
            if sqlite_err.code == rusqlite::ErrorCode::ConstraintViolation && message.contains("FOREIGN KEY")
    )
}

/// Both of these used to `expect()`, on the reasoning that VibeSSH is the
/// only writer of this database so the values are always well-formed.
///
/// That reasoning does not survive contact with a release build. The release
/// profile sets `panic = "abort"`, so a single malformed value - a
/// half-written row after a power cut, a hand-edited database, a file
/// restored from a partial backup - does not surface as a handled error or
/// even a catchable panic: **the whole desktop app aborts, with no dialog,
/// no log line and no way to get back in**, on every launch, because these
/// run on the read path every list view uses.
///
/// Returning `rusqlite::Error` instead means one bad row fails one query
/// with a message naming the column, which `AppError::Storage` then
/// surfaces normally.
fn parse_uuid(value: String, column: usize) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(&value).map_err(|err| rusqlite::Error::FromSqlConversionFailure(column, rusqlite::types::Type::Text, Box::new(err)))
}

fn parse_timestamp(value: String, column: usize) -> rusqlite::Result<chrono::DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(&value)
        .map(|parsed| parsed.with_timezone(&Utc))
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(column, rusqlite::types::Type::Text, Box::new(err)))
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
    fn upsert_agent_creates_a_row_that_survives_a_reopen() {
        let path = std::env::temp_dir().join(format!("vibessh-agent-test-{}.sqlite3", Uuid::new_v4()));
        let agent_id = Uuid::new_v4();
        {
            let repo = ServerRepository::open(&path).unwrap();
            let server = repo.upsert_agent("Prod Agent", "203.0.113.20", agent_id, Some(NodeCapabilities { docker: true, ..Default::default() })).unwrap();
            assert_eq!(server.connection_mode, ConnectionMode::Agent);
            assert_eq!(server.agent_id, Some(agent_id));
            assert_eq!(server.agent_status, Some(AgentStatus::Disconnected));
            assert_eq!(server.node_capabilities, Some(NodeCapabilities { docker: true, ..Default::default() }));
        }

        // Reopening simulates the next app launch - the row must still be
        // there, which is exactly the gap this method closes (previously
        // agent-paired servers only ever lived in the frontend's in-memory
        // store, gone the moment the app closed).
        let repo = ServerRepository::open(&path).unwrap();
        let servers = repo.list().unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "Prod Agent");
        assert_eq!(servers[0].agent_id, Some(agent_id));
        assert_eq!(servers[0].node_capabilities, Some(NodeCapabilities { docker: true, ..Default::default() }), "a capability reading must survive a reopen, same as every other field");
    }

    #[test]
    fn upsert_agent_on_an_already_known_agent_id_updates_instead_of_duplicating() {
        let repo = temp_repository();
        let agent_id = Uuid::new_v4();
        let first = repo.upsert_agent("Old Name", "203.0.113.20", agent_id, Some(NodeCapabilities { docker: true, ..Default::default() })).unwrap();

        let second = repo.upsert_agent("New Name", "203.0.113.21", agent_id, None).unwrap();
        assert_eq!(second.id, first.id, "re-pairing the same agent should update its row, not create a new one");
        assert_eq!(second.name, "New Name");
        assert_eq!(second.host, "203.0.113.21");
        assert_eq!(second.node_capabilities, Some(NodeCapabilities { docker: true, ..Default::default() }), "a missing fresh reading must not erase a previously known-good one");

        assert_eq!(repo.list().unwrap().len(), 1);
    }

    #[test]
    fn upgrade_to_agent_converts_an_ssh_mode_row_in_place() {
        let repo = temp_repository();
        let ssh_server = repo.create(&test_input("My Node")).unwrap();
        assert_eq!(ssh_server.connection_mode, ConnectionMode::Ssh);
        let agent_id = Uuid::new_v4();

        let upgraded = repo.upgrade_to_agent(ssh_server.id, agent_id, Some(NodeCapabilities { docker: true, ..Default::default() })).unwrap();

        assert_eq!(upgraded.id, ssh_server.id, "must be the exact same row, not a new one");
        assert_eq!(upgraded.name, "My Node", "name is left untouched by an upgrade");
        assert_eq!(upgraded.host, "203.0.113.10", "host is left untouched by an upgrade");
        assert_eq!(upgraded.created_at, ssh_server.created_at);
        assert_eq!(upgraded.connection_mode, ConnectionMode::Agent);
        assert_eq!(upgraded.agent_id, Some(agent_id));
        assert_eq!(upgraded.agent_status, Some(AgentStatus::Disconnected));
        assert_eq!(upgraded.node_capabilities, Some(NodeCapabilities { docker: true, ..Default::default() }));

        // Only ever the one row for this Node - no duplicate second entry.
        assert_eq!(repo.list().unwrap().len(), 1);
    }

    #[test]
    fn upgrade_to_agent_of_an_unknown_server_is_not_found() {
        let repo = temp_repository();
        let err = repo.upgrade_to_agent(Uuid::new_v4(), Uuid::new_v4(), None).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn set_node_capabilities_persists_and_survives_a_reopen() {
        let path = std::env::temp_dir().join(format!("vibessh-capabilities-test-{}.sqlite3", Uuid::new_v4()));
        let id = {
            let repo = ServerRepository::open(&path).unwrap();
            let created = repo.create(&test_input("Docker Box")).unwrap();
            assert_eq!(created.node_capabilities, None, "a freshly created server is unprobed, not known-incapable");

            repo.set_node_capabilities(created.id, NodeCapabilities { docker: true, ..Default::default() }).unwrap();
            let loaded = repo.get(created.id).unwrap().unwrap();
            assert_eq!(loaded.node_capabilities, Some(NodeCapabilities { docker: true, ..Default::default() }));
            created.id
        };

        let repo = ServerRepository::open(&path).unwrap();
        assert_eq!(repo.get(id).unwrap().unwrap().node_capabilities, Some(NodeCapabilities { docker: true, ..Default::default() }));
    }

    /// A real crash, reproduced and pinned down: a `node_capabilities_json`
    /// blob written back when `NodeCapabilities` only had `docker` (every
    /// row probed before `wireguard`/`ufw` existed) used to fail
    /// `row_to_server`'s `serde_json::from_str(..).expect(...)` on every
    /// single launch, aborting the whole app rather than just that one
    /// server's capabilities reading as unprobed. See `NodeCapabilities`'s
    /// own doc comment for the `#[serde(default)]` fix this pins down.
    #[test]
    fn a_node_capabilities_blob_persisted_before_wireguard_and_ufw_existed_still_loads() {
        let path = std::env::temp_dir().join(format!("vibessh-capabilities-legacy-test-{}.sqlite3", Uuid::new_v4()));
        let id = {
            let repo = ServerRepository::open(&path).unwrap();
            repo.create(&test_input("Legacy Box")).unwrap().id
        };

        {
            let conn = Connection::open(&path).unwrap();
            conn.execute("UPDATE servers SET node_capabilities_json = '{\"docker\":true}' WHERE id = ?1", params![id.to_string()]).unwrap();
        }

        let repo = ServerRepository::open(&path).unwrap();
        let loaded = repo.get(id).unwrap().unwrap();
        assert_eq!(loaded.node_capabilities, Some(NodeCapabilities { docker: true, wireguard: false, ufw: false }));
    }

    /// The general form of the crash above. `#[serde(default)]` fixed the
    /// one blob shape that had actually been written; it does nothing for a
    /// blob this build simply cannot parse - a newer build's field, a
    /// truncated write, a restored partial backup. Since the release
    /// profile sets `panic = "abort"`, `expect()` there took the whole app
    /// down on every launch. Unknown capabilities must degrade to "not
    /// probed yet", which the UI already renders, and leave the Node
    /// loadable.
    #[test]
    fn an_unreadable_node_capabilities_blob_degrades_instead_of_failing_the_row() {
        let path = std::env::temp_dir().join(format!("vibessh-capabilities-corrupt-test-{}.sqlite3", Uuid::new_v4()));
        let id = {
            let repo = ServerRepository::open(&path).unwrap();
            repo.create(&test_input("Corrupted Box")).unwrap().id
        };
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute("UPDATE servers SET node_capabilities_json = 'not json at all' WHERE id = ?1", params![id.to_string()]).unwrap();
        }

        let repo = ServerRepository::open(&path).unwrap();
        let loaded = repo.get(id).unwrap().unwrap();
        assert_eq!(loaded.node_capabilities, None);
        assert_eq!(loaded.name, "Corrupted Box");
        // The rest of the list still loads too - one bad blob must not take
        // every other Node with it.
        assert_eq!(repo.list().unwrap().len(), 1);
    }

    /// An id or a timestamp is not optional, so a malformed one cannot
    /// degrade - but it must fail *this query* with a real error rather
    /// than aborting the process.
    #[test]
    fn a_corrupted_id_or_timestamp_is_an_error_not_an_abort() {
        for (column, bogus) in [("id", "not-a-uuid"), ("created_at", "not-a-timestamp")] {
            let path = std::env::temp_dir().join(format!("vibessh-corrupt-{column}-test-{}.sqlite3", Uuid::new_v4()));
            let id = {
                let repo = ServerRepository::open(&path).unwrap();
                repo.create(&test_input("Box")).unwrap().id
            };
            {
                let conn = Connection::open(&path).unwrap();
                conn.execute(&format!("UPDATE servers SET {column} = ?1 WHERE id = ?2"), params![bogus, id.to_string()]).unwrap();
            }

            let repo = ServerRepository::open(&path).unwrap();
            let result = repo.list();
            assert!(matches!(result, Err(AppError::Storage(_))), "{column} should surface as a storage error, got {result:?}");
        }
    }

    #[test]
    fn set_node_capabilities_on_a_missing_server_is_not_found() {
        let repo = temp_repository();
        let err = repo.set_node_capabilities(Uuid::new_v4(), NodeCapabilities { docker: true, ..Default::default() }).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
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
    /// Trust on first use, and *only* first use. Once a Node is pinned, a
    /// second attempt with a different fingerprint must not overwrite it -
    /// otherwise an interceptor present for a single connection would make
    /// itself permanently trusted. The rejection happens in `agent_client`,
    /// before the credential is sent; this pins that the storage layer
    /// cannot undo it either.
    #[test]
    fn an_agent_certificate_pin_is_recorded_once_and_never_silently_replaced() {
        let repo = temp_repository();
        let created = repo.create(&test_input("Agent Node")).unwrap();
        assert_eq!(repo.get(created.id).unwrap().unwrap().agent_certificate_fingerprint, None);

        repo.pin_agent_certificate_fingerprint(created.id, "aaaa").unwrap();
        assert_eq!(repo.get(created.id).unwrap().unwrap().agent_certificate_fingerprint, Some("aaaa".to_string()));

        // A different fingerprint arriving later leaves the pin alone.
        repo.pin_agent_certificate_fingerprint(created.id, "bbbb").unwrap();
        assert_eq!(repo.get(created.id).unwrap().unwrap().agent_certificate_fingerprint, Some("aaaa".to_string()));

        // Clearing is the deliberate operator action that allows re-pinning.
        repo.clear_agent_certificate_fingerprint(created.id).unwrap();
        assert_eq!(repo.get(created.id).unwrap().unwrap().agent_certificate_fingerprint, None);
        repo.pin_agent_certificate_fingerprint(created.id, "bbbb").unwrap();
        assert_eq!(repo.get(created.id).unwrap().unwrap().agent_certificate_fingerprint, Some("bbbb".to_string()));
    }

    /// Editing a Node through the SSH form must not drop its agent pin -
    /// silently forgetting it would make the next connection trust whatever
    /// certificate it saw.
    #[test]
    fn updating_a_server_preserves_its_agent_certificate_pin() {
        let repo = temp_repository();
        let created = repo.create(&test_input("Agent Node")).unwrap();
        repo.pin_agent_certificate_fingerprint(created.id, "aaaa").unwrap();

        repo.update(created.id, &test_input("Renamed Node")).unwrap();
        let loaded = repo.get(created.id).unwrap().unwrap();
        assert_eq!(loaded.name, "Renamed Node");
        assert_eq!(loaded.agent_certificate_fingerprint, Some("aaaa".to_string()));
    }
}
