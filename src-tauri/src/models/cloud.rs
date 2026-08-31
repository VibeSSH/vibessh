//! DTOs mirroring the cloud backend's actual JSON shapes (see
//! backend/src/models.rs) - camelCase to match its `#[serde(rename_all =
//! "camelCase")]` on every response, since these are deserialized directly
//! from real HTTP responses, not constructed locally.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudUserProfile {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudAuthResponse {
    pub user: CloudUserProfile,
    pub access_token: String,
    pub access_token_expires_at: i64,
    pub refresh_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudTeam {
    pub id: Uuid,
    pub name: String,
    pub owner_id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudTeamMember {
    pub user_id: Uuid,
    pub email: String,
    pub display_name: String,
    pub joined_at: DateTime<Utc>,
    pub is_owner: bool,
}

/// What the frontend actually needs to know about "am I signed in and to
/// whom" - never includes a token (those stay in Rust, see
/// state::cloud_session).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSessionInfo {
    pub user: CloudUserProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudRole {
    pub id: Uuid,
    pub team_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub is_system: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudRoleWithPermissions {
    #[serde(flatten)]
    pub role: CloudRole,
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudServer {
    pub id: Uuid,
    pub team_id: Uuid,
    pub name: String,
    pub host: String,
    pub ssh_port: i32,
    pub username: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudInvitation {
    pub id: Uuid,
    pub team_id: Uuid,
    pub email: String,
    pub role_id: Option<Uuid>,
    pub status: String,
    pub invited_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudCreatedInvitation {
    #[serde(flatten)]
    pub invitation: CloudInvitation,
    /// The raw, unhashed token - present only in this one response, the
    /// same "shown once" rule the backend itself documents (see
    /// backend/src/models.rs's CreatedInvitation). Delivering it to the
    /// invitee (copy/paste, chat, email) is left to whoever is inviting.
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudAuditEvent {
    pub id: Uuid,
    pub action: String,
    pub target_type: String,
    pub target_id: Option<Uuid>,
    pub result: String,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub actor_id: Option<Uuid>,
    pub actor_email: Option<String>,
    pub actor_display_name: Option<String>,
}
