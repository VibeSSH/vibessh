use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Login credentials for one Docker registry host (`docker.io`, `ghcr.io`,
/// a private registry, ...) - so an Application's image can come from a
/// private repo, not just a public one. One row per registry: the same
/// credential is reused by every Application that pulls from that host,
/// same as how a real `docker login` on a box is per-registry, not
/// per-container. `password` is deliberately absent here - it's never
/// stored in SQLite at all, only in the OS credential store, keyed by this
/// row's own `id` (see `storage::credentials::store_registry_credential_password`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryCredential {
    pub id: Uuid,
    pub registry: String,
    pub username: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetRegistryCredentialInput {
    pub registry: String,
    pub username: String,
    pub password: String,
}
