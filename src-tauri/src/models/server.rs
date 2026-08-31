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

/// What's actually known about a Node's capabilities - `docker` is the only
/// field Etap M1 detects (an SSH-mode probe running `command -v docker`, or
/// the Agent's own handshake-time detection); more fields (e.g. `firewall`
/// for Etap M2) are expected to join this later without a new migration,
/// since it's stored as one JSON blob (`servers.node_capabilities_json`),
/// not a column per capability. `Default` (all `false`) is never persisted
/// on its own - `Server::node_capabilities` stays `None` until a real probe
/// has actually run, see that field's own doc comment for why the
/// distinction matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NodeCapabilities {
    pub docker: bool,
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
    /// Only meaningful for `AuthenticationType::PrivateKey`. A *path* to a
    /// key file, not the key's contents - the file's own permissions are
    /// what protect it, same as any other SSH client. Storing key content
    /// directly would mean writing potentially several KB into the OS
    /// credential store, which on Windows has a hard ~2.5KB size ceiling
    /// for generic credentials - too small for a 4096-bit RSA key. Not a
    /// secret itself, so it lives in this row, not the keyring.
    pub private_key_path: Option<String>,
    pub connection_mode: ConnectionMode,
    /// Set once a Vibe Agent has been paired for this server.
    pub agent_id: Option<Uuid>,
    pub agent_status: Option<AgentStatus>,
    pub group_id: Option<Uuid>,
    /// `None` means "never probed" - genuinely unknown, not "no
    /// capabilities" - so a freshly added Node (or one from before Etap M1)
    /// doesn't read back as Docker-incapable until something has actually
    /// checked. See `services::probe_node_capabilities`/`upsert_agent`'s own
    /// doc comments for the two ways this gets set.
    pub node_capabilities: Option<NodeCapabilities>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// What the frontend submits to create or replace a server. Secrets
/// (`password`, `key_passphrase`) are pulled out and sent to the OS keyring
/// by the service layer - they never reach `server_repository`, which only
/// ever sees the non-secret `Server` shape above.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInput {
    pub name: String,
    pub host: String,
    pub ssh_port: u16,
    pub username: String,
    pub authentication_type: AuthenticationType,
    pub private_key_path: Option<String>,
    pub group_id: Option<Uuid>,
    /// Required when `authentication_type` is `Password`.
    pub password: Option<String>,
    /// Optional even when `authentication_type` is `PrivateKey` - not
    /// every key file has a passphrase.
    pub key_passphrase: Option<String>,
}
