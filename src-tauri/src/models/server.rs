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
/// `#[serde(default)]` is load-bearing, not decoration: a `NodeCapabilities`
/// blob persisted before `wireguard`/`ufw` existed (just `{"docker":true}`)
/// must still deserialize - falling back to `Default`'s `false` for a field
/// that row predates, exactly what this module's own doc comment above
/// promises "join this later without a new migration" actually means.
/// Without it, `row_to_server`'s `.expect(...)` turns a merely-stale field
/// into a hard panic on every launch for any user who probed a Node before
/// this field existed - confirmed the hard way: this exact case crashed the
/// app on startup for every previously-probed server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct NodeCapabilities {
    pub docker: bool,
    /// The `wg` CLI is on this Node's `PATH` - what `network::wireguard::
    /// install_if_missing` itself checks before deciding whether to install,
    /// surfaced here too so a Setup flow can show it without a redundant
    /// probe of its own.
    pub wireguard: bool,
    /// `ufw` is on this Node's `PATH` - the same detection
    /// `firewall::ufw::UfwProvider::detect` already does for
    /// `firewall::provider_for`, reused rather than re-implemented.
    pub ufw: bool,
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
    /// The SHA-256 fingerprint of the TLS certificate this Agent-mode Node
    /// presented the first time VibeSSH connected to it, and the value
    /// every later connection is checked against.
    ///
    /// `None` means "not pinned yet" - the next successful handshake
    /// records whatever it sees. Trust on first use, the same model
    /// `ssh_known_hosts` uses for SSH host keys, and for the same reason:
    /// the agent's certificate is self-signed and generated on the Node, so
    /// there is no chain to validate against. Not a secret - a certificate
    /// fingerprint is public by construction - so it lives in this row
    /// rather than the keyring.
    pub agent_certificate_fingerprint: Option<String>,
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
