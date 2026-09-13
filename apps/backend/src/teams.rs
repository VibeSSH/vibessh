//! Teams + Team Members. Every read here also doubles as an access check:
//! `team_for_member` returns 404 for both "team doesn't exist" and "you're
//! not a member" - deliberately indistinguishable, so a non-member can't
//! tell a team apart from one that was never created at all. Write/
//! management endpoints additionally call `authorize()` (see authorize.rs)
//! for the specific permission that action needs - real RBAC now, not an
//! `owner_id` comparison; see that module's own doc comment for why the
//! two are behaviorally identical today (only the seeded Owner role exists
//! so far) but aren't the same check.
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use serde_json::json;

use crate::audit;
use crate::auth::AuthUser;
use crate::authorize::{authorize, ensure_can_grant};
use crate::errors::{ApiError, ApiResult, Detail};
use crate::models::{AddMemberRequest, CreateTeamRequest, ProvisionMemberRequest, ProvisionedMember, Team, TeamMember};
use crate::revocations;
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
    team.ok_or_else(|| ApiError::NotFound(Detail::new("team_not_found", "team not found")))
}

pub async fn create_team(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Json(body): Json<CreateTeamRequest>,
) -> ApiResult<impl IntoResponse> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::InvalidInput(Detail::new("team_name_empty", "team name cannot be empty")));
    }
    if name.chars().count() > MAX_TEAM_NAME_LEN {
        return Err(ApiError::InvalidInput(Detail::new("team_name_too_long", format!("team name must be at most {MAX_TEAM_NAME_LEN} characters")).with("max", MAX_TEAM_NAME_LEN)));
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

    audit::record(&mut tx, team_id, user_id, audit::TEAM_CREATED, "team", Some(team_id), json!({ "name": name })).await?;

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
    team_for_member(&state.db, team_id, user_id).await?;
    authorize(&state.db, team_id, user_id, permissions::TEAM_MEMBERS_ADD).await?;

    let email = body.email.trim().to_lowercase();
    let target_user_id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(&email)
        .fetch_optional(&state.db)
        .await?;
    let target_user_id = target_user_id.ok_or_else(|| ApiError::NotFound(Detail::new("no_account_with_email", "no account with that email")))?;

    let mut tx = state.db.begin().await?;
    let insert = sqlx::query("INSERT INTO team_members (team_id, user_id, joined_at) VALUES ($1, $2, $3)")
        .bind(team_id)
        .bind(target_user_id)
        .bind(Utc::now())
        .execute(&mut *tx)
        .await;

    if let Err(sqlx::Error::Database(db_err)) = &insert {
        if db_err.is_unique_violation() {
            return Err(ApiError::Conflict(Detail::new("already_a_team_member", "this user is already a member of the team")));
        }
    }
    insert?;

    // Somebody who was removed and is now back is no longer owed a removal.
    // Left standing, the next sync of a Node would give them their account
    // and take it away again in the same pass.
    let cancelled = revocations::cancel_for_returning_member(&mut tx, team_id, target_user_id).await?;

    audit::record(
        &mut tx,
        team_id,
        user_id,
        audit::MEMBER_ADDED,
        "user",
        Some(target_user_id),
        json!({ "email": email, "revocations_cancelled": cancelled }),
    )
    .await?;
    tx.commit().await?;

    Ok(StatusCode::CREATED)
}

/// Creates an account for somebody and puts them in the team, in one step.
///
/// The flow this replaces asked the person to register on their own first,
/// which meant an invitation could not reach anybody who had not already
/// found the app - the common case when a team lead wants to bring somebody
/// in. Here the lead supplies an email, the server makes the account, and
/// the lead passes on a password that the account is then forced to
/// replace.
///
/// The password is generated here rather than chosen by the caller: a
/// password one person picks for another is reliably the weakest either of
/// them uses, and this one exists only to survive being passed along. It is
/// returned exactly once. Nothing stores it in the clear and no endpoint
/// can produce it again - if it is lost, provision the account again or use
/// a reset.
///
/// Everything happens in one transaction. A half-provisioned account - a
/// user with no team, or a member with no role - would be worse than a
/// clean failure, because nobody would know which half had happened.
pub async fn provision_member(
    State(state): State<AppState>,
    AuthUser(actor_id): AuthUser,
    Path(team_id): Path<Uuid>,
    Json(body): Json<ProvisionMemberRequest>,
) -> ApiResult<impl IntoResponse> {
    team_for_member(&state.db, team_id, actor_id).await?;
    authorize(&state.db, team_id, actor_id, permissions::TEAM_MEMBERS_ADD).await?;

    let email = crate::auth::normalize_email(&body.email);
    crate::auth::validate_email(&email)?;

    // The local part of the address, until they set their own. Better than
    // an empty name in a member list, and it is theirs to change.
    let display_name = match body.display_name.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) => crate::auth::validate_display_name(name)?,
        None => crate::auth::validate_display_name(email.split('@').next().unwrap_or("member"))?,
    };

    // Checked before the role is validated so the common mistake - the
    // person already has an account - is reported as itself.
    let existing: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE email = $1").bind(&email).fetch_optional(&state.db).await?;
    if existing.is_some() {
        return Err(ApiError::Conflict(Detail::new("email_taken_add_as_member", "an account with this email already exists - add them as a member instead")));
    }

    // A role is granted here, so the same rule applies as anywhere else:
    // nobody hands out more than they hold.
    if let Some(role_id) = body.role_id {
        let role_permissions: Vec<String> =
            sqlx::query_scalar("SELECT rp.permission_key FROM role_permissions rp JOIN roles r ON r.id = rp.role_id WHERE r.id = $1 AND r.team_id = $2")
                .bind(role_id)
                .bind(team_id)
                .fetch_all(&state.db)
                .await?;
        ensure_can_grant(&state.db, team_id, actor_id, &role_permissions).await?;
    }

    let temporary_password = crate::password::generate_password();
    let password_hash = crate::password::hash_password(&temporary_password).map_err(ApiError::Internal)?;
    let new_user_id = Uuid::new_v4();
    let now = Utc::now();

    let mut tx = state.db.begin().await?;

    let insert = sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name, created_at, updated_at, must_change_password)
         VALUES ($1, $2, $3, $4, $5, $5, TRUE)",
    )
    .bind(new_user_id)
    .bind(&email)
    .bind(&password_hash)
    .bind(&display_name)
    .bind(now)
    .execute(&mut *tx)
    .await;
    if let Err(sqlx::Error::Database(db_err)) = &insert {
        if db_err.is_unique_violation() {
            // Somebody registered between the check above and this insert.
            return Err(ApiError::Conflict(Detail::new("email_taken_add_as_member", "an account with this email already exists - add them as a member instead")));
        }
    }
    insert?;

    sqlx::query("INSERT INTO team_members (team_id, user_id, joined_at) VALUES ($1, $2, $3)")
        .bind(team_id)
        .bind(new_user_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;

    let mut role_assigned = false;
    if let Some(role_id) = body.role_id {
        let belongs: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM roles WHERE id = $1 AND team_id = $2").bind(role_id).bind(team_id).fetch_optional(&mut *tx).await?;
        if belongs.is_none() {
            return Err(ApiError::NotFound(Detail::new("role_not_found", "role not found")));
        }
        sqlx::query("INSERT INTO member_roles (team_id, user_id, role_id) VALUES ($1, $2, $3)")
            .bind(team_id)
            .bind(new_user_id)
            .bind(role_id)
            .execute(&mut *tx)
            .await?;
        role_assigned = true;
    }

    // The password is never in the audit record - the point of writing one
    // is that somebody can see an account was created for this address, not
    // to leave the credential in a log that outlives it.
    audit::record(
        &mut tx,
        team_id,
        actor_id,
        audit::MEMBER_ADDED,
        "user",
        Some(new_user_id),
        json!({ "email": email, "provisioned": true, "role_assigned": role_assigned }),
    )
    .await?;
    tx.commit().await?;

    let user = crate::models::UserProfile {
        id: new_user_id,
        email,
        display_name,
        created_at: now,
        must_change_password: true,
    };
    Ok((StatusCode::CREATED, Json(ProvisionedMember { user, temporary_password, role_assigned })))
}

pub async fn remove_member(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((team_id, target_user_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    let team = team_for_member(&state.db, team_id, user_id).await?;
    authorize(&state.db, team_id, user_id, permissions::TEAM_MEMBERS_REMOVE).await?;

    // The only owner can't be removed as a member - there would be no one
    // left who could manage the team at all. A real ownership-transfer flow
    // (making someone else owner first) is future work; for now this is a
    // hard rule, not a soft warning.
    if target_user_id == team.owner_id {
        return Err(ApiError::Conflict(Detail::new("owner_cannot_be_removed", "the team owner can't be removed - transfer ownership first")));
    }

    let mut tx = state.db.begin().await?;
    let affected = sqlx::query("DELETE FROM team_members WHERE team_id = $1 AND user_id = $2")
        .bind(team_id)
        .bind(target_user_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if affected == 0 {
        return Err(ApiError::NotFound(Detail::new("not_a_team_member", "that user isn't a member of this team")));
    }

    // Removing the membership row does not touch the account this person has
    // on each Node the team shares - only an install that can reach those
    // machines can do that. So what is owed is written down in the same
    // transaction, and an install completes it later. Reporting the removal
    // as finished here, while their key is still in an authorized_keys file,
    // is the one thing this screen must never do.
    let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(target_user_id)
        .fetch_one(&mut *tx)
        .await?;
    let nodes = revocations::record_for_removed_member(&mut tx, team_id, target_user_id, &email, user_id).await?;

    audit::record(
        &mut tx,
        team_id,
        user_id,
        audit::MEMBER_REMOVED,
        "user",
        Some(target_user_id),
        json!({ "email": email, "revocations_pending": nodes }),
    )
    .await?;
    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_team(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(team_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    team_for_member(&state.db, team_id, user_id).await?;
    authorize(&state.db, team_id, user_id, permissions::TEAM_DELETE).await?;

    // Deliberately not recording a TEAM_DELETED audit event: audit_events.
    // team_id is ON DELETE CASCADE (see migrations/0004), so any event
    // referencing this team_id - including one logging the deletion itself -
    // would be wiped out by this same DELETE. Preserving a team's audit
    // trail past its own deletion needs a real soft-delete/archival design,
    // which is future work, not this stage.
    sqlx::query("DELETE FROM teams WHERE id = $1").bind(team_id).execute(&state.db).await?;
    Ok(StatusCode::NO_CONTENT)
}
