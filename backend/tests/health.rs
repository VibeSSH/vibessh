//! Real integration test against a real Postgres - matches this project's
//! existing convention of testing against the actual dependency rather than
//! a mock (see e.g. src-tauri's credentials.rs tests against the real OS
//! keyring). Needs a reachable database: set DATABASE_URL, e.g. via
//! backend/.env.example's local dev instance.
//!
//! `#[ignore]` - every test here needs a reachable Postgres named by
//! `DATABASE_URL` (see `backend/.env.example`), which no plain developer
//! checkout has. Without the marker these panic in `common::database_url`
//! and, because cargo stops at the first failing test binary, they took the
//! rest of `cargo test --workspace` down with them. Same convention the
//! real-host tests in `src-tauri/tests/` already use. Run them explicitly:
//! `DATABASE_URL=... cargo test -p vibessh-backend -- --ignored`.
mod common;

use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
#[ignore]
async fn health_endpoint_reports_a_real_database_connection() {
    let app = common::test_router().await;

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/health")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "ok");
    assert_eq!(json["database"], "connected");
}

#[tokio::test]
#[ignore]
async fn connect_and_migrate_is_idempotent_across_repeated_calls() {
    // Every process restart calls this again against the same database -
    // it must never fail just because migration 1..N already ran.
    vibessh_backend::connect_and_migrate(&common::database_url()).await.unwrap();
    vibessh_backend::connect_and_migrate(&common::database_url()).await.unwrap();
}
