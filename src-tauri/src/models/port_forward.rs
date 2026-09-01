use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Which of the three classic SSH tunnel shapes a forward is - see
/// `ssh::client`'s own `start_local_forward`/`start_remote_forward`/
/// `start_dynamic_forward` for what each actually does on the wire.
/// Deliberately a separate feature from Vibe Firewall/Exit Ports (design
/// doc's own "Port forwarding jest osobnym feature od Vibe Firewall / Exit
/// Ports" - a tunnel is a temporary, desktop-initiated pipe over an already-
/// authenticated SSH session, not a standing rule attached to a Node).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PortForwardKind {
    /// `ssh -L`: VibeSSH listens on the desktop, each connection is tunneled
    /// to a fixed host:port reachable *from the Node*.
    Local,
    /// `ssh -R`: the Node listens on its own side, each connection is
    /// tunneled back to a fixed host:port reachable *from the desktop*.
    Remote,
    /// `ssh -D`: VibeSSH listens on the desktop as a SOCKS5 proxy - the
    /// target is whatever each SOCKS client asks to CONNECT to, decided
    /// per-connection rather than fixed up front.
    Dynamic,
}

/// A live tunnel, purely in-memory for as long as VibeSSH (and the SSH
/// session it rides on) stays open - same lifetime as an open Terminal
/// (`state::TerminalSessionManager`), not a persisted row. Reconnecting
/// after a restart means recreating the forward from scratch, which is the
/// same thing the user would do with a plain `ssh -L`/`-R`/`-D` flag anyway.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortForwardStatus {
    pub id: Uuid,
    pub server_id: Uuid,
    pub kind: PortForwardKind,
    /// Where VibeSSH is listening - on the desktop for Local/Dynamic, on the
    /// Node for Remote.
    pub bind_address: String,
    /// The port actually bound - the real value even when `0` (any free
    /// port) was requested, so the caller can show/use the port that
    /// actually ended up open.
    pub bind_port: u16,
    /// `None` for Dynamic (SOCKS decides per-connection, there's no one
    /// fixed target to show).
    pub target_host: Option<String>,
    pub target_port: Option<u16>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartPortForwardInput {
    pub server_id: Uuid,
    pub kind: PortForwardKind,
    pub bind_address: String,
    /// `0` asks the OS for any free port - what `PortForwardStatus::bind_port`
    /// then reports back.
    pub bind_port: u16,
    /// Required for Local/Remote, ignored for Dynamic.
    pub target_host: Option<String>,
    pub target_port: Option<u16>,
}
