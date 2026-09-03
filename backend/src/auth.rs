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

use crate::errors::{ApiError, ApiResult};
use crate::models::{AuthResponse, ChangePasswordRequest, LoginRequest, RefreshRequest, RegisterRequest, User, UserProfile};
use crate::{jwt, password, refresh_token, AppState};

const MAX_DISPLAY_NAME_LEN: usize = 100;

pub(crate) fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

pub(crate) fn validate_email(email: &str) -> ApiResult<()> {
    let Some((local, domain)) = email.split_once('@') else {
        return Err(ApiError::InvalidInput("email must contain @".to_string()));
    };
    if local.is_empty() || domain.is_empty() || !domain.contains('.') || email.contains(char::is_whitespace) {
        return Err(ApiError::InvalidInput("email is not a valid address".to_string()));
    }
    Ok(())
}

fn validate_password(password: &str) -> ApiResult<()> {
    if password.chars().count() < password::MIN_PASSWORD_LEN {
        return Err(ApiError::InvalidInput(format!("password must be at least {} characters", password::MIN_PASSWORD_LEN)));
    }
    if password.len() > password::MAX_PASSWORD_LEN {
        return Err(ApiError::InvalidInput(format!("password must be at most {} characters", password::MAX_PASSWORD_LEN)));
    }
    Ok(())
}

pub(crate) fn validate_display_name(name: &str) -> ApiResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ApiError::InvalidInput("display name cannot be empty".to_string()));
    }
    if trimmed.chars().count() > MAX_DISPLAY_NAME_LEN {
        return Err(ApiError::InvalidInput(format!("display name must be at most {MAX_DISPLAY_NAME_LEN} characters")));
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

pub async fn register(State(state): State<AppState>, Json(body): Json<RegisterRequest>) -> ApiResult<impl IntoResponse> {
    let email = normalize_email(&body.email);
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
            return Err(ApiError::Conflict("an account with this email already exists".to_string()));
        }
    }
    insert?;

    // Self-registered: the person chose this password themselves, so
    // there is nothing to force them to replace.
    let user = User { id: user_id, email, password_hash, display_name, created_at: now, must_change_password: false };
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

pub async fn login(State(state): State<AppState>, Json(body): Json<LoginRequest>) -> ApiResult<Json<AuthResponse>> {
    let email = normalize_email(&body.email);

    // A real password is never anywhere near this long - reject before
    // spending an Argon2 computation on it, the same DoS concern
    // MAX_PASSWORD_LEN exists for on the register path.
    if body.password.len() > password::MAX_PASSWORD_LEN {
        return Err(ApiError::Unauthorized(INVALID_CREDENTIALS.to_string()));
    }

    let user: Option<User> = sqlx::query_as(
        "SELECT id, email, password_hash, display_name, created_at, must_change_password FROM users WHERE email = $1",
    )
    .bind(&email)
    .fetch_optional(&state.db)
    .await?;

    let Some(user) = user else {
        // Still runs a real Argon2 verification against a fixed hash, so a
        // nonexistent email doesn't respond measurably faster than a wrong
        // password for a real one.
        password::verify_password(&body.password, dummy_hash_for_timing_safety());
        return Err(ApiError::Unauthorized(INVALID_CREDENTIALS.to_string()));
    };

    if !password::verify_password(&body.password, &user.password_hash) {
        return Err(ApiError::Unauthorized(INVALID_CREDENTIALS.to_string()));
    }

    Ok(Json(issue_auth_response(&state, &user).await?))
}

pub async fn refresh(State(state): State<AppState>, Json(body): Json<RefreshRequest>) -> ApiResult<Json<AuthResponse>> {
    let (user_id, new_refresh) = refresh_token::verify_and_rotate(&state.db, &body.refresh_token).await?;

    let user: Option<User> = sqlx::query_as("SELECT id, email, password_hash, display_name, created_at, must_change_password FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?;
    let user = user.ok_or_else(|| ApiError::Unauthorized("account no longer exists".to_string()))?;

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
        .ok_or_else(|| ApiError::Unauthorized("missing Authorization header".to_string()))?;
    let token = header
        .strip_prefix("Bearer ")
        .ok_or_else(|| ApiError::Unauthorized("Authorization header must be a Bearer token".to_string()))?;
    let claims = jwt::verify_access_token(token, &state.jwt_secret).map_err(ApiError::Unauthorized)?;
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
            None => Err(ApiError::Unauthorized("account no longer exists".to_string())),
            Some(true) => Err(ApiError::PasswordChangeRequired(
                "set a new password before using this account - the one it has was set by somebody else".to_string(),
            )),
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
        return Err(ApiError::Unauthorized(INVALID_CREDENTIALS.to_string()));
    }
    validate_password(&body.new_password)?;

    let user: Option<User> =
        sqlx::query_as("SELECT id, email, password_hash, display_name, created_at, must_change_password FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;
    let user = user.ok_or_else(|| ApiError::NotFound("account no longer exists".to_string()))?;

    if !password::verify_password(&body.current_password, &user.password_hash) {
        return Err(ApiError::Unauthorized(INVALID_CREDENTIALS.to_string()));
    }
    // Refusing this is not pedantry: a provisioned account whose owner
    // "changes" the password to the one they were given has changed nothing,
    // and the person who issued it still knows it.
    if password::verify_password(&body.new_password, &user.password_hash) {
        return Err(ApiError::InvalidInput("the new password must be different from the current one".to_string()));
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
    let user: Option<User> = sqlx::query_as("SELECT id, email, password_hash, display_name, created_at, must_change_password FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?;
    let user = user.ok_or_else(|| ApiError::NotFound("account no longer exists".to_string()))?;
    Ok(Json(UserProfile::from(&user)))
}
