use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::capabilities::AgentCapabilities;
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
    /// Either a one-time pairing code (first connection) or the durable
    /// credential issued in response to a previous successful pairing
    /// (every connection after that). The agent tells the two apart itself
    /// - callers don't need to know which kind they're holding.
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
    /// Present exactly once: the handshake that consumed a valid pairing
    /// code gets a brand new durable credential back here. The desktop must
    /// persist it (OS keyring, never plaintext) and use it as `auth_token`
    /// for every future connection instead of the pairing code, which is
    /// now spent. Absent on ordinary reconnects.
    pub issued_credential: Option<String>,
    /// Real (freshly detected) capabilities on an accepted handshake;
    /// all-`false` on a rejected one - an unauthenticated attempt doesn't
    /// get free system fingerprinting.
    pub capabilities: AgentCapabilities,
}
