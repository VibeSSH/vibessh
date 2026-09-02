//! Invitations - the second way (alongside teams.rs's add_member) someone
//! joins a team, and the one that works when the invitee doesn't already
//! have to be found by email in one step: an invitation is created for an
//! email address, a raw token is handed back exactly once, and whoever
//! later presents that token *while logged in as that exact email* accepts
//! it. No email-sending integration exists in this backend - delivering
//! the token to the invitee (email, Slack, a pasted link) is a client
//! concern; nothing here constructs or hardcodes an invitation URL.
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{Duration, Utc};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::audit;
use crate::auth::AuthUser;
use crate::authorize::{authorize_any, ensure_can_grant};
use crate::errors::{ApiError, ApiResult};
use crate::models::{CreateInvitationRequest, CreatedInvitation, Invitation, Team};
use crate::teams::team_for_member;
use crate::{permissions, tokens, AppState};

const DEFAULT_EXPIRES_IN_DAYS: i64 = 7;
const MAX_EXPIRES_IN_DAYS: i64 = 30;

/// Selects every column `Invitation` needs, computing `status` as
/// `'expired'` at read time for a still-`'pending'` row whose `expires_at`
/// has passed - `status` itself never actually stores that value (see
/// migrations/0005).
const SELECT_INVITATION: &str = "SELECT id, team_id, email, role_id,
    CASE WHEN status = 'pending' AND expires_at < now() THEN 'expired' ELSE status END AS status,
    invited_by, created_at, expires_at
    FROM invitations";

async fn invitation_for_team(db: &PgPool, team_id: Uuid, invitation_id: Uuid) -> ApiResult<Invitation> {
    let invitation: Option<Invitation> =
        sqlx::query_as(&format!("{SELECT_INVITATION} WHERE id = $1 AND team_id = $2"))
            .bind(invitation_id)
            .bind(team_id)
            .fetch_optional(db)
            .await?;
    invitation.ok_or_else(|| ApiError::NotFound("invitation not found".to_string()))
}

pub async fn create_invitation(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(team_id): Path<Uuid>,
    Json(body): Json<CreateInvitationRequest>,
) -> ApiResult<impl IntoResponse> {
    team_for_member(&state.db, team_id, user_id).await?;
    authorize_any(&state.db, team_id, user_id, &[permissions::TEAM_MEMBERS_ADD, permissions::TEAM_INVITATIONS_MANAGE]).await?;

    let email = body.email.trim().to_lowercase();
    if email.split_once('@').is_none_or(|(local, domain)| local.is_empty() || domain.is_empty() || !domain.contains('.')) {
        return Err(ApiError::InvalidInput("email is not a valid address".to_string()));
    }

    let already_member: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM team_members tm JOIN users u ON u.id = tm.user_id WHERE tm.team_id = $1 AND u.email = $2)",
    )
    .bind(team_id)
    .bind(&email)
    .fetch_one(&state.db)
    .await?;
    if already_member {
        return Err(ApiError::Conflict("this email already belongs to a member of the team".to_string()));
    }

    if let Some(role_id) = body.role_id {
        let role_exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM roles WHERE id = $1 AND team_id = $2)")
            .bind(role_id)
            .bind(team_id)
            .fetch_one(&state.db)
            .await?;
        if !role_exists {
            return Err(ApiError::NotFound("role not found".to_string()));
        }
        // An invitation with a role attached grants that role the moment
        // it's accepted - the inviter can't hand out more power that way
        // than they could by assigning the role directly (see roles.rs's
        // assign_role, which enforces the identical rule).
        let role_permissions: Vec<String> =
            sqlx::query_scalar("SELECT permission_key FROM role_permissions WHERE role_id = $1").bind(role_id).fetch_all(&state.db).await?;
        ensure_can_grant(&state.db, team_id, user_id, &role_permissions).await?;
    }

    let expires_in_days = body.expires_in_days.unwrap_or(DEFAULT_EXPIRES_IN_DAYS).clamp(1, MAX_EXPIRES_IN_DAYS);
    let raw_token = tokens::generate();
    let invitation_id = Uuid::new_v4();
    let now = Utc::now();
    let expires_at = now + Duration::days(expires_in_days);

    let mut tx = state.db.begin().await?;
    let insert = sqlx::query(
        "INSERT INTO invitations (id, team_id, email, role_id, token_hash, invited_by, created_at, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(invitation_id)
    .bind(team_id)
    .bind(&email)
    .bind(body.role_id)
    .bind(tokens::hash(&raw_token))
    .bind(user_id)
    .bind(now)
    .bind(expires_at)
    .execute(&mut *tx)
    .await;
    if let Err(sqlx::Error::Database(db_err)) = &insert {
        if db_err.is_unique_violation() {
            return Err(ApiError::Conflict("there's already a pending invitation for this email".to_string()));
        }
    }
    insert?;

    audit::record(&mut tx, team_id, user_id, audit::INVITATION_CREATED, "invitation", Some(invitation_id), json!({ "email": email }))
        .await?;
    tx.commit().await?;

    let invitation = Invitation {
        id: invitation_id,
        team_id,
        email,
        role_id: body.role_id,
        status: "pending".to_string(),
        invited_by: Some(user_id),
        created_at: now,
        expires_at,
    };
    Ok((StatusCode::CREATED, Json(CreatedInvitation { invitation, token: raw_token })))
}

pub async fn list_invitations(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(team_id): Path<Uuid>,
) -> ApiResult<Json<Vec<Invitation>>> {
    team_for_member(&state.db, team_id, user_id).await?;
    authorize_any(&state.db, team_id, user_id, &[permissions::TEAM_MEMBERS_ADD, permissions::TEAM_INVITATIONS_MANAGE]).await?;

    let invitations: Vec<Invitation> =
        sqlx::query_as(&format!("{SELECT_INVITATION} WHERE team_id = $1 ORDER BY created_at DESC")).bind(team_id).fetch_all(&state.db).await?;
    Ok(Json(invitations))
}

pub async fn revoke_invitation(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((team_id, invitation_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    team_for_member(&state.db, team_id, user_id).await?;
    authorize_any(&state.db, team_id, user_id, &[permissions::TEAM_MEMBERS_ADD, permissions::TEAM_INVITATIONS_MANAGE]).await?;

    let invitation = invitation_for_team(&state.db, team_id, invitation_id).await?;
    if invitation.status != "pending" {
        return Err(ApiError::Conflict(format!("this invitation is already {}, not pending", invitation.status)));
    }

    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE invitations SET status = 'revoked', revoked_at = $1 WHERE id = $2")
        .bind(Utc::now())
        .bind(invitation_id)
        .execute(&mut *tx)
        .await?;
    audit::record(&mut tx, team_id, user_id, audit::INVITATION_REVOKED, "invitation", Some(invitation_id), json!({ "email": invitation.email }))
        .await?;
    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Looks up a pending, unexpired invitation by its raw token and confirms
/// it was sent to exactly the authenticated caller's email - the two
/// checks any accept/decline needs before doing anything else.
async fn pending_invitation_for_caller(db: &PgPool, token: &str, caller_email: &str) -> ApiResult<Invitation> {
    let invitation: Option<Invitation> = sqlx::query_as(&format!("{SELECT_INVITATION} WHERE token_hash = $1"))
        .bind(tokens::hash(token))
        .fetch_optional(db)
        .await?;
    let invitation = invitation.ok_or_else(|| ApiError::NotFound("invitation not found".to_string()))?;
    if invitation.status != "pending" {
        return Err(ApiError::NotFound("invitation not found".to_string()));
    }
    if !invitation.email.eq_ignore_ascii_case(caller_email) {
        return Err(ApiError::Forbidden("this invitation was sent to a different email address".to_string()));
    }
    Ok(invitation)
}

pub async fn accept_invitation(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(token): Path<String>,
) -> ApiResult<Json<Team>> {
    let caller_email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = $1").bind(user_id).fetch_one(&state.db).await?;
    let invitation = pending_invitation_for_caller(&state.db, &token, &caller_email).await?;

    let mut tx = state.db.begin().await?;
    let insert = sqlx::query("INSERT INTO team_members (team_id, user_id, joined_at) VALUES ($1, $2, $3)")
        .bind(invitation.team_id)
        .bind(user_id)
        .bind(Utc::now())
        .execute(&mut *tx)
        .await;
    if let Err(sqlx::Error::Database(db_err)) = &insert {
        if db_err.is_unique_violation() {
            return Err(ApiError::Conflict("you're already a member of this team".to_string()));
        }
    }
    insert?;

    if let Some(role_id) = invitation.role_id {
        sqlx::query("INSERT INTO member_roles (team_id, user_id, role_id) VALUES ($1, $2, $3)")
            .bind(invitation.team_id)
            .bind(user_id)
            .bind(role_id)
            .execute(&mut *tx)
            .await?;
    }

    sqlx::query("UPDATE invitations SET status = 'accepted', accepted_at = $1 WHERE id = $2")
        .bind(Utc::now())
        .bind(invitation.id)
        .execute(&mut *tx)
        .await?;

    audit::record(&mut tx, invitation.team_id, user_id, audit::INVITATION_ACCEPTED, "invitation", Some(invitation.id), json!({})).await?;

    let team: Team = sqlx::query_as("SELECT id, name, owner_id, created_at FROM teams WHERE id = $1")
        .bind(invitation.team_id)
        .fetch_one(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(Json(team))
}

pub async fn decline_invitation(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(token): Path<String>,
) -> ApiResult<StatusCode> {
    let caller_email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = $1").bind(user_id).fetch_one(&state.db).await?;
    let invitation = pending_invitation_for_caller(&state.db, &token, &caller_email).await?;

    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE invitations SET status = 'declined' WHERE id = $1").bind(invitation.id).execute(&mut *tx).await?;
    audit::record(&mut tx, invitation.team_id, user_id, audit::INVITATION_DECLINED, "invitation", Some(invitation.id), json!({})).await?;
    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}
