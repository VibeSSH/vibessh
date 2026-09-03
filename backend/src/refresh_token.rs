//! Refresh token lifecycle - opaque random tokens, hashed at rest (see
//! migrations/0001 for why), rotated on every use: each `/auth/refresh`
//! call revokes the token it was given and issues a brand new one, rather
//! than reusing the same refresh token indefinitely. That bounds how long a
//! stolen-but-not-yet-used refresh token stays valid, and a token being
//! presented twice (the old one, after rotation already happened) is a
//! reasonable signal something is wrong - the token is simply rejected
//! either way since it's already revoked.
/// `(id, user_id, expires_at, revoked_at)` - the columns every lookup here
/// selects, named so the query below reads as a lookup rather than a tuple
/// puzzle.
type StoredRefreshToken = (Uuid, Uuid, DateTime<Utc>, Option<DateTime<Utc>>);

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::{ApiError, ApiResult};
use crate::tokens;

pub const REFRESH_TOKEN_TTL_DAYS: i64 = 30;

pub struct IssuedRefreshToken {
    pub raw: String,
    pub expires_at: DateTime<Utc>,
}

/// Inserts a brand new refresh token row and returns the raw value to hand
/// to the client - the raw value is never persisted, only its hash.
pub async fn issue(db: &PgPool, user_id: Uuid) -> ApiResult<IssuedRefreshToken> {
    let raw = tokens::generate();
    let now = Utc::now();
    let expires_at = now + Duration::days(REFRESH_TOKEN_TTL_DAYS);

    sqlx::query(
        "INSERT INTO refresh_tokens (id, user_id, token_hash, created_at, expires_at) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(tokens::hash(&raw))
    .bind(now)
    .bind(expires_at)
    .execute(db)
    .await?;

    Ok(IssuedRefreshToken { raw, expires_at })
}

/// Validates a presented refresh token (exists, not expired, not already
/// revoked), revokes it, and issues its replacement - all in one
/// transaction, so a crash mid-rotation can never leave the old token
/// revoked with no new one issued (which would strand the client) or both
/// tokens simultaneously valid.
pub async fn verify_and_rotate(db: &PgPool, raw_token: &str) -> ApiResult<(Uuid, IssuedRefreshToken)> {
    let hashed = tokens::hash(raw_token);
    let mut tx = db.begin().await?;

    let row: Option<StoredRefreshToken> = sqlx::query_as(
        "SELECT id, user_id, expires_at, revoked_at FROM refresh_tokens WHERE token_hash = $1",
    )
    .bind(&hashed)
    .fetch_optional(&mut *tx)
    .await?;

    let Some((token_id, user_id, expires_at, revoked_at)) = row else {
        return Err(ApiError::Unauthorized("refresh token is invalid".to_string()));
    };
    if revoked_at.is_some() {
        return Err(ApiError::Unauthorized("refresh token has already been used or revoked".to_string()));
    }
    if expires_at < Utc::now() {
        return Err(ApiError::Unauthorized("refresh token has expired".to_string()));
    }

    sqlx::query("UPDATE refresh_tokens SET revoked_at = $1 WHERE id = $2")
        .bind(Utc::now())
        .bind(token_id)
        .execute(&mut *tx)
        .await?;

    let raw = tokens::generate();
    let now = Utc::now();
    let new_expires_at = now + Duration::days(REFRESH_TOKEN_TTL_DAYS);
    sqlx::query(
        "INSERT INTO refresh_tokens (id, user_id, token_hash, created_at, expires_at) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(tokens::hash(&raw))
    .bind(now)
    .bind(new_expires_at)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok((user_id, IssuedRefreshToken { raw, expires_at: new_expires_at }))
}

/// Logout - revokes a refresh token without issuing a replacement. A token
/// that doesn't exist (already invalid, or never existed) is treated as
/// already-logged-out rather than an error, since the end state the caller
/// wants (this token no longer works) already holds either way.
pub async fn revoke(db: &PgPool, raw_token: &str) -> ApiResult<()> {
    sqlx::query("UPDATE refresh_tokens SET revoked_at = $1 WHERE token_hash = $2 AND revoked_at IS NULL")
        .bind(Utc::now())
        .bind(tokens::hash(raw_token))
        .execute(db)
        .await?;
    Ok(())
}

/// Ends every session for one account.
///
/// Used when a password changes: a provisioned password was known to
/// whoever created the account, so any session opened with it belongs to
/// them as much as to the owner, and leaving those alive would make the
/// change cosmetic. Already-revoked rows are left alone rather than
/// re-stamped, so the timestamp keeps saying when a session actually ended.
pub async fn revoke_all_for_user(db: &PgPool, user_id: Uuid) -> ApiResult<()> {
    sqlx::query("UPDATE refresh_tokens SET revoked_at = $1 WHERE user_id = $2 AND revoked_at IS NULL")
        .bind(Utc::now())
        .bind(user_id)
        .execute(db)
        .await?;
    Ok(())
}
