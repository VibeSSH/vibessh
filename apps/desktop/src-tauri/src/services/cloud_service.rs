//! Session orchestration on top of `cloud_client::CloudClient` - the "make
//! sure we have a live access token before doing anything authenticated"
//! logic lives here, not in the client (which only knows how to make one
//! HTTP call) or in the commands (which stay thin, same split as every
//! other module in this crate).
use chrono::Utc;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{
    CloudAuditEvent, CloudProvisionedMember, CloudRole, CloudRoleWithPermissions,
    CloudServer, CloudSessionInfo,
    CloudTeam, CloudTeamMember, CloudTwoFactorEnabled, CloudTwoFactorSetup, CloudUserProfile,
};
use crate::state::cloud_session::{CloudSession, CloudState};
use crate::storage::credentials;

const REFRESH_SKEW_SECONDS: i64 = 30;

pub async fn register(state: &CloudState, email: &str, password: &str, display_name: &str) -> AppResult<CloudUserProfile> {
    let mut inner = state.inner.lock().await;
    let auth = inner.client.register(email, password, display_name).await?;
    credentials::store_cloud_refresh_token(&auth.refresh_token)?;
    let user = auth.user.clone();
    inner.session =
        Some(CloudSession { access_token: auth.access_token, access_token_expires_at: auth.access_token_expires_at, user: auth.user });
    Ok(user)
}

pub async fn login(state: &CloudState, email: &str, password: &str, totp_code: Option<&str>, recovery_code: Option<&str>) -> AppResult<CloudUserProfile> {
    let mut inner = state.inner.lock().await;
    let auth = inner.client.login(email, password, totp_code, recovery_code).await?;
    credentials::store_cloud_refresh_token(&auth.refresh_token)?;
    let user = auth.user.clone();
    inner.session =
        Some(CloudSession { access_token: auth.access_token, access_token_expires_at: auth.access_token_expires_at, user: auth.user });
    Ok(user)
}

pub async fn logout(state: &CloudState) -> AppResult<()> {
    let mut inner = state.inner.lock().await;
    if let Some(refresh_token) = credentials::load_cloud_refresh_token()? {
        // Best-effort - if the backend is unreachable the local session is
        // still cleared below, so the user is signed out of *this device*
        // either way. The refresh token stays valid server-side until it
        // expires on its own in that case, same trade-off any "log out
        // while offline" flow has.
        let _ = inner.client.logout(&refresh_token).await;
    }
    credentials::delete_cloud_refresh_token()?;
    inner.session = None;
    Ok(())
}

pub async fn session_info(state: &CloudState) -> Option<CloudSessionInfo> {
    state.session_info().await
}

/// Whether a failed refresh means the stored credential is genuinely dead.
///
/// A refusal from the backend arrives as `AppError::Cloud` once the backend
/// names its own error codes, so matching only on `Unauthorized` would have
/// stopped recognising a revoked token - the credential would have been kept
/// for ever and every start would have failed the same way.
fn refresh_was_rejected(err: &AppError) -> bool {
    match err {
        AppError::Unauthorized(_) => true,
        AppError::Cloud { kind, .. } => kind == "unauthorized",
        _ => false,
    }
}

/// Returns a definitely-not-expired access token, refreshing it first if
/// necessary - every authenticated call below goes through this rather
/// than reading `session.access_token` directly.
async fn ensure_valid_access_token(state: &CloudState) -> AppResult<String> {
    let mut inner = state.inner.lock().await;

    if let Some(session) = &inner.session {
        if session.access_token_expires_at > Utc::now().timestamp() + REFRESH_SKEW_SECONDS {
            return Ok(session.access_token.clone());
        }
    }

    let refresh_token = credentials::load_cloud_refresh_token()?
        .ok_or_else(|| AppError::Unauthorized("not signed in to the VibeSSH cloud backend".to_string()))?;
    let auth = inner.client.refresh(&refresh_token).await.inspect_err(|err| {
        // Cleared only when the backend actually rejected it - revoked, or
        // expired. A refresh that failed because the backend could not be
        // reached says nothing about whether the token is still good, and
        // deleting it there turns a moment of downtime into a permanent
        // sign-out: the app starts, cannot reach the backend, throws away a
        // perfectly valid credential, and the user has to log in again with
        // no idea why. That is exactly what happened every time the backend
        // was restarted while the app was starting.
        if refresh_was_rejected(err) {
            let _ = credentials::delete_cloud_refresh_token();
        }
    })?;
    credentials::store_cloud_refresh_token(&auth.refresh_token)?;
    let access_token = auth.access_token.clone();
    inner.session =
        Some(CloudSession { access_token: auth.access_token, access_token_expires_at: auth.access_token_expires_at, user: auth.user });
    Ok(access_token)
}

/// Called once at app startup - if a refresh token is already sitting in
/// the OS keyring from a previous run, this turns it back into a live
/// session so the user doesn't have to log in again every launch. Silent,
/// not an error, if there's nothing stored or it no longer works: "not
/// signed in yet" is the normal state for most of this app's history so
/// far, not a failure.
pub async fn try_restore_session(state: &CloudState) {
    let _ = ensure_valid_access_token(state).await;
}

/// The backend address this device is configured to talk to, and a
/// currently-valid access token for it - refreshed here if it was close
/// to expiry.
///
/// Exists so `services::ai_service` can build the hosted AI provider
/// without reaching into `CloudState` itself. It returns owned values
/// rather than a borrow because the provider outlives this call: it is
/// moved into the task that runs the turn.
pub async fn cloud_ai_endpoint(state: &CloudState) -> AppResult<(String, String)> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    Ok((inner.client.base_url().to_string(), token))
}

pub async fn list_team_applications(state: &CloudState, team_id: uuid::Uuid) -> AppResult<Vec<crate::models::CloudApplication>> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.list_team_applications(&token, team_id).await
}

#[allow(clippy::too_many_arguments)]
pub async fn push_team_application(
    state: &CloudState,
    team_id: uuid::Uuid,
    local_id: uuid::Uuid,
    team_server_id: Option<uuid::Uuid>,
    name: &str,
    blueprint_id: &str,
    runtime_type: &str,
    working_directory: &str,
    ports: &[crate::models::CloudApplicationPort],
    environment: &[crate::models::CloudApplicationEnvironment],
) -> AppResult<crate::models::CloudApplication> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner
        .client
        .push_team_application(&token, team_id, local_id, team_server_id, name, blueprint_id, runtime_type, working_directory, ports, environment)
        .await
}

pub async fn remove_team_application(state: &CloudState, team_id: uuid::Uuid, application_id: uuid::Uuid) -> AppResult<()> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.remove_team_application(&token, team_id, application_id).await
}

pub async fn list_application_members(
    state: &CloudState,
    team_id: uuid::Uuid,
    application_id: uuid::Uuid,
) -> AppResult<Vec<crate::models::CloudApplicationMember>> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.list_application_members(&token, team_id, application_id).await
}

pub async fn add_application_member(
    state: &CloudState,
    team_id: uuid::Uuid,
    application_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> AppResult<()> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.add_application_member(&token, team_id, application_id, user_id).await
}

pub async fn remove_application_member(
    state: &CloudState,
    team_id: uuid::Uuid,
    application_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> AppResult<()> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.remove_application_member(&token, team_id, application_id, user_id).await
}

/// Registers this device's public key with the backend.
pub async fn publish_device_key(state: &CloudState, public_key: &str, label: &str) -> AppResult<crate::models::CloudDeviceKey> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.publish_device_key(&token, public_key, label).await
}

pub async fn list_device_keys(state: &CloudState) -> AppResult<Vec<crate::models::CloudDeviceKey>> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.list_device_keys(&token).await
}

pub async fn revoke_device_key(state: &CloudState, key_id: uuid::Uuid) -> AppResult<()> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.revoke_device_key(&token, key_id).await
}

/// Everyone in the team, with the account and keys each should have on a
/// Node.
pub async fn list_team_access(state: &CloudState, team_id: uuid::Uuid) -> AppResult<Vec<crate::models::CloudMemberAccess>> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.list_team_access(&token, team_id).await
}

pub async fn list_teams(state: &CloudState) -> AppResult<Vec<CloudTeam>> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.list_teams(&token).await
}

pub async fn create_team(state: &CloudState, name: &str) -> AppResult<CloudTeam> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.create_team(&token, name).await
}

pub async fn get_team(state: &CloudState, team_id: Uuid) -> AppResult<CloudTeam> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.get_team(&token, team_id).await
}

pub async fn list_members(state: &CloudState, team_id: Uuid) -> AppResult<Vec<CloudTeamMember>> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.list_members(&token, team_id).await
}

pub async fn list_permissions(state: &CloudState) -> AppResult<Vec<String>> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.list_permissions(&token).await
}

pub async fn list_roles(state: &CloudState, team_id: Uuid) -> AppResult<Vec<CloudRoleWithPermissions>> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.list_roles(&token, team_id).await
}

#[allow(clippy::too_many_arguments)]
pub async fn create_role(
    state: &CloudState,
    team_id: Uuid,
    name: &str,
    description: Option<&str>,
    permissions: &[String],
) -> AppResult<CloudRoleWithPermissions> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.create_role(&token, team_id, name, description, permissions).await
}

#[allow(clippy::too_many_arguments)]
pub async fn update_role(
    state: &CloudState,
    team_id: Uuid,
    role_id: Uuid,
    name: &str,
    description: Option<&str>,
    permissions: &[String],
) -> AppResult<CloudRoleWithPermissions> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.update_role(&token, team_id, role_id, name, description, permissions).await
}

pub async fn delete_role(state: &CloudState, team_id: Uuid, role_id: Uuid) -> AppResult<()> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.delete_role(&token, team_id, role_id).await
}

pub async fn list_member_roles(state: &CloudState, team_id: Uuid, user_id: Uuid) -> AppResult<Vec<CloudRole>> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.list_member_roles(&token, team_id, user_id).await
}

pub async fn assign_role(state: &CloudState, team_id: Uuid, user_id: Uuid, role_id: Uuid) -> AppResult<()> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.assign_role(&token, team_id, user_id, role_id).await
}

pub async fn unassign_role(state: &CloudState, team_id: Uuid, user_id: Uuid, role_id: Uuid) -> AppResult<()> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.unassign_role(&token, team_id, user_id, role_id).await
}

pub async fn list_servers(state: &CloudState, team_id: Uuid) -> AppResult<Vec<CloudServer>> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.list_servers(&token, team_id).await
}

pub async fn create_server(
    state: &CloudState,
    team_id: Uuid,
    name: &str,
    host: &str,
    ssh_port: i32,
    username: Option<&str>,
) -> AppResult<CloudServer> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.create_server(&token, team_id, name, host, ssh_port, username).await
}

pub async fn list_pending_revocations(state: &CloudState, team_id: Uuid) -> AppResult<Vec<crate::models::CloudNodeRevocation>> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.list_pending_revocations(&token, team_id).await
}

pub async fn complete_revocation(state: &CloudState, team_id: Uuid, revocation_id: Uuid) -> AppResult<()> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.complete_revocation(&token, team_id, revocation_id).await
}

pub async fn delete_server(state: &CloudState, team_id: Uuid, server_id: Uuid) -> AppResult<()> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.delete_server(&token, team_id, server_id).await
}

pub async fn my_permissions(state: &CloudState, team_id: Uuid) -> AppResult<Vec<String>> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.my_permissions(&token, team_id).await
}

pub async fn remove_member(state: &CloudState, team_id: Uuid, user_id: Uuid) -> AppResult<()> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.remove_member(&token, team_id, user_id).await
}

pub async fn delete_team(state: &CloudState, team_id: Uuid) -> AppResult<()> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.delete_team(&token, team_id).await
}

/// Creates an account for somebody and puts them in the team.
///
/// The returned password is the only copy that will ever exist outside an
/// Argon2 hash, so it is passed straight back to the caller and this layer
/// keeps none of it.
pub async fn provision_member(
    state: &CloudState,
    team_id: Uuid,
    email: &str,
    display_name: Option<&str>,
    role_id: Option<Uuid>,
) -> AppResult<CloudProvisionedMember> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.provision_member(&token, team_id, email, display_name, role_id).await
}

/// Replaces the signed-in account's own password and adopts the session the
/// backend issues in return.
///
/// Storing the new session is not optional bookkeeping: the change ends
/// every previous session, so the tokens this app is holding stop working
/// the moment the call succeeds. Without this the user would be signed out
/// by their own password change.
pub async fn change_password(state: &CloudState, current_password: &str, new_password: &str) -> AppResult<CloudUserProfile> {
    let token = ensure_valid_access_token(state).await?;
    let response = {
        let inner = state.inner.lock().await;
        inner.client.change_password(&token, current_password, new_password).await?
    };
    // Same shape as `login`: keep the refresh token where the keyring
    // expects it and adopt the access token, or this app is holding a
    // session the backend has just ended.
    let user = response.user.clone();
    credentials::store_cloud_refresh_token(&response.refresh_token)?;
    let mut inner = state.inner.lock().await;
    inner.session = Some(CloudSession {
        access_token: response.access_token,
        access_token_expires_at: response.access_token_expires_at,
        user: response.user,
    });
    Ok(user)
}

/// Starts turning two-factor on, and draws the QR code for the link.
pub async fn two_factor_setup(state: &CloudState) -> AppResult<CloudTwoFactorSetup> {
    let token = ensure_valid_access_token(state).await?;
    let mut setup = {
        let inner = state.inner.lock().await;
        inner.client.two_factor_setup(&token).await?
    };
    let code = qrcode::QrCode::new(setup.otpauth_uri.as_bytes()).map_err(|err| AppError::Internal(format!("couldn't draw the QR code: {err}")))?;
    setup.qr_svg = code
        .render::<qrcode::render::svg::Color<'_>>()
        .min_dimensions(200, 200)
        .dark_color(qrcode::render::svg::Color("#000000"))
        .light_color(qrcode::render::svg::Color("#ffffff"))
        .build();
    Ok(setup)
}

/// Confirms the setup with a code and returns the recovery codes. The
/// session's copy of the profile is refreshed, so the account screen shows
/// two-factor as on without signing in again.
pub async fn two_factor_enable(state: &CloudState, code: &str) -> AppResult<CloudTwoFactorEnabled> {
    let token = ensure_valid_access_token(state).await?;
    let enabled = {
        let inner = state.inner.lock().await;
        inner.client.two_factor_enable(&token, code).await?
    };
    refresh_profile(state, &token).await;
    Ok(enabled)
}

pub async fn two_factor_disable(state: &CloudState, password: &str, totp_code: Option<&str>, recovery_code: Option<&str>) -> AppResult<CloudUserProfile> {
    let token = ensure_valid_access_token(state).await?;
    {
        let inner = state.inner.lock().await;
        inner.client.two_factor_disable(&token, password, totp_code, recovery_code).await?;
    }
    refresh_profile(state, &token).await;
    let inner = state.inner.lock().await;
    inner.session.as_ref().map(|session| session.user.clone()).ok_or_else(|| AppError::Internal("the session ended while turning two-factor off".into()))
}

/// Re-reads the profile into the session after something changed it. A
/// failure leaves the old copy - the change itself already succeeded.
async fn refresh_profile(state: &CloudState, token: &str) {
    let mut inner = state.inner.lock().await;
    match inner.client.me(token).await {
        Ok(user) => {
            if let Some(session) = inner.session.as_mut() {
                session.user = user;
            }
        }
        Err(err) => log::warn!("couldn't refresh the account profile after a change: {err}"),
    }
}

pub async fn list_audit_events(state: &CloudState, team_id: Uuid, limit: i64, offset: i64) -> AppResult<Vec<CloudAuditEvent>> {
    let token = ensure_valid_access_token(state).await?;
    let inner = state.inner.lock().await;
    inner.client.list_audit_events(&token, team_id, limit, offset).await
}

#[cfg(test)]
mod session_persistence_tests {
    use super::refresh_was_rejected;
    use crate::errors::AppError;

    /// Which refresh failures are allowed to destroy the stored credential.
    ///
    /// Mirrors the condition in `ensure_valid_access_token`. It is a test
    /// about a policy rather than about code shape: the failure it guards
    /// against - a backend being briefly unreachable silently logging
    /// somebody out for good - is invisible until it happens to a user, and
    /// it happened repeatedly while the backend was being restarted.
    fn should_forget_token(err: &AppError) -> bool {
        refresh_was_rejected(err)
    }

    #[test]
    fn a_rejected_token_is_forgotten() {
        assert!(should_forget_token(&AppError::Unauthorized("refresh token revoked".into())));
        // The shape a real rejection now arrives in, since the backend
        // names its own codes.
        assert!(should_forget_token(&AppError::Cloud {
            kind: "unauthorized".into(),
            code: "refresh_token_used".into(),
            params: serde_json::Value::Null,
            message: "refresh token has already been used or revoked".into(),
        }));
        // A refusal that re-authenticating cannot lift is not a dead token.
        assert!(!should_forget_token(&AppError::Cloud {
            kind: "invalid_input".into(),
            code: "password_unchanged".into(),
            params: serde_json::Value::Null,
            message: "the new password must be different from the current one".into(),
        }));
    }

    #[test]
    fn a_token_survives_a_backend_that_cannot_be_reached() {
        assert!(!should_forget_token(&AppError::Connection("couldn't reach the VibeSSH cloud backend".into())));
        assert!(!should_forget_token(&AppError::Timeout { operation: "the request", seconds: 30 }));
        assert!(!should_forget_token(&AppError::Internal("backend returned 500".into())));
    }
}
