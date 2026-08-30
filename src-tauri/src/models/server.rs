use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthenticationType {
    Password,
    PrivateKey,
}

/// How the app talks to a given server. This is the only place the two
/// transports are named side by side — everything downstream (commands,
/// services, frontend) works through `ServerConnection` and never branches
/// on this value itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionMode {
    Ssh,
    Agent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentStatus {
    /// Pairing code generated, agent hasn't connected yet.
    Pairing,
    Connected,
    Disconnected,
    /// Agent connected but reported an incompatible protocol version.
    Incompatible,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    pub id: Uuid,
    pub name: String,
    pub host: String,
    pub ssh_port: u16,
    pub username: String,
    pub authentication_type: AuthenticationType,
    pub connection_mode: ConnectionMode,
    /// Set once a Vibe Agent has been paired for this server.
    pub agent_id: Option<Uuid>,
    pub agent_status: Option<AgentStatus>,
    pub group_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
