//! Roles + Permissions: real team-scoped roles, each with a set of
//! permissions drawn from the catalog in permissions.rs, and member_roles
//! assigning roles to team members. Managing roles/assignments requires the
//! `team.roles.manage` permission (see authorize.rs) - anyone a team's
//! owner grants that permission to can manage roles, not only the owner.
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::authorize::{authorize, effective_permissions};
use crate::errors::{ApiError, ApiResult};
use crate::models::{AssignRoleRequest, CreateRoleRequest, Role, RoleWithPermissions, UpdateRoleRequest};
use crate::permissions;
use crate::teams::{team_for_member, OWNER_ROLE_NAME};
use crate::AppState;

const MAX_ROLE_NAME_LEN: usize = 60;

async fn role_for_team(db: &PgPool, team_id: Uuid, role_id: Uuid) -> ApiResult<Role> {
    let role: Option<Role> = sqlx::query_as(
        "SELECT id, team_id, name, description, is_system, created_at FROM roles WHERE id = $1 AND team_id = $2",
    )
    .bind(role_id)
    .bind(team_id)
    .fetch_optional(db)
    .await?;
    role.ok_or_else(|| ApiError::NotFound("role not found".to_string()))
}

async fn with_permissions(db: &PgPool, role: Role) -> ApiResult<RoleWithPermissions> {
    let permissions: Vec<String> =
        sqlx::query_scalar("SELECT permission_key FROM role_permissions WHERE role_id = $1 ORDER BY permission_key")
            .bind(role.id)
            .fetch_all(db)
            .await?;
    Ok(RoleWithPermissions { role, permissions })
}

fn validate_role_name(name: &str) -> ApiResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ApiError::InvalidInput("role name cannot be empty".to_string()));
    }
    if trimmed.chars().count() > MAX_ROLE_NAME_LEN {
        return Err(ApiError::InvalidInput(format!("role name must be at most {MAX_ROLE_NAME_LEN} characters")));
    }
    if trimmed.eq_ignore_ascii_case(OWNER_ROLE_NAME) {
        return Err(ApiError::InvalidInput(format!("\"{OWNER_ROLE_NAME}\" is reserved for the built-in owner role")));
    }
    Ok(trimmed.to_string())
}

fn validate_permission_keys(keys: &[String]) -> ApiResult<()> {
    for key in keys {
        if !permissions::is_known_permission(key) {
            return Err(ApiError::InvalidInput(format!("unknown permission: {key}")));
        }
    }
    Ok(())
}

async fn replace_role_permissions(db: &PgPool, role_id: Uuid, keys: &[String]) -> ApiResult<()> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM role_permissions WHERE role_id = $1").bind(role_id).execute(&mut *tx).await?;
    for key in keys {
        sqlx::query("INSERT INTO role_permissions (role_id, permission_key) VALUES ($1, $2)")
            .bind(role_id)
            .bind(key)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn list_permissions() -> Json<&'static [&'static str]> {
    Json(permissions::ALL_PERMISSIONS)
}

/// The caller's own effective permissions on this team - what a frontend
/// would call once after loading a team to drive `can(permission)`-style
/// UI gating (hiding/disabling actions the user can't take). Never the
/// actual enforcement on its own; every write endpoint still calls
/// `authorize()` itself server-side regardless of what this reports.
pub async fn my_permissions(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(team_id): Path<Uuid>,
) -> ApiResult<Json<Vec<String>>> {
    team_for_member(&state.db, team_id, user_id).await?;
    Ok(Json(effective_permissions(&state.db, team_id, user_id).await?))
}

pub async fn create_role(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(team_id): Path<Uuid>,
    Json(body): Json<CreateRoleRequest>,
) -> ApiResult<impl IntoResponse> {
    team_for_member(&state.db, team_id, user_id).await?;
    authorize(&state.db, team_id, user_id, permissions::TEAM_ROLES_MANAGE).await?;

    let name = validate_role_name(&body.name)?;
    validate_permission_keys(&body.permissions)?;

    let role_id = Uuid::new_v4();
    let now = Utc::now();
    let insert = sqlx::query(
        "INSERT INTO roles (id, team_id, name, description, is_system, created_at, updated_at) VALUES ($1, $2, $3, $4, FALSE, $5, $5)",
    )
    .bind(role_id)
    .bind(team_id)
    .bind(&name)
    .bind(&body.description)
    .bind(now)
    .execute(&state.db)
    .await;

    if let Err(sqlx::Error::Database(db_err)) = &insert {
        if db_err.is_unique_violation() {
            return Err(ApiError::Conflict("a role with this name already exists on this team".to_string()));
        }
    }
    insert?;

    for permission in &body.permissions {
        sqlx::query("INSERT INTO role_permissions (role_id, permission_key) VALUES ($1, $2)")
            .bind(role_id)
            .bind(permission)
            .execute(&state.db)
            .await?;
    }

    let role = Role { id: role_id, team_id, name, description: body.description, is_system: false, created_at: now };
    Ok((StatusCode::CREATED, Json(with_permissions(&state.db, role).await?)))
}

pub async fn list_roles(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(team_id): Path<Uuid>,
) -> ApiResult<Json<Vec<RoleWithPermissions>>> {
    team_for_member(&state.db, team_id, user_id).await?;

    let roles: Vec<Role> =
        sqlx::query_as("SELECT id, team_id, name, description, is_system, created_at FROM roles WHERE team_id = $1 ORDER BY created_at")
            .bind(team_id)
            .fetch_all(&state.db)
            .await?;

    let mut result = Vec::with_capacity(roles.len());
    for role in roles {
        result.push(with_permissions(&state.db, role).await?);
    }
    Ok(Json(result))
}

pub async fn get_role(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((team_id, role_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<RoleWithPermissions>> {
    team_for_member(&state.db, team_id, user_id).await?;
    let role = role_for_team(&state.db, team_id, role_id).await?;
    Ok(Json(with_permissions(&state.db, role).await?))
}

pub async fn update_role(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((team_id, role_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateRoleRequest>,
) -> ApiResult<Json<RoleWithPermissions>> {
    team_for_member(&state.db, team_id, user_id).await?;
    authorize(&state.db, team_id, user_id, permissions::TEAM_ROLES_MANAGE).await?;
    let role = role_for_team(&state.db, team_id, role_id).await?;
    if role.is_system {
        return Err(ApiError::Forbidden("the built-in owner role cannot be modified".to_string()));
    }

    let name = validate_role_name(&body.name)?;
    validate_permission_keys(&body.permissions)?;

    let now = Utc::now();
    let update = sqlx::query("UPDATE roles SET name = $1, description = $2, updated_at = $3 WHERE id = $4")
        .bind(&name)
        .bind(&body.description)
        .bind(now)
        .bind(role_id)
        .execute(&state.db)
        .await;
    if let Err(sqlx::Error::Database(db_err)) = &update {
        if db_err.is_unique_violation() {
            return Err(ApiError::Conflict("a role with this name already exists on this team".to_string()));
        }
    }
    update?;

    replace_role_permissions(&state.db, role_id, &body.permissions).await?;

    let updated = Role { id: role_id, team_id, name, description: body.description, is_system: false, created_at: role.created_at };
    Ok(Json(with_permissions(&state.db, updated).await?))
}

pub async fn delete_role(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((team_id, role_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    team_for_member(&state.db, team_id, user_id).await?;
    authorize(&state.db, team_id, user_id, permissions::TEAM_ROLES_MANAGE).await?;
    let role = role_for_team(&state.db, team_id, role_id).await?;
    if role.is_system {
        return Err(ApiError::Forbidden("the built-in owner role cannot be deleted".to_string()));
    }

    sqlx::query("DELETE FROM roles WHERE id = $1").bind(role_id).execute(&state.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_member_roles(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((team_id, target_user_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Vec<Role>>> {
    team_for_member(&state.db, team_id, user_id).await?;

    let roles: Vec<Role> = sqlx::query_as(
        "SELECT r.id, r.team_id, r.name, r.description, r.is_system, r.created_at
         FROM roles r
         JOIN member_roles mr ON mr.role_id = r.id
         WHERE mr.team_id = $1 AND mr.user_id = $2
         ORDER BY r.created_at",
    )
    .bind(team_id)
    .bind(target_user_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(roles))
}

pub async fn assign_role(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((team_id, target_user_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<AssignRoleRequest>,
) -> ApiResult<StatusCode> {
    team_for_member(&state.db, team_id, user_id).await?;
    authorize(&state.db, team_id, user_id, permissions::TEAM_ROLES_MANAGE).await?;
    // Confirms both that the role really belongs to this team and that the
    // target is really a member of it, so the insert below fails with a
    // clear ApiError instead of a raw foreign-key-violation.
    role_for_team(&state.db, team_id, body.role_id).await?;
    let target_is_member: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM team_members WHERE team_id = $1 AND user_id = $2)")
        .bind(team_id)
        .bind(target_user_id)
        .fetch_one(&state.db)
        .await?;
    if !target_is_member {
        return Err(ApiError::NotFound("that user isn't a member of this team".to_string()));
    }

    let insert = sqlx::query("INSERT INTO member_roles (team_id, user_id, role_id) VALUES ($1, $2, $3)")
        .bind(team_id)
        .bind(target_user_id)
        .bind(body.role_id)
        .execute(&state.db)
        .await;
    if let Err(sqlx::Error::Database(db_err)) = &insert {
        if db_err.is_unique_violation() {
            return Err(ApiError::Conflict("this member already has that role".to_string()));
        }
    }
    insert?;

    Ok(StatusCode::CREATED)
}

pub async fn unassign_role(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((team_id, target_user_id, role_id)): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    let team = team_for_member(&state.db, team_id, user_id).await?;
    authorize(&state.db, team_id, user_id, permissions::TEAM_ROLES_MANAGE).await?;
    let role = role_for_team(&state.db, team_id, role_id).await?;
    if role.is_system && target_user_id == team.owner_id {
        return Err(ApiError::Conflict("the owner's built-in role can't be unassigned".to_string()));
    }

    let affected = sqlx::query("DELETE FROM member_roles WHERE team_id = $1 AND user_id = $2 AND role_id = $3")
        .bind(team_id)
        .bind(target_user_id)
        .bind(role_id)
        .execute(&state.db)
        .await?
        .rows_affected();
    if affected == 0 {
        return Err(ApiError::NotFound("that member doesn't have that role".to_string()));
    }

    Ok(StatusCode::NO_CONTENT)
}
