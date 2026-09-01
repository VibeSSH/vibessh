//! The Application domain model - see docs/APPLICATIONS_ARCHITECTURE.md for
//! the full design this implements. `Application` itself deliberately
//! doesn't carry environment/ports/runtime-config/metadata inline; those
//! live in their own tables (real per-row CRUD, not a JSON blob - see the
//! migration's own comment in storage::migrations) and are loaded
//! alongside it by the repository as separate, explicit calls.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Derived from `server_id` (`None` ⇒ Local), never its own stored column -
/// there is exactly one source of truth for "where does this run."
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApplicationLocation {
    Local,
    Remote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeType {
    LocalProcess,
    RemoteProcess,
    Systemd,
    Docker,
}

/// Last-known status, always refreshed FROM the runtime before being
/// trusted for anything user-facing - persisted only so the UI has
/// something to show before the first live refresh completes after an app
/// restart, not because this column is itself authoritative (see
/// docs/APPLICATIONS_ARCHITECTURE.md Section 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApplicationStatus {
    Unknown,
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
}

/// What `ApplicationRuntime::health_check` actually probes, beyond "is the
/// process still running" (that check always happens first, regardless of
/// this setting - see each runtime's own `health_check` implementation).
/// `Tcp`/`Http` need `health_check_port_id`; `Http` additionally needs
/// `health_check_http_path`; `MinecraftStatus` needs `health_check_port_id`
/// but speaks the real Minecraft Server List Ping protocol on it rather
/// than a plain connect - see `runtime::health_check`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HealthCheckType {
    Process,
    Tcp,
    Http,
    MinecraftStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Application {
    pub id: Uuid,
    pub server_id: Option<Uuid>,
    pub name: String,
    pub description: Option<String>,
    pub blueprint_id: String,
    pub blueprint_version: i32,
    pub runtime_type: RuntimeType,
    pub working_directory: String,
    pub status: ApplicationStatus,
    pub last_status_check_at: Option<DateTime<Utc>>,
    pub health_check_type: HealthCheckType,
    /// References an `ApplicationPort` - `None` for `HealthCheckType::Process`
    /// (nothing to check beyond the process itself), and also `None` if the
    /// port a check was pointed at has since been removed (the FK is
    /// `ON DELETE SET NULL`, not a hard failure) - either way, a check that
    /// needs a port but doesn't have one resolves to `HealthStatus::Unknown`,
    /// not an error.
    pub health_check_port_id: Option<Uuid>,
    pub health_check_http_path: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Application {
    pub fn location(&self) -> ApplicationLocation {
        if self.server_id.is_some() {
            ApplicationLocation::Remote
        } else {
            ApplicationLocation::Local
        }
    }
}

/// What the frontend submits to create an Application. Environment/ports
/// are supplied as plain maps/vecs here even though they're stored as
/// separate rows - the repository is what fans a `CreateApplicationInput`
/// out into the right tables in one transaction, callers never assemble
/// the SQL shape themselves.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateApplicationInput {
    pub server_id: Option<Uuid>,
    pub name: String,
    pub description: Option<String>,
    pub blueprint_id: String,
    pub blueprint_version: i32,
    pub runtime_type: RuntimeType,
    pub working_directory: String,
    #[serde(default)]
    pub environment: Vec<EnvironmentVariable>,
    #[serde(default)]
    pub ports: Vec<PortInput>,
    /// Runtime-specific shape (JVM flags, Docker image, systemd ExecStart,
    /// ...) - kept opaque here, typed downstream per `runtime_type` by the
    /// runtime implementation that actually reads it (Phase 2+).
    pub runtime_config: serde_json::Value,
    #[serde(default = "serde_json::Value::default")]
    pub metadata: serde_json::Value,
}

/// What the Create Application wizard submits - distinct from
/// `CreateApplicationInput` (which already expects a fully-formed
/// `runtime_config`) because the wizard only collects a blueprint id plus
/// raw field values; turning those into the concrete `runtime_config` a
/// runtime reads is `BlueprintHandler::render_runtime_config`'s job, done
/// server-side in `services::application_service::create_application`
/// (the frontend has no way to run that Rust logic itself).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateApplicationFromBlueprintInput {
    pub server_id: Option<Uuid>,
    pub name: String,
    pub description: Option<String>,
    pub blueprint_id: String,
    pub runtime_type: RuntimeType,
    pub working_directory: String,
    #[serde(default)]
    pub environment: Vec<EnvironmentVariable>,
    /// A `{ fieldKey: value }` object - keys matching the chosen
    /// blueprint's own `BlueprintField::key`s.
    pub blueprint_inputs: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateApplicationInput {
    pub name: String,
    pub description: Option<String>,
    pub working_directory: String,
    pub runtime_config: serde_json::Value,
    pub metadata: serde_json::Value,
}

/// A separate, small input rather than folding this into
/// `UpdateApplicationInput` - health check configuration is its own concern
/// with its own validation (does `port_id`, if any, actually belong to this
/// application?), not part of the general name/description/working-
/// directory edit flow.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetHealthCheckInput {
    pub health_check_type: HealthCheckType,
    pub port_id: Option<Uuid>,
    pub http_path: Option<String>,
}

/// What `services::set_application_resource_limits` accepts - `None` clears
/// that particular limit rather than leaving it untouched, same "this is
/// the whole desired state, not a patch" shape `SetHealthCheckInput` uses.
/// Only meaningful for `RuntimeType::Docker`/`RuntimeType::Systemd` - see
/// that function's own doc comment for why `LocalProcess`/`RemoteProcess`
/// reject this outright instead of silently accepting and ignoring it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetResourceLimitsInput {
    pub memory_limit_mb: Option<u32>,
    pub cpu_limit_cores: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentVariable {
    pub key: String,
    /// For a secret row (`is_secret == true`), this is never the real
    /// value outside `services::application_service`'s own keyring
    /// resolution - see that module's `resolve_environment_secrets`/
    /// `store_secret_environment_values`. A plain read (`ApplicationRepository::get`,
    /// what every Tauri command returns to the frontend) always has this
    /// empty for a secret row; only a runtime about to actually start the
    /// Application ever sees the real value.
    pub value: String,
    #[serde(default)]
    pub is_secret: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortProtocol {
    Tcp,
    Udp,
}

/// The user-facing *intent* behind a port (Etap M4's "Application Network"
/// - the user configures this, never a bind address/CIDR/firewall rule by
/// hand). `bind_address` is still the column `runtime::docker` actually
/// reads to publish the port - `services::application_service` computes it
/// from this on every save (Public/VibeNetwork both bind `0.0.0.0`, since
/// what actually restricts a "Vibe Network only" port to mesh members is
/// the firewall rule `services::firewall_service` derives from this same
/// field, not a different bind address - see that module's own doc
/// comment), except `Custom`, where the user's own typed address wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PortVisibility {
    /// Reachable from the public internet.
    Public,
    /// Reachable only from other Nodes on the Vibe Network.
    VibeNetwork,
    /// Reachable only from this same host.
    Localhost,
    /// A specific bind address the user typed themselves.
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationPort {
    pub id: Uuid,
    pub application_id: Uuid,
    pub name: String,
    pub protocol: PortProtocol,
    pub bind_address: String,
    pub internal_port: u16,
    pub external_port: Option<u16>,
    #[serde(default = "default_visibility")]
    pub visibility: PortVisibility,
    /// Blueprint-declared as required - the UI lets it be edited, not
    /// removed (docs/APPLICATIONS_ARCHITECTURE.md's Ports tab spec).
    pub required: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn default_visibility() -> PortVisibility {
    PortVisibility::Public
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortInput {
    pub name: String,
    pub protocol: PortProtocol,
    /// Only actually used when `visibility` is `Custom` - otherwise
    /// `services::application_service::add_application_port`/
    /// `update_application_port` overwrite it with the address the chosen
    /// visibility implies. Still required on the wire so a `Custom` port
    /// has somewhere to carry the user's address.
    pub bind_address: String,
    pub internal_port: u16,
    pub external_port: Option<u16>,
    #[serde(default = "default_visibility")]
    pub visibility: PortVisibility,
    #[serde(default)]
    pub required: bool,
}

/// Everything a single `applications` row round-trips as, one repository
/// call - the environment/ports/config/metadata that live in their own
/// tables, assembled together for the frontend so it never has to make 4
/// separate calls to render one Application's detail page.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationDetail {
    #[serde(flatten)]
    pub application: Application,
    pub environment: Vec<EnvironmentVariable>,
    pub ports: Vec<ApplicationPort>,
    pub runtime_config: serde_json::Value,
    pub metadata: serde_json::Value,
    /// The other Applications this one is allowed to reach over the Node's
    /// internal Docker networking, from `application_links` (migration 16).
    ///
    /// Ids rather than a richer view type: every consumer already has the
    /// full Application list to hand - the frontend from its own store, the
    /// runtime because it only needs the id to derive a network name - and a
    /// join here would make `get` pay for a lookup that the one caller who
    /// wants names does better itself.
    ///
    /// Unordered pairs, so this is symmetric: if A lists B, B lists A. See
    /// migration 16 for why a connection cannot be one-way.
    pub links: Vec<Uuid>,
}
