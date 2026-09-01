//! SQLite-backed storage for `firewall_custom_rules` - manually declared
//! firewall rules for a Node, not derived from any Application's own
//! published port. See `models::FirewallCustomRule`'s own doc comment.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use chrono::Utc;
use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{FirewallCustomRule, FirewallCustomRuleInput, PortProtocol};
use crate::storage::migrations::migrations;

pub struct FirewallRuleRepository {
    conn: Mutex<Connection>,
}

impl FirewallRuleRepository {
    pub fn open(db_path: &Path) -> AppResult<Self> {
        // Pragmas (WAL, busy timeout, foreign keys) live in one place -
        // see `storage::open_connection` for why they matter with nine
        // connections open on the same file.
        let mut conn = super::open_connection(db_path, "firewall rule")?;
        migrations()
            .to_latest(&mut conn)
            .map_err(|err| AppError::Storage(format!("failed to migrate the firewall rule database: {err}")))?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().expect("firewall rule repository connection mutex poisoned")
    }

    pub fn list(&self, server_id: Uuid) -> AppResult<Vec<FirewallCustomRule>> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare("SELECT id, server_id, label, protocol, port, source_cidr, created_at FROM firewall_custom_rules WHERE server_id = ?1 ORDER BY created_at")
            .map_err(|err| AppError::Storage(format!("failed to prepare the firewall rule list query: {err}")))?;
        let rows = stmt
            .query_map(params![server_id.to_string()], row_to_rule)
            .map_err(|err| AppError::Storage(format!("failed to list firewall rules: {err}")))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|err| AppError::Storage(format!("failed to read a firewall rule row: {err}")))
    }

    pub fn create(&self, server_id: Uuid, input: &FirewallCustomRuleInput) -> AppResult<FirewallCustomRule> {
        let conn = self.lock();
        let id = Uuid::new_v4();
        let created_at = Utc::now();
        conn.execute(
            "INSERT INTO firewall_custom_rules (id, server_id, label, protocol, port, source_cidr, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id.to_string(),
                server_id.to_string(),
                input.label,
                protocol_to_str(input.protocol),
                input.port,
                input.source_cidr,
                created_at.to_rfc3339(),
            ],
        )
        .map_err(|err| AppError::Storage(format!("failed to create the firewall rule: {err}")))?;
        Ok(FirewallCustomRule {
            id,
            server_id,
            label: input.label.clone(),
            protocol: input.protocol,
            port: input.port,
            source_cidr: input.source_cidr.clone(),
            created_at,
        })
    }

    pub fn delete(&self, id: Uuid) -> AppResult<()> {
        let affected = self
            .lock()
            .execute("DELETE FROM firewall_custom_rules WHERE id = ?1", params![id.to_string()])
            .map_err(|err| AppError::Storage(format!("failed to delete the firewall rule: {err}")))?;
        if affected == 0 {
            return Err(AppError::NotFound(format!("firewall rule {id}")));
        }
        Ok(())
    }
}

fn protocol_to_str(value: PortProtocol) -> &'static str {
    match value {
        PortProtocol::Tcp => "tcp",
        PortProtocol::Udp => "udp",
    }
}

fn protocol_from_str(value: &str) -> PortProtocol {
    match value {
        "udp" => PortProtocol::Udp,
        _ => PortProtocol::Tcp,
    }
}

fn row_to_rule(row: &rusqlite::Row) -> rusqlite::Result<FirewallCustomRule> {
    Ok(FirewallCustomRule {
        id: Uuid::parse_str(&row.get::<_, String>(0)?).expect("stored UUID column is always well-formed"),
        server_id: Uuid::parse_str(&row.get::<_, String>(1)?).expect("stored UUID column is always well-formed"),
        label: row.get(2)?,
        protocol: protocol_from_str(&row.get::<_, String>(3)?),
        port: row.get(4)?,
        source_cidr: row.get(5)?,
        created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(6)?)
            .expect("stored timestamp column is always well-formed")
            .with_timezone(&Utc),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::server_repository::ServerRepository;

    fn temp_repository() -> (FirewallRuleRepository, ServerRepository) {
        let path = std::env::temp_dir().join(format!("vibessh-firewall-rule-test-{}.sqlite3", Uuid::new_v4()));
        (FirewallRuleRepository::open(&path).unwrap(), ServerRepository::open(&path).unwrap())
    }

    fn create_test_server(server_repo: &ServerRepository, name: &str) -> Uuid {
        server_repo
            .create(&crate::models::ServerInput {
                name: name.to_string(),
                host: "203.0.113.10".into(),
                ssh_port: 22,
                username: "root".into(),
                authentication_type: crate::models::AuthenticationType::Password,
                private_key_path: None,
                group_id: None,
                password: Some("x".into()),
                key_passphrase: None,
            })
            .unwrap()
            .id
    }

    #[test]
    fn create_then_list_round_trips_every_field() {
        let (repo, server_repo) = temp_repository();
        let server_id = create_test_server(&server_repo, "Node A");

        let created = repo
            .create(
                server_id,
                &FirewallCustomRuleInput { label: Some("debug port".into()), protocol: PortProtocol::Udp, port: 9999, source_cidr: Some("10.77.0.0/16".into()) },
            )
            .unwrap();
        assert_eq!(created.server_id, server_id);
        assert_eq!(created.label.as_deref(), Some("debug port"));
        assert_eq!(created.protocol, PortProtocol::Udp);
        assert_eq!(created.port, 9999);
        assert_eq!(created.source_cidr.as_deref(), Some("10.77.0.0/16"));

        let listed = repo.list(server_id).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, created.id);
    }

    #[test]
    fn list_only_returns_that_servers_own_rules() {
        let (repo, server_repo) = temp_repository();
        let server_a = create_test_server(&server_repo, "Node A");
        let server_b = create_test_server(&server_repo, "Node B");
        repo.create(server_a, &FirewallCustomRuleInput { label: None, protocol: PortProtocol::Tcp, port: 8443, source_cidr: None }).unwrap();
        repo.create(server_b, &FirewallCustomRuleInput { label: None, protocol: PortProtocol::Tcp, port: 9443, source_cidr: None }).unwrap();

        let for_a = repo.list(server_a).unwrap();
        assert_eq!(for_a.len(), 1);
        assert_eq!(for_a[0].port, 8443);
    }

    #[test]
    fn delete_removes_the_row() {
        let (repo, server_repo) = temp_repository();
        let server_id = create_test_server(&server_repo, "Node A");
        let created = repo.create(server_id, &FirewallCustomRuleInput { label: None, protocol: PortProtocol::Tcp, port: 8443, source_cidr: None }).unwrap();

        repo.delete(created.id).unwrap();
        assert!(repo.list(server_id).unwrap().is_empty());
    }

    #[test]
    fn delete_of_an_unknown_id_is_not_found() {
        let (repo, _server_repo) = temp_repository();
        let err = repo.delete(Uuid::new_v4()).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn deleting_the_server_cascades_its_custom_rules() {
        let (repo, server_repo) = temp_repository();
        let server_id = create_test_server(&server_repo, "Node A");
        repo.create(server_id, &FirewallCustomRuleInput { label: None, protocol: PortProtocol::Tcp, port: 8443, source_cidr: None }).unwrap();

        server_repo.delete(server_id).unwrap();
        assert!(repo.list(server_id).unwrap().is_empty());
    }
}
