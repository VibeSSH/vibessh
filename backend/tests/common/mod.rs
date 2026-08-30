//! Shared by every integration test file - not a test binary itself (lives
//! in a `tests/<name>/` subdirectory, which cargo doesn't treat as its own
//! test target the way a direct `tests/*.rs` file is).
use std::sync::Arc;

use axum::Router;
use sqlx::PgPool;

pub fn database_url() -> String {
    std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set to run backend integration tests - see backend/.env.example")
}

pub fn test_jwt_secret() -> Arc<[u8]> {
    Arc::from(b"test-only-jwt-secret-at-least-32-bytes-long".as_slice())
}

pub async fn test_db() -> PgPool {
    vibessh_backend::connect_and_migrate(&database_url())
        .await
        .expect("connecting and migrating should succeed against a reachable database")
}

pub async fn test_router() -> Router {
    vibessh_backend::build_router(test_db().await, test_jwt_secret())
}

/// A fresh, guaranteed-not-already-registered email for tests that create a
/// real user row - random rather than a fixed constant so tests can run
/// repeatedly against the same persistent database without colliding on a
/// leftover row from a previous run.
pub fn unique_email() -> String {
    format!("test-{}@example.com", uuid::Uuid::new_v4())
}
