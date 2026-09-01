//! `credentials` exists early because Etap E's pairing flow produces a
//! secret the moment it lands - "don't persist secrets in plaintext" isn't
//! optional just because the full server repository wasn't built yet.
//! `server_repository` (Etap 2) is the SQLite-backed store for server
//! records themselves; it never touches secrets, see `credentials`.

pub mod application_backup_repository;
pub mod application_repository;
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
pub mod server_repository;
