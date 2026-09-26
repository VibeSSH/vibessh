//! Resetting a forgotten password, against a real Postgres.
//!
//! `#[ignore]` for the same reason as every other test here - see
//! `audit.rs`. Run them with
//! `DATABASE_URL=... cargo test -p vibessh-backend -- --ignored`.
//!
//! No SMTP server is involved: a code is put in the table the way the
//! request endpoint stores one - as the SHA-256 of the normalised code - and
//! the confirm endpoint is exercised against it.
mod common;

use axum::http::StatusCode;
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use common::{post, test_db, test_router, unique_email};

const PASSWORD: &str = "correct horse battery staple";
const NEW_PASSWORD: &str = "a different long passphrase";
const CODE: &str = "ABCDE-FGH23";

/// A fresh account; returns (email, user id, refresh token).
async fn account() -> (String, Uuid, String) {
    let email = unique_email();
    let (status, body) = post(test_router().await, "/auth/register", json!({ "email": email, "password": PASSWORD, "displayName": "Reset" })).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let user_id = Uuid::parse_str(body["user"]["id"].as_str().unwrap()).unwrap();
    (email, user_id, body["refreshToken"].as_str().unwrap().to_string())
}

/// Stores `code` as a pending reset for `user_id`, as the request endpoint would.
async fn pending_code(user_id: Uuid, code: &str) {
    let normalized: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect();
    let hash: String = Sha256::digest(normalized.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect();
    let now = chrono::Utc::now();
    sqlx::query("INSERT INTO password_resets (id, user_id, code_hash, created_at, expires_at) VALUES ($1, $2, $3, $4, $5)")
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(hash)
        .bind(now)
        .bind(now + chrono::Duration::minutes(30))
        .execute(&test_db().await)
        .await
        .unwrap();
}

async fn confirm(email: &str, code: &str, new_password: &str) -> (StatusCode, serde_json::Value) {
    post(test_router().await, "/auth/password-reset/confirm", json!({ "email": email, "code": code, "newPassword": new_password })).await
}

async fn login(email: &str, password: &str) -> StatusCode {
    post(test_router().await, "/auth/login", json!({ "email": email, "password": password })).await.0
}

/// The whole reset: the code, typed any old way, sets the new password once,
/// the old password stops working, and every session opened with it ends.
#[tokio::test]
#[ignore]
async fn a_code_sets_a_new_password_once_and_ends_every_session() {
    let (email, user_id, refresh_token) = account().await;
    pending_code(user_id, CODE).await;

    let (status, body) = confirm(&email, "abcde fgh23", NEW_PASSWORD).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_eq!(login(&email, NEW_PASSWORD).await, StatusCode::OK);
    assert_eq!(login(&email, PASSWORD).await, StatusCode::UNAUTHORIZED);

    let (status, _) = post(test_router().await, "/auth/refresh", json!({ "refreshToken": refresh_token })).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "a session from before the reset has to end");

    // Spent: the same code a second time is refused.
    let (status, body) = confirm(&email, CODE, "yet another long passphrase").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "reset_code_invalid");
}

/// Five wrong guesses and the code stops working, even when the sixth try is
/// the right one - so its length, not only the rate limiter, is what a
/// guesser is up against.
#[tokio::test]
#[ignore]
async fn a_code_stops_working_after_five_wrong_guesses() {
    let (email, user_id, _) = account().await;
    pending_code(user_id, CODE).await;

    for guess in ["AAAAA-AAAAA", "BBBBB-BBBBB", "CCCCC-CCCCC", "DDDDD-DDDDD", "EEEEE-EEEEE"] {
        let (status, body) = confirm(&email, guess, NEW_PASSWORD).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["code"], "reset_code_invalid");
    }
    let (status, body) = confirm(&email, CODE, NEW_PASSWORD).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(login(&email, PASSWORD).await, StatusCode::OK, "the password must be unchanged");
}

/// An address with no account gets the same refusal as a wrong code - the
/// answer says nothing about which addresses are registered.
#[tokio::test]
#[ignore]
async fn an_unknown_address_is_refused_the_same_way_as_a_wrong_code() {
    let (status, body) = confirm(&unique_email(), CODE, NEW_PASSWORD).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "reset_code_invalid");
}

/// Without SMTP configured - as in CI - asking for a code is refused with a
/// reason, the same for every address, rather than accepted and never sent.
#[tokio::test]
#[ignore]
async fn without_email_configured_a_reset_is_refused_with_a_reason() {
    if std::env::var("SMTP_HOST").is_ok() {
        return;
    }
    let (email, _, _) = account().await;
    for address in [email, unique_email()] {
        let (status, body) = post(test_router().await, "/auth/password-reset/request", json!({ "email": address, "language": "pl" })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["code"], "password_reset_unavailable");
    }
}
