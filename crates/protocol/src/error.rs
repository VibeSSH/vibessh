use serde::{Deserialize, Serialize};

/// Machine-readable reasons a handshake or message was rejected. Kept
/// separate from any human-facing text so the desktop can branch on it
/// (e.g. show a "reconnect won't help" message for `VersionMismatch`) without
/// string-matching an error message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolErrorCode {
    VersionMismatch,
    Unauthorized,
    RateLimited,
    InvalidMessage,
    Internal,
}
