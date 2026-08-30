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
