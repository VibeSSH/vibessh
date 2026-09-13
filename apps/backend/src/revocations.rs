//! Access that has been taken away here but is still on somebody's server.
//!
//! Removing a member from a team is one statement in this backend. Their
//! account on each Node the team shares is a file on a machine this backend
//! cannot reach, and only an install with SSH access can remove it. That gap
//! is the reason this module exists: the intent is recorded when the member
//! is removed, and whichever install next syncs that Node completes it.
//!
//! **Pending is reported as pending.** Nothing here marks a revocation done
//! because it was asked for. `complete` is called by an install that has
//! just run the removal on the Node and watched it succeed - see
//! `team_access_service::sync_team_access` on the desktop. The difference
//! between "they no longer have access" and "we asked" is the whole feature,
//! and a list that blurred the two would be worse than no list.
//!
//! See `docs/planning/team-access-design.md`, "Revocation".

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use serde_json::json;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::audit;
use crate::auth::AuthUser;
use crate::authorize::authorize;
use crate::device_keys::node_username;
use crate::errors::{ApiError, ApiResult, Detail};
use crate::models::NodeRevocation;
use crate::teams::team_for_member;
use crate::{permissions, AppState};

/// Records that this member's account has to come off every Node the team
/// shares.
///
/// Runs inside the caller's transaction, so a member removal that fails
/// leaves no revocations owed for a member who is still in the team - the
/// two facts are one fact and are written as one.
///
/// A team sharing no Nodes writes nothing and is not an error: there is
/// genuinely nothing owed.
pub async fn record_for_removed_member(
    tx: &mut Transaction<'_, Postgres>,
    team_id: Uuid,
    user_id: Uuid,
    email: &str,
    requested_by: Uuid,
) -> ApiResult<u64> {
    // One statement covering every team server rather than a query per row.
    // That means the id comes from the database here, unlike everywhere else
    // in this codebase, which generates it in Rust - `gen_random_uuid()` is
    // core Postgres since 13 and the deployment pins 17.
    //
    // `ON CONFLICT DO NOTHING` against the partial unique index: somebody
    // removed, re-added and removed again before anybody synced is owed one
    // removal, not two, and the first record is the older and truer one.
    let written = sqlx::query(
        "INSERT INTO node_revocations \
           (id, team_id, team_server_id, user_id, node_username, email, requested_at, requested_by) \
         SELECT gen_random_uuid(), $1, s.id, $2, $3, $4, $5, $6 FROM team_servers s WHERE s.team_id = $1 \
         ON CONFLICT DO NOTHING",
    )
    .bind(team_id)
    .bind(user_id)
    .bind(node_username(user_id))
    .bind(email)
    .bind(Utc::now())
    .bind(requested_by)
    .execute(&mut **tx)
    .await?
    .rows_affected();

    Ok(written)
}

/// Cancels anything owed for somebody who is back in the team.
///
/// Without this, a member removed and re-added before anybody synced would
/// be granted their account and then have it taken away again in the same
/// sync - the record still said to remove them - leaving a current member
/// with no access and nothing on screen explaining why.
///
/// Deleted rather than marked completed. "Completed" is a claim that
/// something happened on a machine; nothing did, and the removal simply
/// stopped being owed.
pub async fn cancel_for_returning_member(tx: &mut Transaction<'_, Postgres>, team_id: Uuid, user_id: Uuid) -> ApiResult<u64> {
    let cancelled = sqlx::query("DELETE FROM node_revocations WHERE team_id = $1 AND user_id = $2 AND completed_at IS NULL")
        .bind(team_id)
        .bind(user_id)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    Ok(cancelled)
}

/// What is still owed on every Node this team shares.
///
/// Membership is all this needs, matching `list_team_access`: an email, an
/// account name derived from a user id, and a host the team already stores
/// are all things any member can already read. Hiding the list from the
/// people who would notice an overdue revocation would be the wrong way
/// round.
pub async fn list_pending(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(team_id): Path<Uuid>,
) -> ApiResult<Json<Vec<NodeRevocation>>> {
    team_for_member(&state.db, team_id, user_id).await?;

    let rows: Vec<NodeRevocation> = sqlx::query_as(
        "SELECT r.id, r.team_id, r.team_server_id, s.name AS server_name, s.host, s.ssh_port, \
                r.user_id, r.node_username, r.email, r.requested_at \
         FROM node_revocations r \
         JOIN team_servers s ON s.id = r.team_server_id \
         WHERE r.team_id = $1 AND r.completed_at IS NULL \
         ORDER BY r.requested_at",
    )
    .bind(team_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

/// Marks one revocation as having actually landed on the Node.
///
/// Called only after the removal ran there and returned success. The update
/// is conditional on the row still being pending, so two installs syncing
/// the same Node at once produce one completion and one honest "already
/// done" rather than two audit entries for one event.
pub async fn complete(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((team_id, revocation_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    team_for_member(&state.db, team_id, user_id).await?;
    authorize(&state.db, team_id, user_id, permissions::SERVERS_MANAGE).await?;

    let mut tx = state.db.begin().await?;
    let row: Option<(String, String)> = sqlx::query_as(
        "UPDATE node_revocations SET completed_at = $1, completed_by = $2 \
         WHERE id = $3 AND team_id = $4 AND completed_at IS NULL \
         RETURNING node_username, email",
    )
    .bind(Utc::now())
    .bind(user_id)
    .bind(revocation_id)
    .bind(team_id)
    .fetch_optional(&mut *tx)
    .await?;

    let Some((account, email)) = row else {
        return Err(ApiError::NotFound(Detail::new(
            "revocation_not_pending",
            "that revocation is not pending on this team - it may already have been completed",
        )));
    };

    audit::record(
        &mut tx,
        team_id,
        user_id,
        audit::ACCESS_REVOKED,
        "revocation",
        Some(revocation_id),
        json!({ "email": email, "account": account }),
    )
    .await?;
    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}
