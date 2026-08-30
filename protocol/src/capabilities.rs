use serde::{Deserialize, Serialize};

/// What this host can actually do, reported on every accepted handshake.
/// The desktop must never assume every Linux box has Docker/systemd - it
/// hides or marks unavailable features based on this instead. A `true`
/// here means the *host* supports it, not necessarily that the agent has a
/// feature built to act on it yet (see docs/agent-privileges.md for which
/// of these still need a privilege-escalation story before they're usable).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    pub docker: bool,
    pub systemd: bool,
    pub minecraft: bool,
    pub file_access: bool,
    pub terminal: bool,
}
