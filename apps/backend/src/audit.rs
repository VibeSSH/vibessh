//! Audit Log. `record()` is called from inside the same database
//! transaction as the mutation it's recording, everywhere it's used (see
//! teams.rs/roles.rs) - an action and its audit event either both commit or
//! neither does, so "this succeeded" can never happen without "and it was
//! logged" also being true. Action names are dot-namespaced strings
//! (matching the permission-key style already established in
//! permissions.rs) rather than a Rust enum - new action kinds are added
//! freely as new mutations are built, without needing a matching database
//! migration each time (the column is TEXT, not an enum type).
use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::Utc;
use serde::Deserialize;
use serde_json::Value as JsonValue;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::authorize::authorize;
use crate::errors::ApiResult;
use crate::models::AuditEvent;
use crate::teams::team_for_member;
use crate::{permissions, AppState};

pub const TEAM_CREATED: &str = "team.created";
pub const TEAM_DELETED: &str = "team.deleted";
pub const MEMBER_ADDED: &str = "member.added";
pub const MEMBER_REMOVED: &str = "member.removed";
pub const ROLE_CREATED: &str = "role.created";
pub const ROLE_UPDATED: &str = "role.updated";
pub const ROLE_DELETED: &str = "role.deleted";
pub const ROLE_ASSIGNED: &str = "role.assigned";
pub const ROLE_UNASSIGNED: &str = "role.unassigned";
pub const INVITATION_CREATED: &str = "invitation.created";
pub const INVITATION_REVOKED: &str = "invitation.revoked";
pub const APPLICATION_SHARED: &str = "application.shared";
pub const APPLICATION_UNSHARED: &str = "application.unshared";
/// A member was allowed to, or stopped from, seeing one shared Application -
/// the per-application allow-list, distinct from the whole-team share above.
pub const APPLICATION_ACCESS_GRANTED: &str = "application.access_granted";
pub const APPLICATION_ACCESS_REVOKED: &str = "application.access_revoked";
/// Recorded when a member's account actually came off a Node, not when its
/// removal was asked for - the asking is part of `MEMBER_REMOVED`.
pub const ACCESS_REVOKED: &str = "access.revoked";
pub const SERVER_ADDED: &str = "server.added";
pub const SERVER_REMOVED: &str = "server.removed";
pub const INVITATION_ACCEPTED: &str = "invitation.accepted";
pub const INVITATION_DECLINED: &str = "invitation.declined";

#[allow(clippy::too_many_arguments)]
pub async fn record(
    tx: &mut Transaction<'_, Postgres>,
    team_id: Uuid,
    actor_id: Uuid,
    action: &str,
    target_type: &str,
    target_id: Option<Uuid>,
    metadata: JsonValue,
) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO audit_events (id, team_id, actor_id, action, target_type, target_id, metadata, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(Uuid::new_v4())
    .bind(team_id)
    .bind(actor_id)
    .bind(action)
    .bind(target_type)
    .bind(target_id)
    .bind(metadata)
    .bind(Utc::now())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[derive(Deserialize)]
pub struct ListAuditEventsQuery {
    limit: Option<i64>,
    offset: Option<i64>,
}

const DEFAULT_PAGE_SIZE: i64 = 50;
const MAX_PAGE_SIZE: i64 = 200;

pub async fn list_audit_events(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(team_id): Path<Uuid>,
    Query(query): Query<ListAuditEventsQuery>,
) -> ApiResult<Json<Vec<AuditEvent>>> {
    team_for_member(&state.db, team_id, user_id).await?;
    authorize(&state.db, team_id, user_id, permissions::AUDIT_VIEW).await?;

    let limit = query.limit.unwrap_or(DEFAULT_PAGE_SIZE).clamp(1, MAX_PAGE_SIZE);
    let offset = query.offset.unwrap_or(0).max(0);

    let events: Vec<AuditEvent> = sqlx::query_as(
        "SELECT e.id, e.action, e.target_type, e.target_id, e.result, e.metadata, e.created_at,
                e.actor_id, u.email AS actor_email, u.display_name AS actor_display_name
         FROM audit_events e
         LEFT JOIN users u ON u.id = e.actor_id
         WHERE e.team_id = $1
         ORDER BY e.created_at DESC
         LIMIT $2 OFFSET $3",
    )
    .bind(team_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(events))
}
