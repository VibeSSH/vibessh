use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ProtocolErrorCode;

/// Bumped whenever a breaking change is made to the message shapes in this
/// crate. The agent rejects any client whose version it doesn't recognize
/// instead of guessing at compatibility.
pub const PROTOCOL_VERSION: u32 = 1;

/// First message the desktop sends after the WebSocket upgrade completes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandshakeRequest {
    pub protocol_version: u32,
    pub client_name: String,
    pub client_version: String,
    /// Device credential issued during pairing (Etap E). `None` is accepted
    /// for now since pairing doesn't exist yet — the agent logs a warning
    /// rather than enforcing this field. Etap E turns that warning into a
    /// hard rejection.
    pub auth_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandshakeResponse {
    pub accepted: bool,
    pub agent_id: Uuid,
    pub agent_version: String,
    pub protocol_version: u32,
    pub error: Option<ProtocolErrorCode>,
}
