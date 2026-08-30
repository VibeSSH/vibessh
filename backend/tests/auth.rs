//! Real integration tests for register/login/refresh/logout/me, against a
//! real Postgres (see common::test_router). Every test uses a fresh random
//! email (common::unique_email) so re-running the suite against the same
//! persistent dev database never collides with a previous run's rows.
mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;

use common::{get_with_bearer, post, test_router, unique_email};

#[tokio::test]
async fn register_then_login_round_trips_the_same_account() {
    let email = unique_email();
    let (status, body) = post(
        test_router().await,
        "/auth/register",
        json!({ "email": email, "password": "correct horse battery staple", "displayName": "Test User" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["user"]["email"], email);
    assert!(body["accessToken"].as_str().is_some());
    assert!(body["refreshToken"].as_str().is_some());

    let (status, body) = post(test_router().await, "/auth/login", json!({ "email": email, "password": "correct horse battery staple" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["user"]["email"], email);
}

#[tokio::test]
async fn registering_the_same_email_twice_is_a_conflict_not_a_silent_overwrite() {
    let email = unique_email();
    let register = || json!({ "email": email, "password": "correct horse battery staple", "displayName": "Test User" });
    let (status, _) = post(test_router().await, "/auth/register", register()).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = post(test_router().await, "/auth/register", register()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
async fn a_weak_password_is_rejected_at_registration() {
    let (status, body) = post(
        test_router().await,
        "/auth/register",
        json!({ "email": unique_email(), "password": "short", "displayName": "Test User" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn an_invalid_email_is_rejected_at_registration() {
    let (status, body) = post(
        test_router().await,
        "/auth/register",
        json!({ "email": "not-an-email", "password": "correct horse battery staple", "displayName": "Test User" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn login_with_the_wrong_password_is_rejected_with_a_generic_message() {
    let email = unique_email();
    post(
        test_router().await,
        "/auth/register",
        json!({ "email": email, "password": "correct horse battery staple", "displayName": "Test User" }),
    )
    .await;

    let (status, body) = post(test_router().await, "/auth/login", json!({ "email": email, "password": "wrong password" })).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["message"], "invalid email or password");
}

#[tokio::test]
async fn login_with_a_nonexistent_email_gets_the_same_generic_message_as_a_wrong_password() {
    // Same response either way - a different message would let a caller
    // enumerate which emails have accounts.
    let (status, body) = post(test_router().await, "/auth/login", json!({ "email": unique_email(), "password": "correct horse battery staple" })).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["message"], "invalid email or password");
}

#[tokio::test]
async fn me_returns_the_authenticated_users_profile() {
    let (email, access_token) = common::register_user().await;

    let (status, body) = get_with_bearer(test_router().await, "/auth/me", &access_token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["email"], email);
}

#[tokio::test]
async fn me_without_a_token_is_unauthorized() {
    use tower::ServiceExt;
    let response = test_router().await.oneshot(Request::builder().uri("/auth/me").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn me_with_a_garbage_token_is_unauthorized_not_a_panic() {
    let (status, _) = get_with_bearer(test_router().await, "/auth/me", "not.a.real.jwt").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn refresh_rotates_the_token_and_invalidates_the_one_it_was_given() {
    let (_, register_body) = post(
        test_router().await,
        "/auth/register",
        json!({ "email": unique_email(), "password": "correct horse battery staple", "displayName": "Test User" }),
    )
    .await;
    let original_refresh_token = register_body["refreshToken"].as_str().unwrap().to_string();

    let (status, refreshed_body) = post(test_router().await, "/auth/refresh", json!({ "refreshToken": original_refresh_token })).await;
    assert_eq!(status, StatusCode::OK, "{refreshed_body}");
    let new_refresh_token = refreshed_body["refreshToken"].as_str().unwrap();
    assert_ne!(new_refresh_token, original_refresh_token, "rotation must issue a different token");

    // The original token was single-use - presenting it again must fail now.
    let (status, body) = post(test_router().await, "/auth/refresh", json!({ "refreshToken": original_refresh_token })).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
}

#[tokio::test]
async fn an_unknown_refresh_token_is_rejected() {
    let (status, _) = post(test_router().await, "/auth/refresh", json!({ "refreshToken": "not-a-real-token" })).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logout_revokes_the_refresh_token_so_it_can_no_longer_be_used() {
    let (_, register_body) = post(
        test_router().await,
        "/auth/register",
        json!({ "email": unique_email(), "password": "correct horse battery staple", "displayName": "Test User" }),
    )
    .await;
    let refresh_token = register_body["refreshToken"].as_str().unwrap().to_string();

    let (status, _) = post(test_router().await, "/auth/logout", json!({ "refreshToken": refresh_token })).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = post(test_router().await, "/auth/refresh", json!({ "refreshToken": refresh_token })).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
