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
    CloudAuthResponse, CloudRole, CloudRoleWithPermissions, CloudServer, CloudTeam, CloudTeamMember, CloudUserProfile,
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

        let response = request.send().await.map_err(|err| AppError::Connection(format!("couldn't reach the VibeSSH cloud backend: {err}")))?;
        Self::parse(response).await
    }

    async fn send_no_content<B: Serialize + ?Sized>(&self, method: Method, path: &str, bearer: Option<&str>, body: Option<&B>) -> AppResult<()> {
        let mut request = self.http.request(method, format!("{}{path}", self.base_url));
        if let Some(token) = bearer {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.map_err(|err| AppError::Connection(format!("couldn't reach the VibeSSH cloud backend: {err}")))?;
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

    /// Best-effort: the backend always returns `{ kind, message }` on error
    /// (see backend/src/errors.rs), but this falls back to the raw body if
    /// something ahead of it (a proxy, a network edge case) ever returns
    /// something else - never panics on an unexpected error shape.
    fn error_for(status: StatusCode, body: String) -> AppError {
        let message = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|value| value.get("message").and_then(|m| m.as_str()).map(str::to_string))
            .unwrap_or(body);
        match status {
            StatusCode::UNAUTHORIZED => AppError::Unauthorized(message),
            StatusCode::FORBIDDEN => AppError::Unauthorized(message),
            StatusCode::NOT_FOUND => AppError::NotFound(message),
            StatusCode::BAD_REQUEST | StatusCode::CONFLICT => AppError::InvalidInput(message),
            _ => AppError::Internal(format!("cloud backend returned {status}: {message}")),
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
}
