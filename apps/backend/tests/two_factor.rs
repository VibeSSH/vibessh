//! Two-factor sign-in end to end, against a real Postgres - see `tests/auth.rs`
//! for why these are `#[ignore]` and how to run them.
mod common;

use axum::http::StatusCode;
use serde_json::json;

use common::{post, post_with_bearer, test_router, unique_email};
use vibessh_backend::two_factor;

const PASSWORD: &str = "correct horse battery staple";

fn ensure_encryption_key() {
    if std::env::var("TOTP_ENCRYPTION_KEY").is_err() {
        // A fixed test key: 32 bytes of 0x07, base64.
        std::env::set_var("TOTP_ENCRYPTION_KEY", "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=");
    }
}

#[tokio::test]
#[ignore]
async fn two_factor_guards_sign_in_from_setup_to_removal() {
    ensure_encryption_key();
    let email = unique_email();
    let (status, body) = post(test_router().await, "/auth/register", json!({ "email": email, "password": PASSWORD, "displayName": "Two Factor" })).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let token = body["accessToken"].as_str().unwrap().to_string();
    assert_eq!(body["user"]["twoFactorEnabled"], false);

    // Setup hands over a secret; a wrong code does not turn anything on.
    let (status, setup) = post_with_bearer(test_router().await, "/auth/2fa/setup", &token, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{setup}");
    let secret = two_factor::base32_decode(setup["secret"].as_str().unwrap()).unwrap();
    assert!(setup["otpauthUri"].as_str().unwrap().starts_with("otpauth://totp/"));
    let (status, _) = post_with_bearer(test_router().await, "/auth/2fa/enable", &token, json!({ "code": "000000" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let now = chrono::Utc::now().timestamp();
    let (status, enabled) = post_with_bearer(test_router().await, "/auth/2fa/enable", &token, json!({ "code": two_factor::code_at(&secret, now) })).await;
    assert_eq!(status, StatusCode::OK, "{enabled}");
    let recovery: Vec<String> = enabled["recoveryCodes"].as_array().unwrap().iter().map(|code| code.as_str().unwrap().to_string()).collect();
    assert_eq!(recovery.len(), two_factor::RECOVERY_CODE_COUNT);

    // The password alone is no longer enough, and a wrong code is refused.
    let (status, body) = post(test_router().await, "/auth/login", json!({ "email": email, "password": PASSWORD })).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "two_factor_required");
    let (_, body) = post(test_router().await, "/auth/login", json!({ "email": email, "password": PASSWORD, "totpCode": "000000" })).await;
    assert_eq!(body["code"], "two_factor_invalid");

    // The next step's code works once - the step used to enable is spent.
    let next = two_factor::code_at(&secret, now + 30);
    let (status, body) = post(test_router().await, "/auth/login", json!({ "email": email, "password": PASSWORD, "totpCode": next })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["user"]["twoFactorEnabled"], true);
    let (_, body) = post(test_router().await, "/auth/login", json!({ "email": email, "password": PASSWORD, "totpCode": next })).await;
    assert_eq!(body["code"], "two_factor_invalid", "a code is accepted once");

    // A recovery code works once, typed any way.
    let typed = recovery[0].to_uppercase().replace('-', " ");
    let (status, _) = post(test_router().await, "/auth/login", json!({ "email": email, "password": PASSWORD, "recoveryCode": typed })).await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = post(test_router().await, "/auth/login", json!({ "email": email, "password": PASSWORD, "recoveryCode": recovery[0] })).await;
    assert_eq!(body["code"], "two_factor_invalid", "a recovery code is used up");

    // Turning it off takes the password and a second factor.
    let (status, _) = post_with_bearer(test_router().await, "/auth/2fa/disable", &token, json!({ "password": PASSWORD })).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, body) =
        post_with_bearer(test_router().await, "/auth/2fa/disable", &token, json!({ "password": PASSWORD, "recoveryCode": recovery[1] })).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, _) = post(test_router().await, "/auth/login", json!({ "email": email, "password": PASSWORD })).await;
    assert_eq!(status, StatusCode::OK);
}
