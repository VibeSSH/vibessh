//! Library half of the backend - the binary in `main.rs` is a thin shell
//! around this so integration tests (see `tests/`) can build the real
//! router and hit it directly, against a real database, instead of only
//! being able to test through a spawned process.
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::routing::{delete, get, post};
use axum::Router;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

pub mod audit;
pub mod auth;
pub mod authorize;
pub mod errors;
pub mod jwt;
pub mod models;
pub mod password;
pub mod permissions;
pub mod refresh_token;
pub mod roles;
pub mod teams;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    /// Shared, not copied, per request - the secret itself never changes
    /// after startup.
    pub jwt_secret: Arc<[u8]>,
}

/// Connects to Postgres and runs every pending migration in `migrations/`
/// before returning the pool - callers never get a pool backed by a
/// not-yet-migrated database.
pub async fn connect_and_migrate(database_url: &str) -> Result<PgPool, String> {
    let db = PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await
        .map_err(|err| format!("failed to connect to the database: {err}"))?;

    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .map_err(|err| format!("failed to run migrations: {err}"))?;

    Ok(db)
}

pub fn build_router(db: PgPool, jwt_secret: Arc<[u8]>) -> Router {
    let state = AppState { db, jwt_secret };
    Router::new()
        .route("/health", get(health))
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/refresh", post(auth::refresh))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        .route("/teams", post(teams::create_team).get(teams::list_teams))
        .route("/teams/:team_id", get(teams::get_team).delete(teams::delete_team))
        .route("/teams/:team_id/members", get(teams::list_members).post(teams::add_member))
        .route("/teams/:team_id/members/:user_id", delete(teams::remove_member))
        .route("/permissions", get(roles::list_permissions))
        .route("/teams/:team_id/roles", get(roles::list_roles).post(roles::create_role))
        .route("/teams/:team_id/roles/:role_id", get(roles::get_role).patch(roles::update_role).delete(roles::delete_role))
        .route(
            "/teams/:team_id/members/:user_id/roles",
            get(roles::list_member_roles).post(roles::assign_role),
        )
        .route("/teams/:team_id/members/:user_id/roles/:role_id", delete(roles::unassign_role))
        .route("/teams/:team_id/me/permissions", get(roles::my_permissions))
        .route("/teams/:team_id/audit", get(audit::list_audit_events))
        .with_state(state)
}

/// Real connectivity check, not just "the process is running" - a load
/// balancer or orchestrator using this to gate traffic should see a 503 the
/// moment the database is actually unreachable, not a false "ok".
async fn health(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&state.db).await {
        Ok(_) => (StatusCode::OK, Json(json!({ "status": "ok", "database": "connected" }))),
        Err(err) => {
            log::error!("health check: database query failed: {err}");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "status": "error", "database": "unreachable" })),
            )
        }
    }
}
