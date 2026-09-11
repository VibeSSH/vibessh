use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A Private DNS alias for one Application (Etap M4/M5 - "Private DNS").
/// One alias per Application (`application_id` is UNIQUE at the schema
/// level) - the alias follows the *service*, not the Node it happens to run
/// on right now, which is what lets `services::network_service`-style
/// migration move an Application to a different Node without breaking
/// anything that references it by hostname. See `services::dns_service`'s
/// own doc comment for how the IP behind a hostname is actually resolved.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsRecord {
    pub id: Uuid,
    pub application_id: Uuid,
    pub hostname: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsRecordInput {
    pub application_id: Uuid,
    /// The alias's own name, without the `.vibe` suffix -
    /// `services::dns_service::normalize_alias` adds it (and slugifies)
    /// before this ever reaches storage, so `"db01"` and `"db01.vibe"` both
    /// end up as the exact same stored hostname.
    pub hostname: String,
}

/// One live entry in the rendered `/etc/hosts` fragment
/// (`services::dns_service::render_hosts_fragment`) - a Node's own
/// always-present alias, or one service's alias, each resolved to a real
/// Vibe Network IP at render time.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsView {
    pub hostname: String,
    pub ip: String,
    pub kind: DnsViewKind,
    /// The Node's own id this currently resolves to - for a service alias,
    /// this is whichever Node the Application is on *right now*, so the UI
    /// can show "db01.vibe -> MariaDB / Hetzner-03" per the spec.
    pub server_id: Uuid,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DnsViewKind {
    Node,
    Service { application_id: Uuid },
}
