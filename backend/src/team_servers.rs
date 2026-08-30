//! Team-scoped server *metadata* - see migrations/0006 for why this holds
//! no secrets. Reading the list only requires team membership (same as
//! reading roles/members); adding or removing an entry requires
//! `servers.manage`.
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use crate::audit;
use crate::auth::AuthUser;
use crate::authorize::authorize;
use crate::errors::{ApiError, ApiResult};
use crate::models::{CreateTeamServerRequest, TeamServer};
use crate::teams::team_for_member;
use crate::{permissions, AppState};

const MAX_NAME_LEN: usize = 100;
const MAX_HOST_LEN: usize = 255;

pub async fn list_servers(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(team_id): Path<Uuid>,
) -> ApiResult<Json<Vec<TeamServer>>> {
    team_for_member(&state.db, team_id, user_id).await?;

    let servers: Vec<TeamServer> = sqlx::query_as(
        "SELECT id, team_id, name, host, ssh_port, username, created_at FROM team_servers WHERE team_id = $1 ORDER BY created_at",
    )
    .bind(team_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(servers))
}

pub async fn create_server(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(team_id): Path<Uuid>,
    Json(body): Json<CreateTeamServerRequest>,
) -> ApiResult<impl IntoResponse> {
    team_for_member(&state.db, team_id, user_id).await?;
    authorize(&state.db, team_id, user_id, permissions::SERVERS_MANAGE).await?;

    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::InvalidInput("server name cannot be empty".to_string()));
    }
    if name.chars().count() > MAX_NAME_LEN {
        return Err(ApiError::InvalidInput(format!("server name must be at most {MAX_NAME_LEN} characters")));
    }
    let host = body.host.trim();
    if host.is_empty() {
        return Err(ApiError::InvalidInput("host cannot be empty".to_string()));
    }
    if host.chars().count() > MAX_HOST_LEN {
        return Err(ApiError::InvalidInput(format!("host must be at most {MAX_HOST_LEN} characters")));
    }
    let ssh_port = body.ssh_port.unwrap_or(22);
    if !(1..=65535).contains(&ssh_port) {
        return Err(ApiError::InvalidInput("ssh port must be between 1 and 65535".to_string()));
    }

    let server_id = Uuid::new_v4();
    let now = Utc::now();
    let mut tx = state.db.begin().await?;
    sqlx::query(
        "INSERT INTO team_servers (id, team_id, name, host, ssh_port, username, added_by, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8)",
    )
    .bind(server_id)
    .bind(team_id)
    .bind(name)
    .bind(host)
    .bind(ssh_port)
    .bind(body.username.as_deref())
    .bind(user_id)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    audit::record(&mut tx, team_id, user_id, audit::SERVER_ADDED, "server", Some(server_id), json!({ "name": name, "host": host }))
        .await?;
    tx.commit().await?;

    let server = TeamServer { id: server_id, team_id, name: name.to_string(), host: host.to_string(), ssh_port, username: body.username, created_at: now };
    Ok((StatusCode::CREATED, Json(server)))
}

pub async fn delete_server(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((team_id, server_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    team_for_member(&state.db, team_id, user_id).await?;
    authorize(&state.db, team_id, user_id, permissions::SERVERS_MANAGE).await?;

    let mut tx = state.db.begin().await?;
    let affected = sqlx::query("DELETE FROM team_servers WHERE id = $1 AND team_id = $2")
        .bind(server_id)
        .bind(team_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if affected == 0 {
        return Err(ApiError::NotFound("server not found".to_string()));
    }

    audit::record(&mut tx, team_id, user_id, audit::SERVER_REMOVED, "server", Some(server_id), json!({})).await?;
    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}
