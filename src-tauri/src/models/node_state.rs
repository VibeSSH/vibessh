use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

/// One Node's desired-vs-applied state (Etap M3) - `Serialize` so a Tauri
/// command can hand this straight to the frontend for the "Out of sync
/// [Reconcile]" UI. `in_sync` is a plain integer comparison
/// (`desired_revision == applied_revision`) computed once here rather than
/// left for the frontend to get wrong - see
/// `services::node_state_service::sync_status`'s own doc comment for why a
/// Node that's never had a desired state at all (both `0`) is trivially "in
/// sync," not "unknown."
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeSyncStatus {
    pub server_id: Uuid,
    pub desired_revision: u64,
    pub applied_revision: u64,
    pub in_sync: bool,
}

/// The result of one real reconcile attempt (`services::node_state_service::reconcile_node`)
/// - distinguishes a genuine failure (`Failed`, the Agent tried and
/// couldn't) from a Node that simply wasn't reachable to ask (`OfflinePending`)
/// from a confirmed success (`Applied`). Never collapsed into a single
/// boolean - the spec's own explicit requirement that a reconcile never
/// reports SUCCESS for a Node that was unreachable.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum ReconcileOutcome {
    Applied { revision: u64 },
    /// The Node has no live session (never connected this app run, or the
    /// connection dropped) - the desired revision was still recorded, so
    /// the next successful (re)connect can reconcile against it.
    OfflinePending { desired_revision: u64 },
    /// A live session accepted the command, but the Agent's own ack said
    /// `ok: false`, or no ack arrived within the wait window.
    Failed { revision: u64, error: Option<String> },
}

#[derive(Debug, Clone)]
pub struct NodeAppliedRecord {
    pub server_id: Uuid,
    pub applied_revision: u64,
    pub applied_at: Option<DateTime<Utc>>,
    pub last_reconcile_status: Option<String>,
    pub last_error: Option<String>,
}
