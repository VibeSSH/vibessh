//! Shared by every integration test file - not a test binary itself (lives
//! in a `tests/<name>/` subdirectory, which cargo doesn't treat as its own
//! test target the way a direct `tests/*.rs` file is).
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

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

pub async fn post(app: Router, uri: &str, body: Value) -> (StatusCode, Value) {
    request(app, "POST", uri, Some(body), None).await
}

pub async fn delete(app: Router, uri: &str, token: &str) -> (StatusCode, Value) {
    request(app, "DELETE", uri, None, Some(token)).await
}

pub async fn get_with_bearer(app: Router, uri: &str, token: &str) -> (StatusCode, Value) {
    request(app, "GET", uri, None, Some(token)).await
}

pub async fn post_with_bearer(app: Router, uri: &str, token: &str, body: Value) -> (StatusCode, Value) {
    request(app, "POST", uri, Some(body), Some(token)).await
}

pub async fn patch(app: Router, uri: &str, token: &str, body: Value) -> (StatusCode, Value) {
    request(app, "PATCH", uri, Some(body), Some(token)).await
}

async fn request(app: Router, method: &str, uri: &str, body: Option<Value>, token: Option<&str>) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let request = if let Some(body) = body {
        builder.header("content-type", "application/json").body(Body::from(body.to_string())).unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    };

    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|err| {
            panic!("{method} {uri} returned a non-JSON body ({err}): {:?}", String::from_utf8_lossy(&bytes))
        })
    };
    (status, json)
}

/// Registers a brand new user and returns (email, accessToken) - the
/// starting point for most teams.rs tests, which all need at least one
/// authenticated user.
pub async fn register_user() -> (String, String) {
    let email = unique_email();
    let (status, body) = post(
        test_router().await,
        "/auth/register",
        json!({ "email": email, "password": "correct horse battery staple", "displayName": "Test User" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "test setup: register_user failed: {body}");
    (email, body["accessToken"].as_str().unwrap().to_string())
}
