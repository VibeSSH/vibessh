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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateApplicationInput {
    pub name: String,
    pub description: Option<String>,
    pub working_directory: String,
    pub runtime_config: serde_json::Value,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentVariable {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortProtocol {
    Tcp,
    Udp,
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
    /// Blueprint-declared as required - the UI lets it be edited, not
    /// removed (docs/APPLICATIONS_ARCHITECTURE.md's Ports tab spec).
    pub required: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortInput {
    pub name: String,
    pub protocol: PortProtocol,
    pub bind_address: String,
    pub internal_port: u16,
    pub external_port: Option<u16>,
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
}
