use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::identity::AgentIdentity;

/// Reported to the desktop during pairing/handshake (Etap D/E). For now it's
/// only logged at startup — nothing consumes it over the network yet.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub id: Uuid,
    pub version: String,
    pub hostname: String,
    pub os: String,
    pub status: ConnectionStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionStatus {
    Disconnected,
    Pairing,
    Connected,
}

impl AgentInfo {
    pub fn collect(identity: &AgentIdentity) -> Self {
        Self {
            id: identity.id,
            version: env!("CARGO_PKG_VERSION").to_string(),
            hostname: hostname::get()
                .ok()
                .and_then(|h| h.into_string().ok())
                .unwrap_or_else(|| "unknown".to_string()),
            os: os_info::get().to_string(),
            status: ConnectionStatus::Disconnected,
        }
    }
}
