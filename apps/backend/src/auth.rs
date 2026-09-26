//! Register/login/refresh/logout/me - the accounts stage of the production
//! roadmap's P0 foundation. Nothing here is Team/Role-aware yet (that's the
//! next stage); this is purely "does this person have a valid VibeSSH
//! account and a token proving it."
use std::sync::OnceLock;

use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use uuid::Uuid;

use crate::errors::{ApiError, ApiResult, Detail};
use crate::models::{
    AuthResponse, ChangePasswordRequest, LoginRequest, RefreshRequest, RegisterRequest, TwoFactorDisableRequest, TwoFactorEnableRequest,
    TwoFactorEnabledResponse, TwoFactorSetupResponse, User, UserProfile,
};
use crate::{jwt, password, refresh_token, two_factor, AppState};

/// Every column `User` reads, in one place - the four queries that load a
/// user must agree, and `two_factor_enabled` is computed rather than stored.
const USER_COLUMNS: &str =
    "id, email, password_hash, display_name, created_at, must_change_password, (totp_enabled_at IS NOT NULL) AS two_factor_enabled";

const MAX_DISPLAY_NAME_LEN: usize = 100;

pub(crate) fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

pub(crate) fn validate_email(email: &str) -> ApiResult<()> {
    let Some((local, domain)) = email.split_once('@') else {
        return Err(ApiError::InvalidInput(Detail::new("email_missing_at", "email must contain @")));
    };
    if local.is_empty() || domain.is_empty() || !domain.contains('.') || email.contains(char::is_whitespace) {
        return Err(ApiError::InvalidInput(Detail::new("email_invalid", "email is not a valid address")));
    }
    Ok(())
}

pub(crate) fn validate_password(password: &str) -> ApiResult<()> {
    if password.chars().count() < password::MIN_PASSWORD_LEN {
        return Err(ApiError::InvalidInput(Detail::new("password_too_short", format!("password must be at least {} characters", password::MIN_PASSWORD_LEN)).with("min", password::MIN_PASSWORD_LEN)));
    }
    if password.len() > password::MAX_PASSWORD_LEN {
        return Err(ApiError::InvalidInput(Detail::new("password_too_long", format!("password must be at most {} characters", password::MAX_PASSWORD_LEN)).with("max", password::MAX_PASSWORD_LEN)));
    }
    Ok(())
}

pub(crate) fn validate_display_name(name: &str) -> ApiResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ApiError::InvalidInput(Detail::new("display_name_empty", "display name cannot be empty")));
    }
    if trimmed.chars().count() > MAX_DISPLAY_NAME_LEN {
        return Err(ApiError::InvalidInput(Detail::new("display_name_too_long", format!("display name must be at most {MAX_DISPLAY_NAME_LEN} characters")).with("max", MAX_DISPLAY_NAME_LEN)));
    }
    Ok(trimmed.to_string())
}

async fn issue_auth_response(state: &AppState, user: &User) -> ApiResult<AuthResponse> {
    let (access_token, access_token_expires_at) =
        jwt::issue_access_token(user.id, &state.jwt_secret).map_err(ApiError::Internal)?;
    let refresh = refresh_token::issue(&state.db, user.id).await?;
    Ok(AuthResponse {
        user: UserProfile::from(user),
        access_token,
        access_token_expires_at,
        refresh_token: refresh.raw,
    })
}


/// Refuses a caller who has tried too often, before any expensive work.
///
/// Both keys are checked and both are counted. The account key catches a
/// password list aimed at one email; the address key catches one password
/// tried against many emails, which no per-account counter would see. See
/// `rate_limit` for why the address key is derived the way it is.
pub(crate) fn check_rate_limit(state: &AppState, headers: &axum::http::HeaderMap, peer: Option<std::net::SocketAddr>, email: &str) -> ApiResult<()> {
    let forwarded = headers.get("cf-connecting-ip").or_else(|| headers.get("x-forwarded-for")).and_then(|value| value.to_str().ok());
    let address_key = crate::rate_limit::client_key(forwarded, peer.map(|socket| socket.ip()));
    let account_key = format!("account:{email}");
    for key in [address_key, account_key] {
        if let crate::rate_limit::Decision::RetryAfter(seconds) = state.rate_limiter.check(&key) {
            return Err(ApiError::TooManyRequests(
                Detail::new("too_many_attempts", format!("too many attempts - try again in {seconds} seconds")).with("seconds", seconds),
            ));
        }
    }
    Ok(())
}

pub async fn register(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    peer: Option<axum::extract::ConnectInfo<std::net::SocketAddr>>,
    Json(body): Json<RegisterRequest>,
) -> ApiResult<impl IntoResponse> {
    let email = normalize_email(&body.email);
    check_rate_limit(&state, &headers, peer.map(|info| info.0), &email)?;
    validate_email(&email)?;
    validate_password(&body.password)?;
    let display_name = validate_display_name(&body.display_name)?;

    let password_hash = password::hash_password(&body.password).map_err(ApiError::Internal)?;
    let now = chrono::Utc::now();
    let user_id = Uuid::new_v4();

    let insert = sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $5)",
    )
    .bind(user_id)
    .bind(&email)
    .bind(&password_hash)
    .bind(&display_name)
    .bind(now)
    .execute(&state.db)
    .await;

    if let Err(sqlx::Error::Database(db_err)) = &insert {
        if db_err.is_unique_violation() {
            return Err(ApiError::Conflict(Detail::new("email_taken", "an account with this email already exists")));
        }
    }
    insert?;

    // Self-registered: the person chose this password themselves, so
    // there is nothing to force them to replace.
    let user = User { id: user_id, email, password_hash, display_name, created_at: now, must_change_password: false, two_factor_enabled: false };
    let response = issue_auth_response(&state, &user).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

/// A generic "invalid email or password" for every failure path - never
/// tells the caller which half was wrong, or even whether the account
/// exists, since either would let an attacker enumerate registered emails.
const INVALID_CREDENTIALS: &str = "invalid email or password";

fn dummy_hash_for_timing_safety() -> &'static str {
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY.get_or_init(|| password::hash_password("vibessh-timing-safety-dummy").expect("hashing a fixed string never fails"))
}

pub async fn login(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    peer: Option<axum::extract::ConnectInfo<std::net::SocketAddr>>,
    Json(body): Json<LoginRequest>,
) -> ApiResult<Json<AuthResponse>> {
    let email = normalize_email(&body.email);
    // Before the Argon2 verification below, which is the expensive thing
    // this is protecting as much as the account is.
    check_rate_limit(&state, &headers, peer.map(|info| info.0), &email)?;

    // A real password is never anywhere near this long - reject before
    // spending an Argon2 computation on it, the same DoS concern
    // MAX_PASSWORD_LEN exists for on the register path.
    if body.password.len() > password::MAX_PASSWORD_LEN {
        return Err(ApiError::Unauthorized(Detail::new("invalid_credentials", INVALID_CREDENTIALS)));
    }

    let user: Option<User> = sqlx::query_as(
        &format!("SELECT {USER_COLUMNS} FROM users WHERE email = $1"),
    )
    .bind(&email)
    .fetch_optional(&state.db)
    .await?;

    let Some(user) = user else {
        // Still runs a real Argon2 verification against a fixed hash, so a
        // nonexistent email doesn't respond measurably faster than a wrong
        // password for a real one.
        password::verify_password(&body.password, dummy_hash_for_timing_safety());
        return Err(ApiError::Unauthorized(Detail::new("invalid_credentials", INVALID_CREDENTIALS)));
    };

    if !password::verify_password(&body.password, &user.password_hash) {
        return Err(ApiError::Unauthorized(Detail::new("invalid_credentials", INVALID_CREDENTIALS)));
    }

    // Only after the password: asking for a code tells the caller the
    // password was right, so it must not be said to somebody who got it wrong.
    if user.two_factor_enabled {
        check_second_factor(&state, user.id, body.totp_code.as_deref(), body.recovery_code.as_deref()).await?;
    }

    Ok(Json(issue_auth_response(&state, &user).await?))
}

/// The second factor for an account that has one: a code from the app, or a
/// recovery code, each accepted once.
///
/// Neither given is `two_factor_required` - the client's cue to ask for one
/// and send the sign-in again with it. The replay guard is enforced in the
/// same statement that records the use, so two requests racing with one code
/// cannot both succeed.
async fn check_second_factor(state: &AppState, user_id: Uuid, totp_code: Option<&str>, recovery_code: Option<&str>) -> ApiResult<()> {
    let invalid = || ApiError::Unauthorized(Detail::new("two_factor_invalid", "that code isn't right - check the time on your phone, or use a recovery code"));
    let (stored, last_step): (Option<Vec<u8>>, Option<i64>) =
        sqlx::query_as("SELECT totp_secret, totp_last_step FROM users WHERE id = $1").bind(user_id).fetch_one(&state.db).await?;
    let Some(stored) = stored else {
        return Ok(());
    };

    if let Some(code) = totp_code.map(str::trim).filter(|code| !code.is_empty()) {
        let cipher = two_factor::cipher().ok_or_else(two_factor_unavailable)?;
        let secret = two_factor::decrypt(cipher, user_id, &stored).map_err(ApiError::Internal)?;
        let step = two_factor::verify(&secret, code, chrono::Utc::now().timestamp(), last_step).ok_or_else(invalid)?;
        let recorded = sqlx::query("UPDATE users SET totp_last_step = $1 WHERE id = $2 AND (totp_last_step IS NULL OR totp_last_step < $1)")
            .bind(step)
            .bind(user_id)
            .execute(&state.db)
            .await?;
        return if recorded.rows_affected() == 1 { Ok(()) } else { Err(invalid()) };
    }

    if let Some(code) = recovery_code.map(two_factor::normalize_recovery_code).filter(|code| !code.is_empty()) {
        let used = sqlx::query("UPDATE totp_recovery_codes SET used_at = $1 WHERE user_id = $2 AND code_hash = $3 AND used_at IS NULL")
            .bind(chrono::Utc::now())
            .bind(user_id)
            .bind(crate::tokens::hash(&code))
            .execute(&state.db)
            .await?;
        return if used.rows_affected() == 1 { Ok(()) } else { Err(invalid()) };
    }

    Err(ApiError::Unauthorized(Detail::new("two_factor_required", "enter the code from your authenticator app")))
}

fn two_factor_unavailable() -> ApiError {
    ApiError::InvalidInput(Detail::new(
        "two_factor_unavailable",
        "two-factor sign-in isn't available on this server - its TOTP_ENCRYPTION_KEY is not set",
    ))
}

/// Starts turning two-factor on: a new secret, kept as pending until a code
/// from the app confirms it (`two_factor_enable`). Starting again replaces a
/// pending one, so a setup abandoned half-way costs nothing.
pub async fn two_factor_setup(State(state): State<AppState>, AuthUser(user_id): AuthUser) -> ApiResult<Json<TwoFactorSetupResponse>> {
    let cipher = two_factor::cipher().ok_or_else(two_factor_unavailable)?;
    let user: User = sqlx::query_as(&format!("SELECT {USER_COLUMNS} FROM users WHERE id = $1")).bind(user_id).fetch_one(&state.db).await?;
    if user.two_factor_enabled {
        return Err(ApiError::Conflict(Detail::new("two_factor_already_enabled", "two-factor sign-in is already on for this account")));
    }
    let secret = two_factor::generate_secret();
    let encrypted = two_factor::encrypt(cipher, user_id, &secret).map_err(ApiError::Internal)?;
    sqlx::query("UPDATE users SET totp_pending_secret = $1 WHERE id = $2").bind(encrypted).bind(user_id).execute(&state.db).await?;
    Ok(Json(TwoFactorSetupResponse { secret: two_factor::base32(&secret), otpauth_uri: two_factor::otpauth_uri(&user.email, &secret) }))
}

/// Confirms the pending secret with a code from the app and turns two-factor
/// on, returning the recovery codes - the only time they are ever shown.
pub async fn two_factor_enable(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Json(body): Json<TwoFactorEnableRequest>,
) -> ApiResult<Json<TwoFactorEnabledResponse>> {
    let cipher = two_factor::cipher().ok_or_else(two_factor_unavailable)?;
    let pending: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT totp_pending_secret FROM users WHERE id = $1").bind(user_id).fetch_one(&state.db).await?;
    let pending = pending.ok_or_else(|| ApiError::InvalidInput(Detail::new("two_factor_setup_missing", "start the two-factor setup again")))?;
    let secret = two_factor::decrypt(cipher, user_id, &pending).map_err(ApiError::Internal)?;
    let step = two_factor::verify(&secret, &body.code, chrono::Utc::now().timestamp(), None).ok_or_else(|| {
        ApiError::InvalidInput(Detail::new("two_factor_invalid", "that code isn't right - check the time on your phone, or use a recovery code"))
    })?;

    let codes = two_factor::generate_recovery_codes();
    let mut tx = state.db.begin().await?;
    sqlx::query(
        "UPDATE users SET totp_secret = totp_pending_secret, totp_pending_secret = NULL, totp_enabled_at = $1, totp_last_step = $2 WHERE id = $3",
    )
    .bind(chrono::Utc::now())
    .bind(step)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM totp_recovery_codes WHERE user_id = $1").bind(user_id).execute(&mut *tx).await?;
    for code in &codes {
        sqlx::query("INSERT INTO totp_recovery_codes (user_id, code_hash) VALUES ($1, $2)")
            .bind(user_id)
            .bind(crate::tokens::hash(&two_factor::normalize_recovery_code(code)))
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(TwoFactorEnabledResponse { recovery_codes: codes }))
}

/// Turns two-factor off. Takes the password and a current second factor, so
/// a stolen session alone cannot remove the protection that would have kept
/// its thief out.
pub async fn two_factor_disable(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    peer: Option<axum::extract::ConnectInfo<std::net::SocketAddr>>,
    AuthUser(user_id): AuthUser,
    Json(body): Json<TwoFactorDisableRequest>,
) -> ApiResult<StatusCode> {
    let user: User = sqlx::query_as(&format!("SELECT {USER_COLUMNS} FROM users WHERE id = $1")).bind(user_id).fetch_one(&state.db).await?;
    check_rate_limit(&state, &headers, peer.map(|info| info.0), &user.email)?;
    if body.password.len() > password::MAX_PASSWORD_LEN || !password::verify_password(&body.password, &user.password_hash) {
        return Err(ApiError::Unauthorized(Detail::new("invalid_credentials", INVALID_CREDENTIALS)));
    }
    if !user.two_factor_enabled {
        return Err(ApiError::Conflict(Detail::new("two_factor_not_enabled", "two-factor sign-in isn't on for this account")));
    }
    check_second_factor(&state, user_id, body.totp_code.as_deref(), body.recovery_code.as_deref()).await?;

    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE users SET totp_secret = NULL, totp_pending_secret = NULL, totp_enabled_at = NULL, totp_last_step = NULL WHERE id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM totp_recovery_codes WHERE user_id = $1").bind(user_id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn refresh(State(state): State<AppState>, Json(body): Json<RefreshRequest>) -> ApiResult<Json<AuthResponse>> {
    let (user_id, new_refresh) = refresh_token::verify_and_rotate(&state.db, &body.refresh_token).await?;

    let user: Option<User> = sqlx::query_as(&format!("SELECT {USER_COLUMNS} FROM users WHERE id = $1"))
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?;
    let user = user.ok_or_else(|| ApiError::Unauthorized(Detail::new("account_gone", "account no longer exists")))?;

    let (access_token, access_token_expires_at) =
        jwt::issue_access_token(user.id, &state.jwt_secret).map_err(ApiError::Internal)?;

    Ok(Json(AuthResponse {
        user: UserProfile::from(&user),
        access_token,
        access_token_expires_at,
        refresh_token: new_refresh.raw,
    }))
}

pub async fn logout(State(state): State<AppState>, Json(body): Json<RefreshRequest>) -> ApiResult<StatusCode> {
    refresh_token::revoke(&state.db, &body.refresh_token).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Extracts and verifies the caller's identity from `Authorization: Bearer
/// <access token>` - any handler that takes this as an argument is
/// automatically rejected with 401 before the handler body runs at all if
/// the token is missing, malformed, wrongly signed, or expired.
/// An authenticated caller whose account is fully usable.
///
/// Every endpoint takes this one except the two that must keep working
/// while an account still holds a password somebody else set for it - see
/// `AnyAuthUser`.
pub struct AuthUser(pub Uuid);

/// An authenticated caller in any state, including one who has not yet
/// replaced a provisioned password.
///
/// Exactly two handlers take this: reading your own profile, and setting a
/// new password. Anything else would defeat the point of the flag.
pub struct AnyAuthUser(pub Uuid);

/// The shared half: prove the bearer token is real and get the subject out.
fn user_id_from_parts(parts: &Parts, state: &AppState) -> Result<Uuid, ApiError> {
    let header = parts
        .headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::Unauthorized(Detail::new("missing_authorization_header", "missing Authorization header")))?;
    let token = header
        .strip_prefix("Bearer ")
        .ok_or_else(|| ApiError::Unauthorized(Detail::new("malformed_authorization_header", "Authorization header must be a Bearer token")))?;
    // One code for every way a token fails to verify - expired, tampered
    // with, signed with a rotated secret. The client's response is the same
    // in all three: authenticate again.
    let claims = jwt::verify_access_token(token, &state.jwt_secret)
        .map_err(|reason| ApiError::Unauthorized(Detail::new("access_token_invalid", reason)))?;
    Ok(claims.sub)
}

#[async_trait::async_trait]
impl FromRequestParts<AppState> for AnyAuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        Ok(AnyAuthUser(user_id_from_parts(parts, state)?))
    }
}

#[async_trait::async_trait]
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let user_id = user_id_from_parts(parts, state)?;

        // Read on every authenticated request, deliberately.
        //
        // The alternative is carrying the flag in the token, which is one
        // fewer query and wrong in the way that matters: a token issued
        // before the change would keep asserting the old answer until it
        // expired, so somebody who had just set a new password would go on
        // being refused. Asking the database means the refusal stops the
        // instant the reason does.
        let must_change: Option<bool> =
            sqlx::query_scalar("SELECT must_change_password FROM users WHERE id = $1").bind(user_id).fetch_optional(&state.db).await?;

        match must_change {
            // A token for an account that no longer exists.
            None => Err(ApiError::Unauthorized(Detail::new("account_gone", "account no longer exists"))),
            Some(true) => Err(ApiError::PasswordChangeRequired(Detail::new(
                "password_change_required",
                "set a new password before using this account - the one it has was set by somebody else",
            ))),
            Some(false) => Ok(AuthUser(user_id)),
        }
    }
}

/// Replaces your own password, and clears the "somebody else set this"
/// flag if it was set.
///
/// Takes `AnyAuthUser`, because an account in exactly that state is the one
/// that most needs to call it.
///
/// Every other session is ended. A provisioned password was known to
/// whoever created the account, so any session opened with it belongs to
/// them as much as to the owner - leaving those alive would make the change
/// cosmetic. The caller keeps working: a fresh pair is issued and returned.
pub async fn change_password(
    State(state): State<AppState>,
    AnyAuthUser(user_id): AnyAuthUser,
    Json(body): Json<ChangePasswordRequest>,
) -> ApiResult<Json<AuthResponse>> {
    if body.current_password.len() > password::MAX_PASSWORD_LEN {
        return Err(ApiError::Unauthorized(Detail::new("invalid_credentials", INVALID_CREDENTIALS)));
    }
    validate_password(&body.new_password)?;

    let user: Option<User> =
        sqlx::query_as(&format!("SELECT {USER_COLUMNS} FROM users WHERE id = $1"))
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;
    let user = user.ok_or_else(|| ApiError::NotFound(Detail::new("account_gone", "account no longer exists")))?;

    if !password::verify_password(&body.current_password, &user.password_hash) {
        return Err(ApiError::Unauthorized(Detail::new("invalid_credentials", INVALID_CREDENTIALS)));
    }
    // Refusing this is not pedantry: a provisioned account whose owner
    // "changes" the password to the one they were given has changed nothing,
    // and the person who issued it still knows it.
    if password::verify_password(&body.new_password, &user.password_hash) {
        return Err(ApiError::InvalidInput(Detail::new("password_unchanged", "the new password must be different from the current one")));
    }

    let new_hash = password::hash_password(&body.new_password).map_err(ApiError::Internal)?;
    sqlx::query("UPDATE users SET password_hash = $1, must_change_password = FALSE, updated_at = $2 WHERE id = $3")
        .bind(&new_hash)
        .bind(chrono::Utc::now())
        .bind(user_id)
        .execute(&state.db)
        .await?;

    refresh_token::revoke_all_for_user(&state.db, user_id).await?;

    let updated = User { password_hash: new_hash, must_change_password: false, ..user };
    Ok(Json(issue_auth_response(&state, &updated).await?))
}

pub async fn me(State(state): State<AppState>, AuthUser(user_id): AuthUser) -> ApiResult<Json<UserProfile>> {
    let user: Option<User> = sqlx::query_as(&format!("SELECT {USER_COLUMNS} FROM users WHERE id = $1"))
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?;
    let user = user.ok_or_else(|| ApiError::NotFound(Detail::new("account_gone", "account no longer exists")))?;
    Ok(Json(UserProfile::from(&user)))
}
