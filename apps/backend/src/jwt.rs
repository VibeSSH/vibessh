//! Short-lived JWT access tokens. Deliberately stateless - unlike refresh
//! tokens, an access token is never looked up or revoked server-side; its
//! short lifetime (see ACCESS_TOKEN_TTL) is the whole revocation story,
//! which is the standard access/refresh split (a stolen access token is
//! only useful until it expires; a stolen refresh token is the one that
//! actually needs to be revocable, see refresh_token.rs).
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const ACCESS_TOKEN_TTL_SECONDS: i64 = 15 * 60;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// Subject - the user id.
    pub sub: Uuid,
    pub exp: i64,
    pub iat: i64,
}

pub fn issue_access_token(user_id: Uuid, secret: &[u8]) -> Result<(String, i64), String> {
    let now = Utc::now();
    let expires_at = now + Duration::seconds(ACCESS_TOKEN_TTL_SECONDS);
    let claims = Claims { sub: user_id, exp: expires_at.timestamp(), iat: now.timestamp() };
    let token = encode(&Header::default(), &claims, &EncodingKey::from_secret(secret))
        .map_err(|err| format!("failed to sign access token: {err}"))?;
    Ok((token, expires_at.timestamp()))
}

pub fn verify_access_token(token: &str, secret: &[u8]) -> Result<Claims, String> {
    decode::<Claims>(token, &DecodingKey::from_secret(secret), &Validation::default())
        .map(|data| data.claims)
        .map_err(|err| format!("invalid access token: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_freshly_issued_token_verifies_and_carries_the_right_subject() {
        let user_id = Uuid::new_v4();
        let (token, _expires_at) = issue_access_token(user_id, b"test-secret").unwrap();
        let claims = verify_access_token(&token, b"test-secret").unwrap();
        assert_eq!(claims.sub, user_id);
    }

    #[test]
    fn a_token_signed_with_a_different_secret_is_rejected() {
        let (token, _) = issue_access_token(Uuid::new_v4(), b"secret-a").unwrap();
        assert!(verify_access_token(&token, b"secret-b").is_err());
    }

    #[test]
    fn a_malformed_token_is_rejected_not_a_panic() {
        assert!(verify_access_token("not.a.jwt", b"test-secret").is_err());
    }
}
