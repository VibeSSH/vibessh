//! Applications a team can see.
//!
//! A projection of the install that owns each one - see
//! `migrations/0011_team_applications.sql` for why it is a projection and why
//! secret environment values are not in it.
//!
//! Reading requires team membership, the same as the server list. Pushing
//! and removing require `applications.create` and `applications.delete`,
//! which is the first time those permissions mean anything this backend can
//! actually enforce: the record lives here, so refusing to write it is a
//! refusal that holds.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use uuid::Uuid;

use crate::audit;
use crate::auth::AuthUser;
use crate::authorize::authorize;
use crate::errors::{ApiError, ApiResult, Detail};
use crate::models::{PushTeamApplicationRequest, TeamApplication};
use crate::teams::team_for_member;
use crate::{permissions, AppState};

const MAX_NAME_LEN: usize = 100;
const MAX_PATH_LEN: usize = 512;

/// How many ports and variables one Application may project.
///
/// Not a guess at what is reasonable so much as a bound on what one member
/// can make every other member's install download. Far above anything a real
/// Application has.
const MAX_ENTRIES: usize = 200;

pub async fn list_applications(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(team_id): Path<Uuid>,
) -> ApiResult<Json<Vec<TeamApplication>>> {
    team_for_member(&state.db, team_id, user_id).await?;

    let applications: Vec<TeamApplication> = sqlx::query_as(
        "SELECT id, team_id, team_server_id, local_id, name, blueprint_id, runtime_type, working_directory, \
                ports, environment, updated_at \
         FROM team_applications WHERE team_id = $1 ORDER BY name",
    )
    .bind(team_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(applications))
}

/// Trims and bounds, and leaves the refusal to the caller.
///
/// The caller builds its own `Detail` with the code written out, rather than
/// this taking one as an argument. That is not ceremony: the test that keeps
/// both locale files honest reads the codes out of this source, and a code
/// passed to a helper is a code it cannot see. A guard that silently misses
/// half of what it guards is worse than none.
fn bounded(value: &str, max: usize) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > max {
        return None;
    }
    Some(trimmed.to_string())
}



/// Publishes one Application to the team, or replaces what was there.
///
/// Idempotent on `(team, local_id)`, so an install that pushes on every
/// change updates one row rather than accumulating one per push.
pub async fn push_application(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(team_id): Path<Uuid>,
    Json(body): Json<PushTeamApplicationRequest>,
) -> ApiResult<impl IntoResponse> {
    authorize(&state.db, team_id, user_id, permissions::APPLICATIONS_CREATE).await?;

    // Written out one by one rather than through a helper that takes the
    // code. Four lines of repetition buys a guarantee: the test that keeps
    // both locale files honest reads these literals out of this file, and it
    // cannot see a code that only exists as a variable.
    let name = bounded(&body.name, MAX_NAME_LEN).ok_or_else(|| {
        ApiError::InvalidInput(
            Detail::new("application_name_invalid", format!("the application name must be between 1 and {MAX_NAME_LEN} characters"))
                .with("max", MAX_NAME_LEN),
        )
    })?;
    let blueprint_id = bounded(&body.blueprint_id, MAX_NAME_LEN).ok_or_else(|| {
        ApiError::InvalidInput(
            Detail::new("application_blueprint_invalid", format!("the blueprint must be between 1 and {MAX_NAME_LEN} characters"))
                .with("max", MAX_NAME_LEN),
        )
    })?;
    let runtime_type = bounded(&body.runtime_type, MAX_NAME_LEN).ok_or_else(|| {
        ApiError::InvalidInput(
            Detail::new("application_runtime_invalid", format!("the runtime type must be between 1 and {MAX_NAME_LEN} characters"))
                .with("max", MAX_NAME_LEN),
        )
    })?;
    let working_directory = bounded(&body.working_directory, MAX_PATH_LEN).ok_or_else(|| {
        ApiError::InvalidInput(
            Detail::new("application_directory_invalid", format!("the working directory must be between 1 and {MAX_PATH_LEN} characters"))
                .with("max", MAX_PATH_LEN),
        )
    })?;

    if body.ports.len() > MAX_ENTRIES || body.environment.len() > MAX_ENTRIES {
        return Err(ApiError::InvalidInput(
            Detail::new("application_too_many_entries", format!("an application may project at most {MAX_ENTRIES} ports or variables"))
                .with("max", MAX_ENTRIES),
        ));
    }

    let now = Utc::now();
    let mut tx = state.db.begin().await?;
    let application: TeamApplication = sqlx::query_as(
        "INSERT INTO team_applications \
            (id, team_id, team_server_id, local_id, name, blueprint_id, runtime_type, working_directory, ports, environment, pushed_by, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $12) \
         ON CONFLICT (team_id, local_id) DO UPDATE SET \
            team_server_id = $3, name = $5, blueprint_id = $6, runtime_type = $7, working_directory = $8, \
            ports = $9, environment = $10, pushed_by = $11, updated_at = $12 \
         RETURNING id, team_id, team_server_id, local_id, name, blueprint_id, runtime_type, working_directory, ports, environment, updated_at",
    )
    .bind(Uuid::new_v4())
    .bind(team_id)
    .bind(body.team_server_id)
    .bind(body.local_id)
    .bind(&name)
    .bind(&blueprint_id)
    .bind(&runtime_type)
    .bind(&working_directory)
    .bind(serde_json::to_value(&body.ports).unwrap_or_else(|_| serde_json::json!([])))
    .bind(serde_json::to_value(&body.environment).unwrap_or_else(|_| serde_json::json!([])))
    .bind(user_id)
    .bind(now)
    .fetch_one(&mut *tx)
    .await?;

    audit::record(
        &mut tx,
        team_id,
        user_id,
        audit::APPLICATION_SHARED,
        "application",
        Some(application.id),
        serde_json::json!({ "name": name }),
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::OK, Json(application)))
}

/// Stops sharing an Application with the team.
///
/// Removes the projection and nothing else. The Application keeps running on
/// its Node and the install that owns it is untouched - anything offering
/// this has to say so, because "remove" next to an application name reads
/// like deletion.
pub async fn remove_application(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((team_id, application_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<impl IntoResponse> {
    authorize(&state.db, team_id, user_id, permissions::APPLICATIONS_DELETE).await?;

    let mut tx = state.db.begin().await?;
    let deleted = sqlx::query("DELETE FROM team_applications WHERE id = $1 AND team_id = $2")
        .bind(application_id)
        .bind(team_id)
        .execute(&mut *tx)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::NotFound(Detail::new("application_not_shared", "that application is not shared with this team")));
    }
    audit::record(&mut tx, team_id, user_id, audit::APPLICATION_UNSHARED, "application", Some(application_id), serde_json::json!({})).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_trimmed_and_bounded() {
        assert_eq!(bounded("  paper  ", MAX_NAME_LEN).unwrap(), "paper");
        assert!(bounded("   ", MAX_NAME_LEN).is_none());
        assert!(bounded(&"x".repeat(MAX_NAME_LEN + 1), MAX_NAME_LEN).is_none());
    }

    /// Counted in characters rather than bytes: a limit that rejects a name
    /// for being in Polish would be a bug rather than a limit.
    #[test]
    fn the_length_limit_counts_characters_not_bytes() {
        assert!(bounded(&"ą".repeat(MAX_NAME_LEN), MAX_NAME_LEN).is_some());
    }
}
