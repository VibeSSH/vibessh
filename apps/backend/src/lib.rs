// This codebase's doc comments lean heavily on multi-line prose under a
// bullet, which `doc_lazy_continuation` wants indented to keep rustdoc's
// rendering exact. The rendering difference is cosmetic, the comments are
// the main way design decisions are recorded here, and reflowing 49 of them
// would bury real changes in whitespace diffs. Allowed deliberately, at the
// crate root, so the choice is visible rather than implicit.
#![allow(clippy::doc_lazy_continuation)]

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

pub mod ai;
pub mod audit;
pub mod auth;
pub mod device_keys;
pub mod authorize;
pub mod errors;
pub mod jwt;
pub mod models;
pub mod password;
pub mod permissions;
pub mod rate_limit;
pub mod refresh_token;
pub mod roles;
pub mod team_applications;
pub mod team_servers;
pub mod teams;
pub mod tokens;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    /// Shared across every request, because a limit that each request kept
    /// its own copy of would not be a limit.
    pub rate_limiter: Arc<rate_limit::RateLimiter>,
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
    let state = AppState { db, jwt_secret, rate_limiter: Arc::new(rate_limit::RateLimiter::new()) };
    Router::new()
        .route("/health", get(health))
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        // Takes `AnyAuthUser`, so it keeps working for the one account
        // state that cannot use anything else - see `auth::AuthUser`.
        .route("/auth/password", post(auth::change_password))
        .route("/auth/refresh", post(auth::refresh))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        // A device's own public key, and which accounts a team's members get
        // on a shared Node. Public values only - see device_keys' own doc.
        .route("/devices", get(device_keys::list_mine).post(device_keys::publish))
        .route("/devices/:key_id", delete(device_keys::revoke))
        .route("/teams/:team_id/access", get(device_keys::list_team_access))
        // Applications a team can see - a projection pushed by the install
        // that owns each one, never the record a runtime acts on.
        .route("/teams/:team_id/applications", get(team_applications::list_applications).post(team_applications::push_application))
        .route("/teams/:team_id/applications/:application_id", delete(team_applications::remove_application))
        // The hosted Vibe AI assistant. Authenticated, because the
        // allowance it spends is per account and VibeSSH pays for it.
        .route("/ai/chat", post(ai::chat))
        .route("/ai/quota", get(ai::quota))
        .route("/teams", post(teams::create_team).get(teams::list_teams))
        .route("/teams/:team_id", get(teams::get_team).delete(teams::delete_team))
        .route("/teams/:team_id/members", get(teams::list_members).post(teams::add_member))
        .route("/teams/:team_id/members/provision", post(teams::provision_member))
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
        .route("/teams/:team_id/servers", get(team_servers::list_servers).post(team_servers::create_server))
        .route("/teams/:team_id/servers/:server_id", delete(team_servers::delete_server))
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
