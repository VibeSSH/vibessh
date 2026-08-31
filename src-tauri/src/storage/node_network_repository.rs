//! SQLite-backed storage for `node_network_members` (Etap M4) - Desktop's
//! own IPAM ledger for the Vibe Network. Same one-concern-per-repository,
//! shared-physical-file convention every other repository here follows.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::NodeNetworkMember;
use crate::storage::migrations::migrations;

/// `10.77.0.0/16` - a private range with no realistic collision against a
/// managed Server's own LAN (which is virtually always `10.0.0.0/8`'s more
/// common `/24`-sized blocks, `172.16.0.0/12`, or `192.168.0.0/16`) chosen
/// specifically to also stay clear of `10.50.0.0/24`, which the project's
/// own dedicated test server already uses for an unrelated, pre-existing
/// WireGuard mesh (see the `vibessh-test-server` memory) - picking a
/// same-/8-but-different-/16 range keeps the two meshes unambiguously
/// distinct on a host that happens to run both.
const MESH_CIDR_SECOND_OCTET: u8 = 77;

pub struct NodeNetworkRepository {
    conn: Mutex<Connection>,
}

impl NodeNetworkRepository {
    /// The whole Vibe Network address range, as a CIDR string - used by
    /// `services::firewall_service` to scope a "Vibe Network only"
    /// Application port's firewall rule to mesh members only.
    pub const MESH_CIDR: &'static str = "10.77.0.0/16";

    pub fn open(db_path: &Path) -> AppResult<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| AppError::Storage(format!("failed to create the node network database directory: {err}")))?;
        }
        let mut conn = Connection::open(db_path).map_err(|err| AppError::Storage(format!("failed to open the node network database: {err}")))?;
        conn.pragma_update(None, "foreign_keys", true)
            .map_err(|err| AppError::Storage(format!("failed to enable foreign key enforcement: {err}")))?;
        migrations()
            .to_latest(&mut conn)
            .map_err(|err| AppError::Storage(format!("failed to migrate the node network database: {err}")))?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().expect("node network repository connection mutex poisoned")
    }

    pub fn list(&self) -> AppResult<Vec<NodeNetworkMember>> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare("SELECT server_id, wireguard_ip, wireguard_public_key, joined_at FROM node_network_members ORDER BY wireguard_ip")
            .map_err(|err| AppError::Storage(format!("failed to prepare the network member list query: {err}")))?;
        let rows = stmt
            .query_map((), row_to_member)
            .map_err(|err| AppError::Storage(format!("failed to list network members: {err}")))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|err| AppError::Storage(format!("failed to read a network member row: {err}")))
    }

    pub fn get(&self, server_id: Uuid) -> AppResult<Option<NodeNetworkMember>> {
        self.lock()
            .query_row(
                "SELECT server_id, wireguard_ip, wireguard_public_key, joined_at FROM node_network_members WHERE server_id = ?1",
                params![server_id.to_string()],
                row_to_member,
            )
            .optional()
            .map_err(|err| AppError::Storage(format!("failed to read the network member: {err}")))
    }

    /// The next free address in the mesh CIDR - `10.77.0.1`, `10.77.0.2`,
    /// ... sequentially, skipping whatever's already taken. One SQLite
    /// transaction (the whole method runs under the connection's own
    /// mutex), so two joins can't race each other onto the same address -
    /// the architecture doc's own "which operations need global
    /// serialization" answer for IPAM, same as
    /// `NodeStateRepository::bump_desired_revision`'s reasoning.
    fn allocate_ip(&self, conn: &Connection) -> AppResult<String> {
        let mut stmt = conn
            .prepare("SELECT wireguard_ip FROM node_network_members")
            .map_err(|err| AppError::Storage(format!("failed to prepare the IP allocation query: {err}")))?;
        let rows = stmt
            .query_map((), |row| row.get::<_, String>(0))
            .map_err(|err| AppError::Storage(format!("failed to list allocated IPs: {err}")))?;
        let taken: Vec<String> = rows.collect::<Result<Vec<_>, _>>().map_err(|err| AppError::Storage(format!("failed to read an allocated IP: {err}")))?;
        drop(stmt);

        for candidate in 1u32..=65534 {
            let ip = format!("10.{MESH_CIDR_SECOND_OCTET}.{}.{}", (candidate >> 8) & 0xFF, candidate & 0xFF);
            if !taken.contains(&ip) {
                return Ok(ip);
            }
        }
        Err(AppError::InvalidInput("the Vibe Network address range is exhausted".into()))
    }

    /// Allocates the next free IP and records this Node's membership in one
    /// step - `wireguard_public_key` is whatever
    /// `network::wireguard::ensure_keypair` returned (the Node's own,
    /// freshly-generated-or-reused public key; never a private key).
    pub fn join(&self, server_id: Uuid, wireguard_public_key: &str) -> AppResult<NodeNetworkMember> {
        let conn = self.lock();
        let wireguard_ip = self.allocate_ip(&conn)?;
        let joined_at = Utc::now();
        conn.execute(
            "INSERT INTO node_network_members (server_id, wireguard_ip, wireguard_public_key, joined_at) VALUES (?1, ?2, ?3, ?4)",
            params![server_id.to_string(), wireguard_ip, wireguard_public_key, joined_at.to_rfc3339()],
        )
        .map_err(|err| AppError::Storage(format!("failed to record network membership: {err}")))?;
        Ok(NodeNetworkMember { server_id, wireguard_ip, wireguard_public_key: wireguard_public_key.to_string(), joined_at })
    }

    pub fn leave(&self, server_id: Uuid) -> AppResult<()> {
        let affected = self
            .lock()
            .execute("DELETE FROM node_network_members WHERE server_id = ?1", params![server_id.to_string()])
            .map_err(|err| AppError::Storage(format!("failed to remove network membership: {err}")))?;
        if affected == 0 {
            return Err(AppError::NotFound(format!("network membership for server {server_id}")));
        }
        Ok(())
    }
}

fn row_to_member(row: &rusqlite::Row) -> rusqlite::Result<NodeNetworkMember> {
    Ok(NodeNetworkMember {
        server_id: Uuid::parse_str(&row.get::<_, String>(0)?).expect("stored UUID column is always well-formed"),
        wireguard_ip: row.get(1)?,
        wireguard_public_key: row.get(2)?,
        joined_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(3)?)
            .expect("stored timestamp column is always well-formed")
            .with_timezone(&Utc),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AuthenticationType, ServerInput};
    use crate::storage::server_repository::ServerRepository;

    fn temp_repository() -> (NodeNetworkRepository, ServerRepository) {
        let path = std::env::temp_dir().join(format!("vibessh-node-network-test-{}.sqlite3", Uuid::new_v4()));
        (NodeNetworkRepository::open(&path).unwrap(), ServerRepository::open(&path).unwrap())
    }

    fn create_test_server(server_repo: &ServerRepository, name: &str) -> Uuid {
        server_repo
            .create(&ServerInput {
                name: name.into(),
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
    fn join_allocates_sequential_ips_starting_at_dot_one() {
        let (repo, server_repo) = temp_repository();
        let a = create_test_server(&server_repo, "A");
        let b = create_test_server(&server_repo, "B");

        let member_a = repo.join(a, "pubkeyA").unwrap();
        assert_eq!(member_a.wireguard_ip, "10.77.0.1");
        let member_b = repo.join(b, "pubkeyB").unwrap();
        assert_eq!(member_b.wireguard_ip, "10.77.0.2");
    }

    #[test]
    fn join_reuses_a_freed_ip_after_a_leave() {
        let (repo, server_repo) = temp_repository();
        let a = create_test_server(&server_repo, "A");
        let b = create_test_server(&server_repo, "B");

        repo.join(a, "pubkeyA").unwrap();
        repo.leave(a).unwrap();
        let member_b = repo.join(b, "pubkeyB").unwrap();
        assert_eq!(member_b.wireguard_ip, "10.77.0.1", "a freed low address should be reused, not skipped forever");
    }

    #[test]
    fn leave_of_a_non_member_is_not_found() {
        let (repo, server_repo) = temp_repository();
        let a = create_test_server(&server_repo, "A");
        assert!(matches!(repo.leave(a).unwrap_err(), AppError::NotFound(_)));
    }

    #[test]
    fn list_and_get_round_trip() {
        let (repo, server_repo) = temp_repository();
        let a = create_test_server(&server_repo, "A");
        repo.join(a, "pubkeyA").unwrap();

        assert_eq!(repo.list().unwrap().len(), 1);
        let fetched = repo.get(a).unwrap().unwrap();
        assert_eq!(fetched.wireguard_public_key, "pubkeyA");
        assert!(repo.get(Uuid::new_v4()).unwrap().is_none());
    }
}
