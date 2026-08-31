use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

/// One Node's membership in the Vibe Network (Etap M4) - `Serialize` so a
/// Tauri command can hand this straight to the frontend. Only the
/// **public** key is ever stored here; the private key is generated on the
/// Node itself and never leaves it, see `services::network_service`'s own
/// doc comment for the full reasoning.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeNetworkMember {
    pub server_id: Uuid,
    pub wireguard_ip: String,
    pub wireguard_public_key: String,
    pub joined_at: DateTime<Utc>,
}
