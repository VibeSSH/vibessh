use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::PortProtocol;

/// A manually declared firewall rule for one Node - not derived from any
/// Application's own published port, unlike every other rule
/// `services::firewall_service::desired_rules` builds. Exists for the case
/// the design doc calls out by name: a port the user wants open for a
/// reason VibeSSH has no other way to know about (an unrelated service run
/// by hand on the same Node, a one-off debugging port, ...). Once created,
/// `desired_rules` includes it in the same reconcile every other rule goes
/// through - it gets applied on the next sync, and revoked automatically if
/// this row is ever deleted, the same as an Application's own port would be.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallCustomRule {
    pub id: Uuid,
    pub server_id: Uuid,
    /// A short, free-text reason - shown in the UI so a rule someone finds
    /// six months later says what it's for, not just a bare port number.
    pub label: Option<String>,
    pub protocol: PortProtocol,
    pub port: u16,
    /// `None` = reachable from anywhere, mirroring `firewall::FirewallRule::
    /// source_cidr`'s own meaning exactly - this row is turned into that
    /// exact shape at reconcile time, no translation in between.
    pub source_cidr: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallCustomRuleInput {
    pub label: Option<String>,
    pub protocol: PortProtocol,
    pub port: u16,
    pub source_cidr: Option<String>,
}
