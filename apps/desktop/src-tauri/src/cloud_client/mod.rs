//! Thin HTTP client for the cloud backend (accounts/teams/roles/audit/
//! invitations - see `backend/`). Every method here does exactly one HTTP
//! call and maps the response - retry/refresh/session orchestration lives
//! in `services::cloud_service`, not here, same thin-client-vs-service
//! split as `ssh::client` vs `services::ssh_service`.
use reqwest::{Method, StatusCode};
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{
    CloudDeviceKey, CloudMemberAccess,
    CloudAiAnswer, CloudAiQuota,
    CloudAuditEvent, CloudAuthResponse, CloudProvisionedMember, CloudRole,
    CloudRoleWithPermissions, CloudServer,
    CloudTeam, CloudTeamMember, CloudUserProfile,
};

pub struct CloudClient {
    base_url: String,
    http: reqwest::Client,
}

impl CloudClient {
    pub fn new(base_url: String) -> Self {
        Self { base_url: base_url.trim_end_matches('/').to_string(), http: reqwest::Client::new() }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    async fn send<B: Serialize + ?Sized, T: serde::de::DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        bearer: Option<&str>,
        body: Option<&B>,
    ) -> AppResult<T> {
        let mut request = self.http.request(method, format!("{}{path}", self.base_url));
        if let Some(token) = bearer {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(body);
        }

        let response = request.send().await.map_err(|err| self.unreachable(err))?;
        Self::parse(response).await
    }

    /// A backend that did not answer, said so that somebody can act on it.
    ///
    /// The bare transport error names the URL and nothing else, which for the
    /// default address reads as "error sending request for url
    /// (http://localhost:8787/auth/register)" - accurate, and no help at all
    /// to somebody who never chose that address and has no idea it is a
    /// setting. VibeSSH does not host a backend; each install points at one
    /// it runs itself, so the fix is always the same and belongs in the
    /// message.
    fn unreachable(&self, err: reqwest::Error) -> AppError {
        let advice = if self.base_url.starts_with("http://localhost") || self.base_url.starts_with("http://127.0.0.1") {
            " - this is the development default, a server on this computer. Set your own backend's address in Settings."
        } else {
            " - check the address in Settings, and that the backend is running."
        };
        AppError::Connection(format!("couldn't reach the VibeSSH cloud backend at {}{advice} ({err})", self.base_url))
    }

    async fn send_no_content<B: Serialize + ?Sized>(&self, method: Method, path: &str, bearer: Option<&str>, body: Option<&B>) -> AppResult<()> {
        let mut request = self.http.request(method, format!("{}{path}", self.base_url));
        if let Some(token) = bearer {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.map_err(|err| self.unreachable(err))?;
        if response.status().is_success() {
            return Ok(());
        }
        Err(Self::error_for(response.status(), response.text().await.unwrap_or_default()))
    }

    async fn parse<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> AppResult<T> {
        let status = response.status();
        let bytes = response.bytes().await.map_err(|err| AppError::Connection(format!("failed to read the backend's response: {err}")))?;
        if status.is_success() {
            return serde_json::from_slice(&bytes)
                .map_err(|err| AppError::Internal(format!("backend response didn't match the expected shape: {err}")));
        }
        Err(Self::error_for(status, String::from_utf8_lossy(&bytes).into_owned()))
    }

    /// Best-effort: the backend always returns `{ kind, code, message, params }`
    /// on error (see apps/backend/src/errors.rs), but this falls back to the raw
    /// body if something ahead of it (a proxy, a network edge case) ever
    /// returns something else - never panics on an unexpected error shape.
    ///
    /// The `code` is what matters to the user: it is what the interface
    /// translates by. Before it existed, the backend's English `message` was
    /// dropped into a translated frame, so a Polish interface said "Brak
    /// uprawnien: invalid email or password". The `kind` is kept separately
    /// because behaviour hangs off it - a 401 has to keep looking like one.
    fn error_for(status: StatusCode, body: String) -> AppError {
        let parsed = serde_json::from_str::<serde_json::Value>(&body).ok();
        let field = |name: &str| {
            parsed.as_ref().and_then(|value| value.get(name)).and_then(|v| v.as_str()).map(str::to_string)
        };
        let message = field("message").unwrap_or(body);
        let params = parsed
            .as_ref()
            .and_then(|value| value.get("params"))
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        // Its own variant, not a generic failure: the UI says something
        // specific about the daily allowance - when it resets, and that a
        // personal API key is the way around it.
        //
        // Matched on the backend's code rather than on the status, because
        // 429 is no longer only the AI allowance: the sign-in rate limit
        // returns it too, and reporting "today's AI allowance is used up" to
        // somebody who mistyped their password five times is a message about
        // a feature they were not using.
        if field("code").as_deref() == Some("ai_daily_limit_used") {
            return AppError::AiQuotaExhausted;
        }

        match (field("kind"), field("code")) {
            (Some(kind), Some(code)) => AppError::Cloud { kind, code, params, message },
            // No code in the body: an older backend, or something between us
            // and it. Fall back to classifying by status, which is all there
            // was before.
            _ => match status {
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => AppError::Unauthorized(message),
                StatusCode::NOT_FOUND => AppError::NotFound(message),
                StatusCode::BAD_REQUEST | StatusCode::CONFLICT => AppError::InvalidInput(message),
                _ => AppError::Internal(format!("cloud backend returned {status}: {message}")),
            },
        }
    }

    pub async fn register(&self, email: &str, password: &str, display_name: &str) -> AppResult<CloudAuthResponse> {
        self.send(Method::POST, "/auth/register", None, Some(&json!({ "email": email, "password": password, "displayName": display_name })))
            .await
    }

    pub async fn login(&self, email: &str, password: &str) -> AppResult<CloudAuthResponse> {
        self.send(Method::POST, "/auth/login", None, Some(&json!({ "email": email, "password": password }))).await
    }

    pub async fn refresh(&self, refresh_token: &str) -> AppResult<CloudAuthResponse> {
        self.send(Method::POST, "/auth/refresh", None, Some(&json!({ "refreshToken": refresh_token }))).await
    }

    pub async fn logout(&self, refresh_token: &str) -> AppResult<()> {
        self.send_no_content(Method::POST, "/auth/logout", None, Some(&json!({ "refreshToken": refresh_token }))).await
    }

    /// One question against the model VibeSSH includes.
    ///
    /// No model, endpoint or key crosses this call in either direction -
    /// all three belong to the backend, which is the whole point of the
    /// hosted arrangement (see `apps/backend/src/ai.rs`). What comes back is an
    /// answer and the account's remaining allowance.
    pub async fn ai_chat(&self, access_token: &str, messages: &serde_json::Value) -> AppResult<CloudAiAnswer> {
        self.send(Method::POST, "/ai/chat", Some(access_token), Some(&json!({ "messages": messages }))).await
    }

    /// The account's usage today, without spending any of it.
    pub async fn ai_quota(&self, access_token: &str) -> AppResult<CloudAiQuota> {
        self.send::<(), _>(Method::GET, "/ai/quota", Some(access_token), None).await
    }

    pub async fn me(&self, access_token: &str) -> AppResult<CloudUserProfile> {
        self.send::<(), _>(Method::GET, "/auth/me", Some(access_token), None).await
    }

    pub async fn list_teams(&self, access_token: &str) -> AppResult<Vec<CloudTeam>> {
        self.send::<(), _>(Method::GET, "/teams", Some(access_token), None).await
    }

    pub async fn get_team(&self, access_token: &str, team_id: Uuid) -> AppResult<CloudTeam> {
        self.send::<(), _>(Method::GET, &format!("/teams/{team_id}"), Some(access_token), None).await
    }

    pub async fn create_team(&self, access_token: &str, name: &str) -> AppResult<CloudTeam> {
        self.send(Method::POST, "/teams", Some(access_token), Some(&json!({ "name": name }))).await
    }

    pub async fn list_members(&self, access_token: &str, team_id: Uuid) -> AppResult<Vec<CloudTeamMember>> {
        self.send::<(), _>(Method::GET, &format!("/teams/{team_id}/members"), Some(access_token), None).await
    }

    pub async fn list_permissions(&self, access_token: &str) -> AppResult<Vec<String>> {
        self.send::<(), _>(Method::GET, "/permissions", Some(access_token), None).await
    }

    pub async fn list_roles(&self, access_token: &str, team_id: Uuid) -> AppResult<Vec<CloudRoleWithPermissions>> {
        self.send::<(), _>(Method::GET, &format!("/teams/{team_id}/roles"), Some(access_token), None).await
    }

    pub async fn create_role(
        &self,
        access_token: &str,
        team_id: Uuid,
        name: &str,
        description: Option<&str>,
        permissions: &[String],
    ) -> AppResult<CloudRoleWithPermissions> {
        self.send(
            Method::POST,
            &format!("/teams/{team_id}/roles"),
            Some(access_token),
            Some(&json!({ "name": name, "description": description, "permissions": permissions })),
        )
        .await
    }

    pub async fn update_role(
        &self,
        access_token: &str,
        team_id: Uuid,
        role_id: Uuid,
        name: &str,
        description: Option<&str>,
        permissions: &[String],
    ) -> AppResult<CloudRoleWithPermissions> {
        self.send(
            Method::PATCH,
            &format!("/teams/{team_id}/roles/{role_id}"),
            Some(access_token),
            Some(&json!({ "name": name, "description": description, "permissions": permissions })),
        )
        .await
    }

    pub async fn delete_role(&self, access_token: &str, team_id: Uuid, role_id: Uuid) -> AppResult<()> {
        self.send_no_content::<()>(Method::DELETE, &format!("/teams/{team_id}/roles/{role_id}"), Some(access_token), None).await
    }

    pub async fn list_member_roles(&self, access_token: &str, team_id: Uuid, user_id: Uuid) -> AppResult<Vec<CloudRole>> {
        self.send::<(), _>(Method::GET, &format!("/teams/{team_id}/members/{user_id}/roles"), Some(access_token), None).await
    }

    pub async fn assign_role(&self, access_token: &str, team_id: Uuid, user_id: Uuid, role_id: Uuid) -> AppResult<()> {
        self.send_no_content(
            Method::POST,
            &format!("/teams/{team_id}/members/{user_id}/roles"),
            Some(access_token),
            Some(&json!({ "roleId": role_id })),
        )
        .await
    }

    pub async fn unassign_role(&self, access_token: &str, team_id: Uuid, user_id: Uuid, role_id: Uuid) -> AppResult<()> {
        self.send_no_content::<()>(
            Method::DELETE,
            &format!("/teams/{team_id}/members/{user_id}/roles/{role_id}"),
            Some(access_token),
            None,
        )
        .await
    }

    pub async fn list_servers(&self, access_token: &str, team_id: Uuid) -> AppResult<Vec<CloudServer>> {
        self.send::<(), _>(Method::GET, &format!("/teams/{team_id}/servers"), Some(access_token), None).await
    }

    pub async fn create_server(
        &self,
        access_token: &str,
        team_id: Uuid,
        name: &str,
        host: &str,
        ssh_port: i32,
        username: Option<&str>,
    ) -> AppResult<CloudServer> {
        self.send(
            Method::POST,
            &format!("/teams/{team_id}/servers"),
            Some(access_token),
            Some(&json!({ "name": name, "host": host, "sshPort": ssh_port, "username": username })),
        )
        .await
    }

    pub async fn delete_server(&self, access_token: &str, team_id: Uuid, server_id: Uuid) -> AppResult<()> {
        self.send_no_content::<()>(Method::DELETE, &format!("/teams/{team_id}/servers/{server_id}"), Some(access_token), None).await
    }

    pub async fn my_permissions(&self, access_token: &str, team_id: Uuid) -> AppResult<Vec<String>> {
        self.send::<(), _>(Method::GET, &format!("/teams/{team_id}/me/permissions"), Some(access_token), None).await
    }

    pub async fn remove_member(&self, access_token: &str, team_id: Uuid, user_id: Uuid) -> AppResult<()> {
        self.send_no_content::<()>(Method::DELETE, &format!("/teams/{team_id}/members/{user_id}"), Some(access_token), None).await
    }

    pub async fn delete_team(&self, access_token: &str, team_id: Uuid) -> AppResult<()> {
        self.send_no_content::<()>(Method::DELETE, &format!("/teams/{team_id}"), Some(access_token), None).await
    }

    /// Creates an account for somebody and adds them to the team.
    ///
    /// The password comes back in the response and is not obtainable again
    /// from anywhere - see the backend's `teams::provision_member`.
    pub async fn provision_member(
        &self,
        access_token: &str,
        team_id: Uuid,
        email: &str,
        display_name: Option<&str>,
        role_id: Option<Uuid>,
    ) -> AppResult<CloudProvisionedMember> {
        self.send(
            Method::POST,
            &format!("/teams/{team_id}/members/provision"),
            Some(access_token),
            Some(&json!({ "email": email, "displayName": display_name, "roleId": role_id })),
        )
        .await
    }

    /// Replaces the signed-in account's own password.
    ///
    /// Returns a fresh session, because the backend ends every other one -
    /// including, from its point of view, this caller's.
    pub async fn change_password(&self, access_token: &str, current_password: &str, new_password: &str) -> AppResult<CloudAuthResponse> {
        self.send(
            Method::POST,
            "/auth/password",
            Some(access_token),
            Some(&json!({ "currentPassword": current_password, "newPassword": new_password })),
        )
        .await
    }

    /// Registers this device's public key, or refreshes it if the backend
    /// already knows it.
    pub async fn publish_device_key(&self, access_token: &str, public_key: &str, label: &str) -> AppResult<CloudDeviceKey> {
        self.send(Method::POST, "/devices", Some(access_token), Some(&json!({ "publicKey": public_key, "label": label }))).await
    }

    pub async fn list_device_keys(&self, access_token: &str) -> AppResult<Vec<CloudDeviceKey>> {
        self.send::<(), _>(Method::GET, "/devices", Some(access_token), None).await
    }

    /// Forgets a device. This removes it from the team's view and does not
    /// remove it from any Node it was already installed on - see the
    /// backend's own doc comment, and say so wherever this is offered.
    pub async fn revoke_device_key(&self, access_token: &str, key_id: Uuid) -> AppResult<()> {
        self.send_no_content::<()>(Method::DELETE, &format!("/devices/{key_id}"), Some(access_token), None).await
    }

    /// Everyone in the team, the account each gets on a Node, and the keys
    /// that belong in it.
    pub async fn list_team_access(&self, access_token: &str, team_id: Uuid) -> AppResult<Vec<CloudMemberAccess>> {
        self.send::<(), _>(Method::GET, &format!("/teams/{team_id}/access"), Some(access_token), None).await
    }

    pub async fn list_audit_events(&self, access_token: &str, team_id: Uuid, limit: i64, offset: i64) -> AppResult<Vec<CloudAuditEvent>> {
        self.send::<(), _>(Method::GET, &format!("/teams/{team_id}/audit?limit={limit}&offset={offset}"), Some(access_token), None).await
    }
}
