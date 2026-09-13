//! The public half of each device's SSH key, and which account a team member
//! gets on a shared Node.
//!
//! **Why a backend holds these at all**, when it deliberately holds no
//! credentials: a public key is not a credential. It is the line an
//! `authorized_keys` file holds in plain text on every server there is.
//! Publishing one lets an install that can already reach a shared Node give
//! a teammate their own account on it, without any secret passing through
//! here - see `docs/planning/team-access-design.md`.
//!
//! **Why the format is validated rather than trusted.** What is stored here
//! is written verbatim into an `authorized_keys` file on somebody else's
//! server. That file is line-oriented and its first field can carry options
//! - `command=`, `from=`, and others that change what a key is allowed to
//! do. A value with a newline in it is two lines, and a value starting with
//! options is a key with powers nobody granted. So only a plain
//! `<type> <base64> [comment]` is accepted.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::errors::{ApiError, ApiResult, Detail};
use crate::models::{DeviceKey, MemberAccess, PublishDeviceKeyRequest};
use crate::teams::team_for_member;
use crate::AppState;

const MAX_LABEL_LEN: usize = 60;
const MAX_KEY_LEN: usize = 4096;

/// The key types worth accepting.
///
/// Ed25519 is what the desktop generates. RSA is here because somebody will
/// bring an existing key, and refusing it would push them towards pasting a
/// private key somewhere instead. DSA is absent on purpose - it is
/// deprecated and OpenSSH has refused it by default for years.
const KEY_TYPES: [&str; 4] = ["ssh-ed25519", "ssh-rsa", "ecdsa-sha2-nistp256", "sk-ssh-ed25519@openssh.com"];

/// The Linux account a member is given on a Node.
///
/// Derived from the user id rather than stored, matching
/// `dedicated_user::username`'s "derive it, don't store it" convention -
/// there is then nothing that can disagree with itself. Well inside
/// `useradd`'s 32-character limit: a 10-character prefix and 12 hex digits.
pub fn node_username(user_id: Uuid) -> String {
    format!("vibessh-m-{}", &user_id.simple().to_string()[..12])
}

/// Accepts only a plain `<type> <base64> [comment]` line.
///
/// Rejects, specifically: anything with a newline or carriage return, which
/// would become a second line in `authorized_keys`; anything beginning with
/// options rather than a key type, which would grant powers nobody asked
/// for; and a base64 field with characters outside the alphabet, which is
/// the cheapest way to catch a value that is not a key at all.
fn validate_public_key(value: &str) -> ApiResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ApiError::InvalidInput(Detail::new("device_key_empty", "the public key cannot be empty")));
    }
    if trimmed.len() > MAX_KEY_LEN {
        return Err(ApiError::InvalidInput(
            Detail::new("device_key_too_long", format!("the public key must be at most {MAX_KEY_LEN} characters")).with("max", MAX_KEY_LEN),
        ));
    }
    if trimmed.contains('\n') || trimmed.contains('\r') {
        return Err(ApiError::InvalidInput(Detail::new(
            "device_key_multiline",
            "a public key is one line - this one has a line break in it",
        )));
    }

    let mut fields = trimmed.split_whitespace();
    let key_type = fields.next().unwrap_or_default();
    if !KEY_TYPES.contains(&key_type) {
        return Err(ApiError::InvalidInput(
            Detail::new("device_key_unsupported_type", format!("'{key_type}' is not a supported key type")).with("type", key_type),
        ));
    }
    let body = fields.next().unwrap_or_default();
    // Decoded rather than pattern-matched. A character-class check passes
    // anything spellable in the base64 alphabet - `not` is a valid base64
    // word and is obviously not a key, which a test caught here rather than
    // a user catching it on a Node. An SSH public key's blob begins with its
    // own type as a length-prefixed string, so decoding and comparing that
    // to the type in front of it is both cheap and the actual format.
    let Ok(decoded) = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, body) else {
        return Err(ApiError::InvalidInput(Detail::new("device_key_malformed", "that does not look like a public key")));
    };
    if !blob_declares(&decoded, key_type) {
        return Err(ApiError::InvalidInput(Detail::new("device_key_malformed", "that does not look like a public key")));
    }
    Ok(trimmed.to_string())
}

/// Whether an SSH public-key blob begins with `expected` as its own type.
///
/// The wire format starts with a 32-bit big-endian length followed by that
/// many bytes of the algorithm name - the same name that appears in front of
/// the base64 in the file. A blob that disagrees with its own label is not a
/// key somebody typed slightly wrong; it is something else entirely.
fn blob_declares(decoded: &[u8], expected: &str) -> bool {
    let Some(length_bytes) = decoded.get(..4) else { return false };
    let length = u32::from_be_bytes([length_bytes[0], length_bytes[1], length_bytes[2], length_bytes[3]]) as usize;
    // A sane algorithm name, and one that fits - both guard against a length
    // taken from arbitrary bytes.
    if length == 0 || length > 64 {
        return false;
    }
    decoded.get(4..4 + length).map(|name| name == expected.as_bytes()).unwrap_or(false)
}

fn validate_label(value: &str) -> ApiResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ApiError::InvalidInput(Detail::new("device_label_empty", "the device name cannot be empty")));
    }
    if trimmed.chars().count() > MAX_LABEL_LEN {
        return Err(ApiError::InvalidInput(
            Detail::new("device_label_too_long", format!("the device name must be at most {MAX_LABEL_LEN} characters")).with("max", MAX_LABEL_LEN),
        ));
    }
    Ok(trimmed.to_string())
}

/// Registers this device's public key, or refreshes it if already known.
///
/// Idempotent by (user, key): an install republishing on every start says
/// "still mine" without creating a second row, which is what would otherwise
/// happen every time somebody reinstalled the app.
pub async fn publish(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Json(body): Json<PublishDeviceKeyRequest>,
) -> ApiResult<impl IntoResponse> {
    let public_key = validate_public_key(&body.public_key)?;
    let label = validate_label(&body.label)?;
    let now = Utc::now();

    let key: DeviceKey = sqlx::query_as(
        "INSERT INTO device_keys (id, user_id, public_key, label, created_at, seen_at) \
         VALUES ($1, $2, $3, $4, $5, $5) \
         ON CONFLICT (user_id, public_key) DO UPDATE SET seen_at = $5, label = $4 \
         RETURNING id, user_id, public_key, label, created_at",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(&public_key)
    .bind(&label)
    .bind(now)
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::OK, Json(key)))
}

/// This account's own devices, so somebody can see and revoke them.
pub async fn list_mine(State(state): State<AppState>, AuthUser(user_id): AuthUser) -> ApiResult<Json<Vec<DeviceKey>>> {
    let keys: Vec<DeviceKey> =
        sqlx::query_as("SELECT id, user_id, public_key, label, created_at FROM device_keys WHERE user_id = $1 ORDER BY created_at")
            .bind(user_id)
            .fetch_all(&state.db)
            .await?;
    Ok(Json(keys))
}

/// Removes one device, which is how somebody revokes a laptop they no longer
/// have.
///
/// This removes the key from the team's view. It does **not** remove it from
/// any Node it was already installed on - that needs an install with access
/// to each Node, and pretending otherwise would be the worst possible thing
/// to be wrong about. It comes off at the next access sync of each Node,
/// which writes `authorized_keys` whole from the keys still published here;
/// until one runs, the key is still in that file.
pub async fn revoke(State(state): State<AppState>, AuthUser(user_id): AuthUser, Path(key_id): Path<Uuid>) -> ApiResult<impl IntoResponse> {
    let deleted = sqlx::query("DELETE FROM device_keys WHERE id = $1 AND user_id = $2")
        .bind(key_id)
        .bind(user_id)
        .execute(&state.db)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::NotFound(Detail::new("device_key_not_found", "that device is not on this account")));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Every member of a team, the account they get on a Node, the keys that
/// should be in it, and what that account is allowed to do.
///
/// Team membership is the only requirement to read this, because everything
/// in it is public by nature and any member needs it to provision access on
/// a Node they can already reach. Permission *keys* are in the same
/// category: a member can already read the team's roles, and what they say
/// is the point of saying it.
pub async fn list_team_access(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(team_id): Path<Uuid>,
) -> ApiResult<Json<Vec<MemberAccess>>> {
    team_for_member(&state.db, team_id, user_id).await?;

    let rows: Vec<(Uuid, String, String, Option<String>)> = sqlx::query_as(
        "SELECT u.id, u.email, u.display_name, k.public_key \
         FROM team_members m \
         JOIN users u ON u.id = m.user_id \
         LEFT JOIN device_keys k ON k.user_id = u.id \
         WHERE m.team_id = $1 \
         ORDER BY u.email, k.created_at",
    )
    .bind(team_id)
    .fetch_all(&state.db)
    .await?;

    // One query for every member's permissions rather than one per member -
    // a team of thirty would otherwise be thirty round trips to build one
    // list.
    let permission_rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT DISTINCT mr.user_id, rp.permission_key \
         FROM member_roles mr \
         JOIN role_permissions rp ON rp.role_id = mr.role_id \
         WHERE mr.team_id = $1 \
         ORDER BY mr.user_id, rp.permission_key",
    )
    .bind(team_id)
    .fetch_all(&state.db)
    .await?;
    let mut permissions_by_user: std::collections::HashMap<Uuid, Vec<String>> = std::collections::HashMap::new();
    for (user_id, key) in permission_rows {
        permissions_by_user.entry(user_id).or_default().push(key);
    }

    let mut members: Vec<MemberAccess> = Vec::new();
    for (id, email, display_name, public_key) in rows {
        // The join produces one row per key, and a member with no key at all
        // still appears - with an empty list, which is the honest answer to
        // "what should this account hold" and the signal the interface needs
        // to say that person has not set up a device yet.
        if members.last().map(|member| member.user_id) != Some(id) {
            members.push(MemberAccess {
                user_id: id,
                email,
                display_name,
                node_username: node_username(id),
                public_keys: Vec::new(),
                permissions: permissions_by_user.remove(&id).unwrap_or_default(),
            });
        }
        if let Some(key) = public_key {
            if let Some(member) = members.last_mut() {
                member.public_keys.push(key);
            }
        }
    }
    Ok(Json(members))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_key_line_is_accepted() {
        // A real key, generated for this test. The fixture used to be a
        // plausible-looking string, which passed while the check was a
        // character class and stopped passing the moment it decoded the
        // blob - a fixture that is not an example of the thing proves
        // nothing about the thing.
        let key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICz0PKtIFpwteAQuKKR6Efa20YOAhyRrvRXT3qkVb7he laptop@example";
        assert_eq!(validate_public_key(key).unwrap(), key);
    }

    /// The one that matters. `authorized_keys` is line-oriented, so a value
    /// with a newline in it is two entries - and the second is whatever the
    /// submitter wanted.
    #[test]
    fn a_key_with_a_line_break_is_refused() {
        let err = validate_public_key("ssh-ed25519 AAAAC3Nz x\nssh-rsa AAAAB3 attacker").unwrap_err();
        assert!(format!("{err}").contains("line break"), "{err}");
    }

    /// An `authorized_keys` line may begin with options that change what the
    /// key can do. Accepting one here would let somebody grant themselves
    /// powers on a Node nobody offered.
    #[test]
    fn a_key_line_beginning_with_options_is_refused() {
        assert!(validate_public_key("command=\"/bin/sh\" ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICz0PKtIFpwteAQuKKR6Efa20YOAhyRrvRXT3qkVb7he laptop@example").is_err());
        assert!(validate_public_key("from=\"0.0.0.0/0\",no-pty ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICz0PKtIFpwteAQuKKR6Efa20YOAhyRrvRXT3qkVb7he laptop@example").is_err());
    }

    #[test]
    fn a_value_that_is_not_a_key_is_refused() {
        assert!(validate_public_key("").is_err());
        assert!(validate_public_key("ssh-ed25519").is_err());
        assert!(validate_public_key("ssh-ed25519 not-base64!").is_err());
        // Spellable in the base64 alphabet and still not a key - the case
        // that made a character-class check insufficient.
        assert!(validate_public_key("ssh-ed25519 not").is_err());
        // Valid base64 whose blob declares a different type than the label
        // in front of it.
        assert!(validate_public_key("ssh-ed25519 AAAAB3NzaC1yc2E=").is_err());
        assert!(validate_public_key("ssh-dss AAAAB3NzaC1kc3M x").is_err(), "DSA is deprecated and must not be accepted");
    }

    /// Derived, so it cannot drift from whatever was stored - and short
    /// enough for `useradd`, which the Application account convention also
    /// had to respect.
    #[test]
    fn the_node_username_is_derived_and_short_enough() {
        let id = Uuid::new_v4();
        let name = node_username(id);
        assert_eq!(name, node_username(id), "the same user must always get the same account");
        assert!(name.len() <= 32, "{name} is too long for useradd");
        assert!(name.starts_with("vibessh-m-"), "{name}");
    }
}
