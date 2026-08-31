use tauri::{Manager, State};
use uuid::Uuid;

use crate::errors::AppResult;
use crate::models::{
    CloudAuditEvent, CloudCreatedInvitation, CloudInvitation, CloudRole, CloudRoleWithPermissions, CloudServer, CloudSessionInfo,
    CloudTeam, CloudTeamMember, CloudUserProfile,
};
use crate::services;
use crate::state::cloud_session::CloudState;
use crate::storage::cloud_config;

#[tauri::command]
pub async fn cloud_register(
    state: State<'_, CloudState>,
    email: String,
    password: String,
    display_name: String,
) -> AppResult<CloudUserProfile> {
    services::cloud_register(&state, &email, &password, &display_name).await
}

#[tauri::command]
pub async fn cloud_login(state: State<'_, CloudState>, email: String, password: String) -> AppResult<CloudUserProfile> {
    services::cloud_login(&state, &email, &password).await
}

#[tauri::command]
pub async fn cloud_logout(state: State<'_, CloudState>) -> AppResult<()> {
    services::cloud_logout(&state).await
}

#[tauri::command]
pub async fn cloud_session_info(state: State<'_, CloudState>) -> AppResult<Option<CloudSessionInfo>> {
    Ok(services::cloud_session_info(&state).await)
}

#[tauri::command]
pub async fn cloud_get_backend_url(state: State<'_, CloudState>) -> AppResult<String> {
    Ok(state.backend_url().await)
}

#[tauri::command]
pub async fn cloud_set_backend_url(app: tauri::AppHandle, state: State<'_, CloudState>, backend_url: String) -> AppResult<()> {
    let config_dir = app.path().app_config_dir().map_err(|err| crate::errors::AppError::Storage(err.to_string()))?;
    cloud_config::save_backend_url(&config_dir, &backend_url)?;
    state.set_backend_url(backend_url).await;
    Ok(())
}

#[tauri::command]
pub async fn cloud_list_teams(state: State<'_, CloudState>) -> AppResult<Vec<CloudTeam>> {
    services::cloud_list_teams(&state).await
}

#[tauri::command]
pub async fn cloud_create_team(state: State<'_, CloudState>, name: String) -> AppResult<CloudTeam> {
    services::cloud_create_team(&state, &name).await
}

#[tauri::command]
pub async fn cloud_list_members(state: State<'_, CloudState>, team_id: Uuid) -> AppResult<Vec<CloudTeamMember>> {
    services::cloud_list_members(&state, team_id).await
}

#[tauri::command]
pub async fn cloud_get_team(state: State<'_, CloudState>, team_id: Uuid) -> AppResult<CloudTeam> {
    services::cloud_get_team(&state, team_id).await
}

#[tauri::command]
pub async fn cloud_list_permissions(state: State<'_, CloudState>) -> AppResult<Vec<String>> {
    services::cloud_list_permissions(&state).await
}

#[tauri::command]
pub async fn cloud_list_roles(state: State<'_, CloudState>, team_id: Uuid) -> AppResult<Vec<CloudRoleWithPermissions>> {
    services::cloud_list_roles(&state, team_id).await
}

#[tauri::command]
pub async fn cloud_create_role(
    state: State<'_, CloudState>,
    team_id: Uuid,
    name: String,
    description: Option<String>,
    permissions: Vec<String>,
) -> AppResult<CloudRoleWithPermissions> {
    services::cloud_create_role(&state, team_id, &name, description.as_deref(), &permissions).await
}

#[tauri::command]
pub async fn cloud_update_role(
    state: State<'_, CloudState>,
    team_id: Uuid,
    role_id: Uuid,
    name: String,
    description: Option<String>,
    permissions: Vec<String>,
) -> AppResult<CloudRoleWithPermissions> {
    services::cloud_update_role(&state, team_id, role_id, &name, description.as_deref(), &permissions).await
}

#[tauri::command]
pub async fn cloud_delete_role(state: State<'_, CloudState>, team_id: Uuid, role_id: Uuid) -> AppResult<()> {
    services::cloud_delete_role(&state, team_id, role_id).await
}

#[tauri::command]
pub async fn cloud_list_member_roles(state: State<'_, CloudState>, team_id: Uuid, user_id: Uuid) -> AppResult<Vec<CloudRole>> {
    services::cloud_list_member_roles(&state, team_id, user_id).await
}

#[tauri::command]
pub async fn cloud_assign_role(state: State<'_, CloudState>, team_id: Uuid, user_id: Uuid, role_id: Uuid) -> AppResult<()> {
    services::cloud_assign_role(&state, team_id, user_id, role_id).await
}

#[tauri::command]
pub async fn cloud_unassign_role(state: State<'_, CloudState>, team_id: Uuid, user_id: Uuid, role_id: Uuid) -> AppResult<()> {
    services::cloud_unassign_role(&state, team_id, user_id, role_id).await
}

#[tauri::command]
pub async fn cloud_list_servers(state: State<'_, CloudState>, team_id: Uuid) -> AppResult<Vec<CloudServer>> {
    services::cloud_list_servers(&state, team_id).await
}

#[tauri::command]
pub async fn cloud_create_server(
    state: State<'_, CloudState>,
    team_id: Uuid,
    name: String,
    host: String,
    ssh_port: i32,
    username: Option<String>,
) -> AppResult<CloudServer> {
    services::cloud_create_server(&state, team_id, &name, &host, ssh_port, username.as_deref()).await
}

#[tauri::command]
pub async fn cloud_delete_server(state: State<'_, CloudState>, team_id: Uuid, server_id: Uuid) -> AppResult<()> {
    services::cloud_delete_server(&state, team_id, server_id).await
}

#[tauri::command]
pub async fn cloud_my_permissions(state: State<'_, CloudState>, team_id: Uuid) -> AppResult<Vec<String>> {
    services::cloud_my_permissions(&state, team_id).await
}

#[tauri::command]
pub async fn cloud_remove_member(state: State<'_, CloudState>, team_id: Uuid, user_id: Uuid) -> AppResult<()> {
    services::cloud_remove_member(&state, team_id, user_id).await
}

#[tauri::command]
pub async fn cloud_delete_team(state: State<'_, CloudState>, team_id: Uuid) -> AppResult<()> {
    services::cloud_delete_team(&state, team_id).await
}

#[tauri::command]
pub async fn cloud_list_invitations(state: State<'_, CloudState>, team_id: Uuid) -> AppResult<Vec<CloudInvitation>> {
    services::cloud_list_invitations(&state, team_id).await
}

#[tauri::command]
pub async fn cloud_create_invitation(
    state: State<'_, CloudState>,
    team_id: Uuid,
    email: String,
    role_id: Option<Uuid>,
    expires_in_days: Option<i64>,
) -> AppResult<CloudCreatedInvitation> {
    services::cloud_create_invitation(&state, team_id, &email, role_id, expires_in_days).await
}

#[tauri::command]
pub async fn cloud_revoke_invitation(state: State<'_, CloudState>, team_id: Uuid, invitation_id: Uuid) -> AppResult<()> {
    services::cloud_revoke_invitation(&state, team_id, invitation_id).await
}

#[tauri::command]
pub async fn cloud_accept_invitation(state: State<'_, CloudState>, token: String) -> AppResult<CloudTeam> {
    services::cloud_accept_invitation(&state, &token).await
}

#[tauri::command]
pub async fn cloud_decline_invitation(state: State<'_, CloudState>, token: String) -> AppResult<()> {
    services::cloud_decline_invitation(&state, &token).await
}

#[tauri::command]
pub async fn cloud_list_audit_events(
    state: State<'_, CloudState>,
    team_id: Uuid,
    limit: i64,
    offset: i64,
) -> AppResult<Vec<CloudAuditEvent>> {
    services::cloud_list_audit_events(&state, team_id, limit, offset).await
}
