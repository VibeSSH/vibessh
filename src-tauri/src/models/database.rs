//! Application Databases domain model - Phase 11 *foundation only*
//! (docs/APPLICATIONS_ARCHITECTURE.md Section 12). Schema + types + storage
//! land here; there is no `services::database_service` yet - the actual
//! `mysql`/`mariadb` CLI provisioning over SSH, the phpMyAdmin Blueprint,
//! and the Databases tab UI are deliberately not built, so nothing in this
//! codebase constructs these types outside tests yet. They exist so that
//! later work has real, tested storage to build on rather than designing it
//! from scratch then.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Wire-compatible engines VibeSSH provisions databases on - both speak the
/// same SQL/CLI surface (`mysql`/`mariadb` client, `CREATE DATABASE`/
/// `CREATE USER`/`GRANT`), so this is a display/informational distinction,
/// not a branch in provisioning logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DatabaseEngine {
    Mysql,
    Mariadb,
}

/// A MySQL/MariaDB engine VibeSSH can provision databases on - almost
/// always the same Server the application itself runs on (Pterodactyl
/// reference screenshot's own `127.0.0.1:3306` pattern: the engine bound to
/// loopback on that host, not exposed publicly).
/// `admin_password` lives in the OS keyring (`storage::credentials`,
/// `SecretKind::DatabaseHostAdmin`), keyed by this row's own `id` - never a
/// column here, same rule every other secret in this codebase follows.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseHost {
    pub id: Uuid,
    /// `None` for a database host that isn't itself a VibeSSH-managed
    /// Server (a shared/external DB host) - still representable, just
    /// without VibeSSH's own SSH connection caching/monitoring for it.
    pub server_id: Option<Uuid>,
    pub name: String,
    pub engine: DatabaseEngine,
    /// As reachable from *that host's own shell*, e.g. `"127.0.0.1"` - not
    /// necessarily reachable from the VibeSSH desktop directly (see
    /// docs/APPLICATIONS_ARCHITECTURE.md Section 12.1 for why provisioning
    /// goes through `SshSession::execute_command`, never a direct MySQL-
    /// protocol connection from the desktop).
    pub host: String,
    pub port: u16,
    /// Has `CREATE DATABASE`/`CREATE USER`/`GRANT` privileges on this
    /// engine - used only for provisioning, never handed to an
    /// application's own generated database user.
    pub admin_username: String,
    /// Set once a built-in phpMyAdmin Blueprint instance is deployed for
    /// this host (Section 12.3) - `None` until then, never a manually-typed
    /// external URL.
    pub phpmyadmin_application_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDatabaseHostInput {
    pub server_id: Option<Uuid>,
    pub name: String,
    pub engine: DatabaseEngine,
    pub host: String,
    pub port: u16,
    pub admin_username: String,
    pub admin_password: String,
}

/// One provisioned database + scoped user for a single Application.
/// `database_name`/`username` are always machine-generated, never
/// user-typed (Section 12.1) - there's no free-text identifier here to
/// validate against SQL-identifier injection in the first place, same
/// "don't accept what you don't have to" reasoning
/// `ssh::systemd::validate_unit_name` already applies elsewhere. The
/// generated user's password lives in the OS keyring (`SecretKind::
/// ApplicationDatabaseUser`), keyed by this row's own `id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationDatabase {
    pub id: Uuid,
    pub application_id: Uuid,
    pub database_host_id: Uuid,
    pub database_name: String,
    pub username: String,
    /// Bind pattern for the generated user's `GRANT`, e.g. `"%"` (any host)
    /// or a specific one.
    pub connections_from: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateApplicationDatabaseInput {
    pub application_id: Uuid,
    pub database_host_id: Uuid,
    pub database_name: String,
    pub username: String,
    pub connections_from: String,
    pub password: String,
}
