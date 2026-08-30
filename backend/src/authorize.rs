//! The single source of truth for "can this user do this on this team" -
//! every write/management endpoint calls `authorize()` instead of comparing
//! against `team.owner_id` directly. That comparison is what every one of
//! those endpoints did before this stage (see teams.rs/roles.rs's git
//! history) - behaviorally identical today, since the only role that
//! exists so far is the seeded "Owner" role with every permission, but now
//! a custom role actually *means* something: granting it `team.roles.manage`
//! really does let that member manage roles, without needing to be the
//! team's owner_id.
//!
//! Structural invariants that aren't really permission checks - "the owner
//! can't be removed as a member", "the built-in Owner role can't be
//! deleted" - stay as direct `team.owner_id` / `role.is_system` comparisons
//! in teams.rs/roles.rs, on top of (not instead of) this. Those aren't
//! about what the *acting* user is allowed to do; they're about protecting
//! specific rows regardless of who's asking.
use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::{ApiError, ApiResult};
use crate::permissions;

pub async fn authorize(db: &PgPool, team_id: Uuid, user_id: Uuid, permission: &str) -> ApiResult<()> {
    debug_assert!(
        permissions::is_known_permission(permission),
        "authorize() called with a permission key not in the catalog: {permission}"
    );

    let granted: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM member_roles mr
            JOIN role_permissions rp ON rp.role_id = mr.role_id
            WHERE mr.team_id = $1 AND mr.user_id = $2 AND rp.permission_key = $3
        )",
    )
    .bind(team_id)
    .bind(user_id)
    .bind(permission)
    .fetch_one(db)
    .await?;

    if !granted {
        return Err(ApiError::Forbidden(format!("missing permission: {permission}")));
    }
    Ok(())
}

/// Every permission the union of a member's assigned roles grants them on a
/// team - what a client-side `can(permission)` check (UX-only hiding/
/// disabling, never the actual enforcement - that's always this module's
/// `authorize()`, server-side) would be built on.
pub async fn effective_permissions(db: &PgPool, team_id: Uuid, user_id: Uuid) -> ApiResult<Vec<String>> {
    let permissions: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT rp.permission_key
         FROM member_roles mr
         JOIN role_permissions rp ON rp.role_id = mr.role_id
         WHERE mr.team_id = $1 AND mr.user_id = $2
         ORDER BY rp.permission_key",
    )
    .bind(team_id)
    .bind(user_id)
    .fetch_all(db)
    .await?;
    Ok(permissions)
}

/// The "can't grant what you don't have" rule: rejects if `desired`
/// contains any permission the actor doesn't themselves currently hold.
/// `team.roles.manage` alone is not enough to define a role, update an
/// existing one's permission set, assign a role (including the built-in
/// Owner role) to someone, or invite someone with a role attached, if doing
/// so would hand out more power than the actor themselves has - otherwise
/// that one permission would be a de facto "become the owner" button.
/// Every one of those call sites must call this in addition to `authorize`,
/// not instead of it: `authorize` checks the actor is allowed to manage
/// roles at all, this checks the specific grant they're attempting isn't
/// wider than their own reach.
pub async fn ensure_can_grant(db: &PgPool, team_id: Uuid, actor_id: Uuid, desired: &[String]) -> ApiResult<()> {
    let held = effective_permissions(db, team_id, actor_id).await?;
    let held: std::collections::HashSet<&str> = held.iter().map(String::as_str).collect();
    if let Some(missing) = desired.iter().find(|permission| !held.contains(permission.as_str())) {
        return Err(ApiError::Forbidden(format!("you can't grant a permission you don't have: {missing}")));
    }
    Ok(())
}
