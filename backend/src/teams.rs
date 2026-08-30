//! Teams + Team Members. No role/permission awareness yet - every member
//! sees the same team, only the owner can add/remove members or delete the
//! team (see migrations/0002 for why there's no per-member role column
//! yet). Every read here also doubles as an access check: `team_for_member`
//! returns 404 for both "team doesn't exist" and "you're not a member" -
//! deliberately indistinguishable, so a non-member can't tell a team apart
//! from one that was never created at all.
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::errors::{ApiError, ApiResult};
use crate::models::{AddMemberRequest, CreateTeamRequest, Team, TeamMember};
use crate::{permissions, AppState};

pub const OWNER_ROLE_NAME: &str = "Owner";

const MAX_TEAM_NAME_LEN: usize = 100;

pub(crate) async fn team_for_member(db: &PgPool, team_id: Uuid, user_id: Uuid) -> ApiResult<Team> {
    let team: Option<Team> = sqlx::query_as(
        "SELECT t.id, t.name, t.owner_id, t.created_at FROM teams t
         JOIN team_members tm ON tm.team_id = t.id
         WHERE t.id = $1 AND tm.user_id = $2",
    )
    .bind(team_id)
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    team.ok_or_else(|| ApiError::NotFound("team not found".to_string()))
}

pub(crate) fn require_owner(team: &Team, user_id: Uuid) -> ApiResult<()> {
    if team.owner_id != user_id {
        return Err(ApiError::Forbidden("only the team owner can do this".to_string()));
    }
    Ok(())
}

pub async fn create_team(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Json(body): Json<CreateTeamRequest>,
) -> ApiResult<impl IntoResponse> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::InvalidInput("team name cannot be empty".to_string()));
    }
    if name.chars().count() > MAX_TEAM_NAME_LEN {
        return Err(ApiError::InvalidInput(format!("team name must be at most {MAX_TEAM_NAME_LEN} characters")));
    }

    let team_id = Uuid::new_v4();
    let now = Utc::now();

    // Creating a team, making its creator both owner and member, seeding
    // the team's "Owner" role (every permission in the catalog), and
    // assigning that role to the creator is all one atomic step - there's
    // never a moment where a team exists with no members, or a member with
    // no way to manage what they just created.
    let mut tx = state.db.begin().await?;
    sqlx::query("INSERT INTO teams (id, name, owner_id, created_at, updated_at) VALUES ($1, $2, $3, $4, $4)")
        .bind(team_id)
        .bind(name)
        .bind(user_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO team_members (team_id, user_id, joined_at) VALUES ($1, $2, $3)")
        .bind(team_id)
        .bind(user_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;

    let owner_role_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO roles (id, team_id, name, description, is_system, created_at, updated_at)
         VALUES ($1, $2, $3, $4, TRUE, $5, $5)",
    )
    .bind(owner_role_id)
    .bind(team_id)
    .bind(OWNER_ROLE_NAME)
    .bind("Full control over the team - every permission, cannot be edited or deleted.")
    .bind(now)
    .execute(&mut *tx)
    .await?;
    for permission in permissions::ALL_PERMISSIONS {
        sqlx::query("INSERT INTO role_permissions (role_id, permission_key) VALUES ($1, $2)")
            .bind(owner_role_id)
            .bind(permission)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("INSERT INTO member_roles (team_id, user_id, role_id) VALUES ($1, $2, $3)")
        .bind(team_id)
        .bind(user_id)
        .bind(owner_role_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    let team = Team { id: team_id, name: name.to_string(), owner_id: user_id, created_at: now };
    Ok((StatusCode::CREATED, Json(team)))
}

pub async fn list_teams(State(state): State<AppState>, AuthUser(user_id): AuthUser) -> ApiResult<Json<Vec<Team>>> {
    let teams: Vec<Team> = sqlx::query_as(
        "SELECT t.id, t.name, t.owner_id, t.created_at FROM teams t
         JOIN team_members tm ON tm.team_id = t.id
         WHERE tm.user_id = $1
         ORDER BY t.created_at",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(teams))
}

pub async fn get_team(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(team_id): Path<Uuid>,
) -> ApiResult<Json<Team>> {
    Ok(Json(team_for_member(&state.db, team_id, user_id).await?))
}

pub async fn list_members(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(team_id): Path<Uuid>,
) -> ApiResult<Json<Vec<TeamMember>>> {
    team_for_member(&state.db, team_id, user_id).await?;

    let members: Vec<TeamMember> = sqlx::query_as(
        "SELECT tm.user_id, u.email, u.display_name, tm.joined_at, (t.owner_id = tm.user_id) AS is_owner
         FROM team_members tm
         JOIN users u ON u.id = tm.user_id
         JOIN teams t ON t.id = tm.team_id
         WHERE tm.team_id = $1
         ORDER BY tm.joined_at",
    )
    .bind(team_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(members))
}

/// Adds an already-registered user directly - not the same thing as the
/// (future, separate) Invitations stage. This exists so Team Members is a
/// real, end-to-end testable feature now rather than only ever having one
/// member until Invitations ships; once Invitations exists it's the
/// pending/accept/decline flow for inviting someone who may not have an
/// account yet, while this stays the "I already know this person has an
/// account, just add them" shortcut - both are legitimate to keep.
pub async fn add_member(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(team_id): Path<Uuid>,
    Json(body): Json<AddMemberRequest>,
) -> ApiResult<impl IntoResponse> {
    let team = team_for_member(&state.db, team_id, user_id).await?;
    require_owner(&team, user_id)?;

    let email = body.email.trim().to_lowercase();
    let target_user_id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(&email)
        .fetch_optional(&state.db)
        .await?;
    let target_user_id = target_user_id.ok_or_else(|| ApiError::NotFound("no account with that email".to_string()))?;

    let insert = sqlx::query("INSERT INTO team_members (team_id, user_id, joined_at) VALUES ($1, $2, $3)")
        .bind(team_id)
        .bind(target_user_id)
        .bind(Utc::now())
        .execute(&state.db)
        .await;

    if let Err(sqlx::Error::Database(db_err)) = &insert {
        if db_err.is_unique_violation() {
            return Err(ApiError::Conflict("this user is already a member of the team".to_string()));
        }
    }
    insert?;

    Ok(StatusCode::CREATED)
}

pub async fn remove_member(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((team_id, target_user_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    let team = team_for_member(&state.db, team_id, user_id).await?;
    require_owner(&team, user_id)?;

    // The only owner can't be removed as a member - there would be no one
    // left who could manage the team at all. A real ownership-transfer flow
    // (making someone else owner first) is future work; for now this is a
    // hard rule, not a soft warning.
    if target_user_id == team.owner_id {
        return Err(ApiError::Conflict("the team owner can't be removed - transfer ownership first".to_string()));
    }

    let affected = sqlx::query("DELETE FROM team_members WHERE team_id = $1 AND user_id = $2")
        .bind(team_id)
        .bind(target_user_id)
        .execute(&state.db)
        .await?
        .rows_affected();
    if affected == 0 {
        return Err(ApiError::NotFound("that user isn't a member of this team".to_string()));
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_team(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(team_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let team = team_for_member(&state.db, team_id, user_id).await?;
    require_owner(&team, user_id)?;

    sqlx::query("DELETE FROM teams WHERE id = $1").bind(team_id).execute(&state.db).await?;
    Ok(StatusCode::NO_CONTENT)
}
