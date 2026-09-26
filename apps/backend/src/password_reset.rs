//! Resetting a forgotten password with a code sent by email.
//!
//! **A code, not a link.** VibeSSH is a desktop app; a link would need a web
//! page to land on and a way back into the app. The person types the code
//! into the same sign-in window they asked from, with the new password.
//!
//! **Nothing tells a caller whether an address has an account.** Asking for
//! a code answers the same way whether or not one exists, and the email is
//! sent after the answer, so the time it takes does not say either.
//!
//! **What a code is worth.** Ten characters from a 32-character alphabet,
//! about 50 bits; it works once, for thirty minutes, and stops accepting
//! guesses after five wrong ones - on top of the sign-in rate limit, which
//! both endpoints count against. A new request retires the older codes.
//!
//! **What a reset does.** It replaces the password and ends every session,
//! the way changing it does - anybody who was signed in with the old one is
//! signed out. It does not turn two-step verification off: the password is
//! the factor that was forgotten, and the authenticator app still guards the
//! account.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{Duration, Utc};
use rand::Rng;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::errors::{ApiError, ApiResult, Detail};
use crate::{auth, mail, password, refresh_token, AppState};

/// How long a code works.
const CODE_LIFETIME_MINUTES: i64 = 30;
/// Wrong guesses one code survives.
const MAX_ATTEMPTS: i32 = 5;
/// No 0/O or 1/I, so a code read off a phone is typed back correctly.
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const CODE_LENGTH: usize = 10;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordResetRequest {
    pub email: String,
    /// The language of the email - `pl` or `en`. Anything else gets Polish.
    #[serde(default)]
    pub language: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordResetConfirm {
    pub email: String,
    pub code: String,
    pub new_password: String,
}

/// A fresh code, as the person sees it: `ABCDE-FGH23`.
fn generate_code() -> String {
    let mut rng = rand::rngs::OsRng;
    let raw: String = (0..CODE_LENGTH).map(|_| CODE_ALPHABET[rng.gen_range(0..CODE_ALPHABET.len())] as char).collect();
    format!("{}-{}", &raw[..5], &raw[5..])
}

/// The code as typed, reduced to what was generated: upper case, without
/// the dash or any spaces somebody pasted along with it.
fn normalize_code(code: &str) -> String {
    code.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect()
}

fn hash_code(normalized: &str) -> String {
    let digest = Sha256::digest(normalized.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn invalid_code() -> ApiError {
    ApiError::InvalidInput(Detail::new("reset_code_invalid", "that code is wrong, used or expired - ask for a new one"))
}

/// Sends a reset code to an address, if it has an account.
///
/// Always `202 Accepted` for a well-formed request, account or not - see the
/// module doc. The email goes out after the response, from a task of its
/// own; a delivery failure is logged, since there is nobody left to tell.
pub async fn request(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    peer: Option<axum::extract::ConnectInfo<std::net::SocketAddr>>,
    Json(body): Json<PasswordResetRequest>,
) -> ApiResult<impl IntoResponse> {
    let email = auth::normalize_email(&body.email);
    auth::check_rate_limit(&state, &headers, peer.map(|info| info.0), &email)?;
    // Said before anything else, and the same for every address: whether
    // this server can send email is not a fact about any account.
    let Some(mailer) = mail::mailer() else {
        return Err(ApiError::InvalidInput(Detail::new(
            "password_reset_unavailable",
            "this server can't send email yet, so a password can't be reset by email",
        )));
    };

    let user_id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE email = $1").bind(&email).fetch_optional(&state.db).await?;
    if let Some(user_id) = user_id {
        let code = generate_code();
        let now = Utc::now();
        let mut tx = state.db.begin().await?;
        // Only the newest code in the inbox works.
        sqlx::query("UPDATE password_resets SET used_at = $2 WHERE user_id = $1 AND used_at IS NULL")
            .bind(user_id)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO password_resets (id, user_id, code_hash, created_at, expires_at) VALUES ($1, $2, $3, $4, $5)")
            .bind(Uuid::new_v4())
            .bind(user_id)
            .bind(hash_code(&normalize_code(&code)))
            .bind(now)
            .bind(now + Duration::minutes(CODE_LIFETIME_MINUTES))
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;

        let message = mail::password_reset_message(&code, body.language.trim());
        tokio::spawn(async move {
            if let Err(err) = mailer.send(&email, &message).await {
                log::warn!("couldn't send a password reset code: {err}");
            }
        });
    }
    Ok(StatusCode::ACCEPTED)
}

/// Sets a new password with a code from the email.
///
/// Every refusal - no such account, no code, a wrong, used or expired one,
/// too many guesses - is the same `reset_code_invalid`, for the reason the
/// request endpoint answers the same way for every address.
pub async fn confirm(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    peer: Option<axum::extract::ConnectInfo<std::net::SocketAddr>>,
    Json(body): Json<PasswordResetConfirm>,
) -> ApiResult<impl IntoResponse> {
    let email = auth::normalize_email(&body.email);
    auth::check_rate_limit(&state, &headers, peer.map(|info| info.0), &email)?;
    auth::validate_password(&body.new_password)?;

    let user_id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE email = $1").bind(&email).fetch_optional(&state.db).await?;
    let Some(user_id) = user_id else { return Err(invalid_code()) };

    let pending: Option<(Uuid, String, i32)> = sqlx::query_as(
        "SELECT id, code_hash, attempts FROM password_resets \
         WHERE user_id = $1 AND used_at IS NULL AND expires_at > $2 \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(user_id)
    .bind(Utc::now())
    .fetch_optional(&state.db)
    .await?;
    let Some((reset_id, code_hash, attempts)) = pending else { return Err(invalid_code()) };
    if attempts >= MAX_ATTEMPTS {
        return Err(invalid_code());
    }
    if hash_code(&normalize_code(&body.code)) != code_hash {
        sqlx::query("UPDATE password_resets SET attempts = attempts + 1 WHERE id = $1").bind(reset_id).execute(&state.db).await?;
        return Err(invalid_code());
    }

    let new_hash = password::hash_password(&body.new_password).map_err(ApiError::Internal)?;
    let now = Utc::now();
    let mut tx = state.db.begin().await?;
    // Spent in the statement that checks it is unspent, so two requests with
    // the same code cannot both get through.
    let spent = sqlx::query("UPDATE password_resets SET used_at = $2 WHERE id = $1 AND used_at IS NULL")
        .bind(reset_id)
        .bind(now)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if spent != 1 {
        return Err(invalid_code());
    }
    sqlx::query("UPDATE users SET password_hash = $1, must_change_password = FALSE, updated_at = $2 WHERE id = $3")
        .bind(&new_hash)
        .bind(now)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    refresh_token::revoke_all_for_user(&state.db, user_id).await?;
    log::info!("a password was reset by email for account {user_id}");
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_is_ten_unambiguous_characters_in_two_groups() {
        for _ in 0..200 {
            let code = generate_code();
            assert_eq!(code.len(), 11, "{code}");
            assert_eq!(&code[5..6], "-", "{code}");
            let normalized = normalize_code(&code);
            assert_eq!(normalized.len(), CODE_LENGTH);
            assert!(normalized.bytes().all(|byte| CODE_ALPHABET.contains(&byte)), "{code}");
            for confusable in ['0', 'O', '1', 'I'] {
                assert!(!normalized.contains(confusable), "{code}");
            }
        }
    }

    /// Typed back from an email: lower case, without the dash, with a stray
    /// space - all the same code.
    #[test]
    fn a_code_is_accepted_however_it_was_typed() {
        let hash = hash_code(&normalize_code("ABCDE-FGH23"));
        for typed in ["abcde-fgh23", "ABCDEFGH23", " abcde fgh23 ", "ABCDE-FGH23\n"] {
            assert_eq!(hash_code(&normalize_code(typed)), hash, "{typed:?}");
        }
        assert_ne!(hash_code(&normalize_code("ABCDE-FGH24")), hash);
    }

    #[test]
    fn the_email_is_in_the_language_asked_for_and_carries_the_code() {
        let english = mail::password_reset_message("ABCDE-FGH23", "en");
        assert!(english.subject.contains("password reset"));
        for body in [&english.text, &english.html] {
            assert!(body.contains("ABCDE-FGH23") && body.contains("30 minutes"), "{body}");
        }
        let polish = mail::password_reset_message("ABCDE-FGH23", "pl");
        assert!(polish.subject.contains("hasła"));
        for body in [&polish.text, &polish.html] {
            assert!(body.contains("ABCDE-FGH23") && body.contains("30 minut"), "{body}");
        }
        assert!(polish.html.contains(r#"lang="pl""#) && english.html.contains(r#"lang="en""#));
        // Anything else is Polish rather than nothing.
        assert_eq!(mail::password_reset_message("X", "de").subject, polish.subject);
    }

    /// Written out for looking at, when asked: `VIBESSH_WRITE_EMAIL_PREVIEW=<dir>`.
    #[test]
    fn preview_the_reset_email() {
        let Ok(dir) = std::env::var("VIBESSH_WRITE_EMAIL_PREVIEW") else { return };
        for language in ["pl", "en"] {
            let email = mail::password_reset_message("ABCDE-FGH23", language);
            std::fs::write(format!("{dir}/reset-{language}.html"), email.html).unwrap();
        }
    }
}
