use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Matches the explicit column list every query in auth.rs selects (never
/// `SELECT *`) - `updated_at` isn't included since nothing reads it yet; a
/// later stage that adds profile editing brings it back then.
#[derive(sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub display_name: String,
    pub created_at: DateTime<Utc>,
}

/// What a client is ever shown about a user - `password_hash` never leaves
/// this service, not even to its own authenticated owner.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub created_at: DateTime<Utc>,
}

impl From<&User> for UserProfile {
    fn from(user: &User) -> Self {
        UserProfile { id: user.id, email: user.email.clone(), display_name: user.display_name.clone(), created_at: user.created_at }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthResponse {
    pub user: UserProfile,
    pub access_token: String,
    pub access_token_expires_at: i64,
    pub refresh_token: String,
}
