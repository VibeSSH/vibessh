//! Real integration test against a real Postgres - matches this project's
//! existing convention of testing against the actual dependency rather than
//! a mock (see e.g. src-tauri's credentials.rs tests against the real OS
//! keyring). Needs a reachable database: set DATABASE_URL, e.g. via
//! backend/.env.example's local dev instance.

use http_body_util::BodyExt;
use tower::ServiceExt;

fn database_url() -> String {
    std::env::var("DATABASE_URL").expect(
        "DATABASE_URL must be set to run backend integration tests - see backend/.env.example",
    )
}

#[tokio::test]
async fn health_endpoint_reports_a_real_database_connection() {
    let db = vibessh_backend::connect_and_migrate(&database_url())
        .await
        .expect("connecting and migrating should succeed against a reachable database");
    let app = vibessh_backend::build_router(db);

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
async fn connect_and_migrate_is_idempotent_across_repeated_calls() {
    // Every process restart calls this again against the same database -
    // it must never fail just because migration 1..N already ran.
    vibessh_backend::connect_and_migrate(&database_url()).await.unwrap();
    vibessh_backend::connect_and_migrate(&database_url()).await.unwrap();
}
