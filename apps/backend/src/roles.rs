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
use serde_json::json;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::audit;
use crate::auth::AuthUser;
use crate::authorize::{authorize, authorize_any, effective_permissions, ensure_can_grant};
use crate::errors::{ApiError, ApiResult, Detail};
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
    role.ok_or_else(|| ApiError::NotFound(Detail::new("role_not_found", "role not found")))
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
        return Err(ApiError::InvalidInput(Detail::new("role_name_empty", "role name cannot be empty")));
    }
    if trimmed.chars().count() > MAX_ROLE_NAME_LEN {
        return Err(ApiError::InvalidInput(Detail::new("role_name_too_long", format!("role name must be at most {MAX_ROLE_NAME_LEN} characters")).with("max", MAX_ROLE_NAME_LEN)));
    }
    if trimmed.eq_ignore_ascii_case(OWNER_ROLE_NAME) {
        return Err(ApiError::InvalidInput(Detail::new("role_name_reserved", format!("\"{OWNER_ROLE_NAME}\" is reserved for the built-in owner role")).with("name", OWNER_ROLE_NAME)));
    }
    Ok(trimmed.to_string())
}

fn validate_permission_keys(keys: &[String]) -> ApiResult<()> {
    for key in keys {
        if !permissions::is_known_permission(key) {
            return Err(ApiError::InvalidInput(Detail::new("unknown_permission", format!("unknown permission: {key}")).with("permission", key.clone())));
        }
    }
    Ok(())
}

/// A client sending the same permission twice is harmless (the resulting
/// role has that permission once, same as if it were sent once) - dedupe
/// rather than reject, since role_permissions has a real PRIMARY KEY on
/// (role_id, permission_key) that would otherwise turn a duplicate into an
/// opaque 500 from the second INSERT's unique-constraint violation.
fn dedupe_permissions(keys: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    keys.into_iter().filter(|key| seen.insert(key.clone())).collect()
}

async fn replace_role_permissions(tx: &mut Transaction<'_, Postgres>, role_id: Uuid, keys: &[String]) -> ApiResult<()> {
    sqlx::query("DELETE FROM role_permissions WHERE role_id = $1").bind(role_id).execute(&mut **tx).await?;
    for key in keys {
        sqlx::query("INSERT INTO role_permissions (role_id, permission_key) VALUES ($1, $2)")
            .bind(role_id)
            .bind(key)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

/// Gives every built-in role every permission in the catalog.
///
/// The Owner role is seeded when a team is created, with the catalog as it
/// stood **at that moment**. Add a permission afterwards and every existing
/// team's Owner silently lacks it - which does not merely look untidy: the
/// "can't grant what you don't have" rule then refuses to let the owner of
/// a team hand out the new permission at all, with an error naming a
/// permission they would reasonably believe they hold. That is exactly what
/// happened when the operations permissions were added.
///
/// Run at startup rather than written as a migration, because a migration
/// would fix it once and the next addition to the catalog would reintroduce
/// it. The role's definition is "every permission", so the code that owns
/// that definition keeps it true, on every boot, for every team - including
/// teams created by an older build.
///
/// Idempotent: `ON CONFLICT DO NOTHING` against the table's own composite
/// primary key, so a boot with nothing to do costs one statement and
/// changes nothing.
pub async fn backfill_system_role_permissions(db: &sqlx::PgPool) -> Result<u64, sqlx::Error> {
    let catalog: Vec<String> = permissions::ALL_PERMISSIONS.iter().map(|key| (*key).to_string()).collect();
    let result = sqlx::query(
        "INSERT INTO role_permissions (role_id, permission_key)
         SELECT r.id, catalog.key
         FROM roles r
         CROSS JOIN UNNEST($1::text[]) AS catalog(key)
         WHERE r.is_system = TRUE
         ON CONFLICT DO NOTHING",
    )
    .bind(&catalog)
    .execute(db)
    .await?;
    Ok(result.rows_affected())
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
    let permissions_to_grant = dedupe_permissions(body.permissions);
    ensure_can_grant(&state.db, team_id, user_id, &permissions_to_grant).await?;

    let role_id = Uuid::new_v4();
    let now = Utc::now();
    let mut tx = state.db.begin().await?;
    let insert = sqlx::query(
        "INSERT INTO roles (id, team_id, name, description, is_system, created_at, updated_at) VALUES ($1, $2, $3, $4, FALSE, $5, $5)",
    )
    .bind(role_id)
    .bind(team_id)
    .bind(&name)
    .bind(&body.description)
    .bind(now)
    .execute(&mut *tx)
    .await;

    if let Err(sqlx::Error::Database(db_err)) = &insert {
        if db_err.is_unique_violation() {
            return Err(ApiError::Conflict(Detail::new("role_name_taken", "a role with this name already exists on this team")));
        }
    }
    insert?;

    for permission in &permissions_to_grant {
        sqlx::query("INSERT INTO role_permissions (role_id, permission_key) VALUES ($1, $2)")
            .bind(role_id)
            .bind(permission)
            .execute(&mut *tx)
            .await?;
    }

    audit::record(
        &mut tx,
        team_id,
        user_id,
        audit::ROLE_CREATED,
        "role",
        Some(role_id),
        json!({ "name": name, "permissions": permissions_to_grant }),
    )
    .await?;
    tx.commit().await?;

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
        return Err(ApiError::Forbidden(Detail::new("owner_role_immutable", "the built-in owner role cannot be modified")));
    }

    let name = validate_role_name(&body.name)?;
    validate_permission_keys(&body.permissions)?;
    let permissions_to_grant = dedupe_permissions(body.permissions);
    ensure_can_grant(&state.db, team_id, user_id, &permissions_to_grant).await?;

    let now = Utc::now();
    let mut tx = state.db.begin().await?;
    let update = sqlx::query("UPDATE roles SET name = $1, description = $2, updated_at = $3 WHERE id = $4")
        .bind(&name)
        .bind(&body.description)
        .bind(now)
        .bind(role_id)
        .execute(&mut *tx)
        .await;
    if let Err(sqlx::Error::Database(db_err)) = &update {
        if db_err.is_unique_violation() {
            return Err(ApiError::Conflict(Detail::new("role_name_taken", "a role with this name already exists on this team")));
        }
    }
    update?;

    replace_role_permissions(&mut tx, role_id, &permissions_to_grant).await?;
    audit::record(
        &mut tx,
        team_id,
        user_id,
        audit::ROLE_UPDATED,
        "role",
        Some(role_id),
        json!({ "name": name, "permissions": permissions_to_grant }),
    )
    .await?;
    tx.commit().await?;

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
        return Err(ApiError::Forbidden(Detail::new("owner_role_undeletable", "the built-in owner role cannot be deleted")));
    }

    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM roles WHERE id = $1").bind(role_id).execute(&mut *tx).await?;
    audit::record(&mut tx, team_id, user_id, audit::ROLE_DELETED, "role", Some(role_id), json!({ "name": role.name })).await?;
    tx.commit().await?;
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
    authorize_any(&state.db, team_id, user_id, &[permissions::TEAM_ROLES_MANAGE, permissions::TEAM_ROLES_ASSIGN]).await?;
    // Confirms both that the role really belongs to this team and that the
    // target is really a member of it, so the insert below fails with a
    // clear ApiError instead of a raw foreign-key-violation.
    let role_to_assign = role_for_team(&state.db, team_id, body.role_id).await?;
    let role_permissions: Vec<String> =
        sqlx::query_scalar("SELECT permission_key FROM role_permissions WHERE role_id = $1").bind(role_to_assign.id).fetch_all(&state.db).await?;
    ensure_can_grant(&state.db, team_id, user_id, &role_permissions).await?;
    let target_is_member: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM team_members WHERE team_id = $1 AND user_id = $2)")
        .bind(team_id)
        .bind(target_user_id)
        .fetch_one(&state.db)
        .await?;
    if !target_is_member {
        return Err(ApiError::NotFound(Detail::new("not_a_team_member", "that user isn't a member of this team")));
    }

    let mut tx = state.db.begin().await?;
    let insert = sqlx::query("INSERT INTO member_roles (team_id, user_id, role_id) VALUES ($1, $2, $3)")
        .bind(team_id)
        .bind(target_user_id)
        .bind(body.role_id)
        .execute(&mut *tx)
        .await;
    if let Err(sqlx::Error::Database(db_err)) = &insert {
        if db_err.is_unique_violation() {
            return Err(ApiError::Conflict(Detail::new("role_already_assigned", "this member already has that role")));
        }
    }
    insert?;

    audit::record(&mut tx, team_id, user_id, audit::ROLE_ASSIGNED, "user", Some(target_user_id), json!({ "roleId": body.role_id })).await?;
    tx.commit().await?;

    Ok(StatusCode::CREATED)
}

pub async fn unassign_role(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((team_id, target_user_id, role_id)): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    let team = team_for_member(&state.db, team_id, user_id).await?;
    authorize_any(&state.db, team_id, user_id, &[permissions::TEAM_ROLES_MANAGE, permissions::TEAM_ROLES_ASSIGN]).await?;
    let role = role_for_team(&state.db, team_id, role_id).await?;
    if role.is_system && target_user_id == team.owner_id {
        return Err(ApiError::Conflict(Detail::new("owner_role_unassignable", "the owner's built-in role can't be unassigned")));
    }

    let mut tx = state.db.begin().await?;
    let affected = sqlx::query("DELETE FROM member_roles WHERE team_id = $1 AND user_id = $2 AND role_id = $3")
        .bind(team_id)
        .bind(target_user_id)
        .bind(role_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if affected == 0 {
        return Err(ApiError::NotFound(Detail::new("role_not_assigned", "that member doesn't have that role")));
    }

    audit::record(&mut tx, team_id, user_id, audit::ROLE_UNASSIGNED, "user", Some(target_user_id), json!({ "roleId": role_id })).await?;
    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}
