use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::dto::{ProcessSummary, ServerMetrics, ServiceSummary};
use crate::error::ProtocolErrorCode;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOutput {
    pub session_id: Uuid,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalClosed {
    pub session_id: Uuid,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub source: String,
    pub line: String,
    pub timestamp: DateTime<Utc>,
    /// Which `DesktopCommand::FollowLogs` this line belongs to.
    ///
    /// Optional so an Agent built before follows existed still
    /// deserialises - it simply never sets it, and the Desktop has nothing
    /// to route such a line to, which is the correct outcome rather than a
    /// parse failure that would take the whole connection down.
    #[serde(default)]
    pub follow_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickActionProgress {
    pub action_id: Uuid,
    pub step_index: u32,
    pub step_count: u32,
    pub status: String,
    pub message: Option<String>,
}

/// Every message the agent can push to the desktop after a successful
/// handshake, over the same WebSocket connection. Internally tagged on
/// `type` using the dot-notation names from the Etap D spec, so the wire
/// format is a flat JSON object like `{"type":"metrics.update","metrics":{...}}`
/// rather than a file full of per-event `if`s on the receiving end — adding
/// a new event means adding a variant here, not touching the connection
/// loop in `agent::transport` or the desktop's client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerEvent {
    #[serde(rename = "metrics.update")]
    MetricsUpdate { metrics: ServerMetrics },

    #[serde(rename = "process.update")]
    ProcessUpdate { processes: Vec<ProcessSummary> },

    #[serde(rename = "service.update")]
    ServiceUpdate { services: Vec<ServiceSummary> },

    #[serde(rename = "terminal.output")]
    TerminalOutput(TerminalOutput),

    #[serde(rename = "terminal.closed")]
    TerminalClosed(TerminalClosed),

    #[serde(rename = "logs.line")]
    LogsLine(LogLine),

    #[serde(rename = "quick_action.progress")]
    QuickActionProgress(QuickActionProgress),

    /// Sent by the agent on a fixed interval; the desktop client resets its
    /// timeout timer on receipt and reconnects if too much time passes
    /// without one. Not itself a state change worth showing the user.
    #[serde(rename = "heartbeat")]
    Heartbeat,

    /// Protocol-level failure after the handshake already succeeded (e.g.
    /// rate limiting). Distinct from a rejected `HandshakeResponse`, which
    /// covers failures before the connection is considered established.
    #[serde(rename = "error")]
    Error {
        code: ProtocolErrorCode,
        message: String,
    },

    /// The Agent's reply to one `DesktopCommand::ApplyDesiredState` (Etap
    /// M3) - carries the same `revision` back so Desktop can tell which
    /// push this acknowledges (and ignore a stale ack that arrives after a
    /// newer push superseded it). `ok: false` with `error` set means the
    /// Agent tried and failed to apply it, not a protocol-level failure -
    /// that still uses `Error` above.
    #[serde(rename = "state.applied")]
    StateApplied {
        revision: u64,
        ok: bool,
        error: Option<String>,
    },
}
