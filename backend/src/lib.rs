//! Library half of the backend - the binary in `main.rs` is a thin shell
//! around this so integration tests (see `tests/health.rs`) can build the
//! real router and hit it directly, against a real database, instead of
//! only being able to test through a spawned process.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
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

pub fn build_router(db: PgPool) -> Router {
    let state = AppState { db };
    Router::new().route("/health", get(health)).with_state(state)
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
