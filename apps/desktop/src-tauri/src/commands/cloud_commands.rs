use tauri::{Manager, State};
use uuid::Uuid;

use crate::errors::AppResult;
use crate::models::{
    CloudAuditEvent, CloudProvisionedMember, CloudRole, CloudRoleWithPermissions,
    CloudServer, CloudSessionInfo,
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

/// Whether accounts can work at all yet - see `cloud_config::is_configured`.
#[tauri::command]
pub async fn cloud_backend_is_configured(state: State<'_, CloudState>) -> AppResult<bool> {
    Ok(cloud_config::is_configured(&state.backend_url().await))
}

#[tauri::command]
pub async fn cloud_set_backend_url(app: tauri::AppHandle, state: State<'_, CloudState>, backend_url: String) -> AppResult<()> {
    let config_dir = app.path().app_config_dir().map_err(|err| crate::errors::AppError::Storage(err.to_string()))?;
    cloud_config::save_backend_url(&config_dir, &backend_url)?;
    state.set_backend_url(backend_url).await;
    Ok(())
}

/// Publishes one Application so the rest of the team can see it.
///
/// A projection: this install keeps the record it acts on, and pushing again
/// refreshes what the team sees.
#[tauri::command]
pub async fn share_application_with_team(
    app_repo: State<'_, crate::storage::application_repository::ApplicationRepository>,
    cloud: State<'_, CloudState>,
    team_id: uuid::Uuid,
    application_id: uuid::Uuid,
    team_server_id: Option<uuid::Uuid>,
) -> AppResult<crate::models::CloudApplication> {
    services::team_application_service::share_application(&app_repo, &cloud, team_id, application_id, team_server_id).await
}

#[tauri::command]
pub async fn list_team_applications(cloud: State<'_, CloudState>, team_id: uuid::Uuid) -> AppResult<Vec<crate::models::CloudApplication>> {
    services::team_application_service::list_shared_applications(&cloud, team_id).await
}

/// Stops sharing. The Application keeps running and this install keeps its
/// own record - only the team's copy goes, which is what the interface has
/// to say next to it.
#[tauri::command]
pub async fn unshare_application_from_team(cloud: State<'_, CloudState>, team_id: uuid::Uuid, application_id: uuid::Uuid) -> AppResult<()> {
    services::team_application_service::unshare_application(&cloud, team_id, application_id).await
}

/// Registers this device's public key so teammates' installs can put it in
/// the account they create for this person on a Node.
///
/// Idempotent, and cheap enough to call whenever a session is established -
/// a key nobody published is a member nobody can be given access to, and the
/// person would have no way of guessing that was the missing step.
#[tauri::command]
pub async fn cloud_publish_this_device(app: tauri::AppHandle, state: State<'_, CloudState>) -> AppResult<()> {
    let config_dir = app.path().app_config_dir().map_err(|err| crate::errors::AppError::Storage(err.to_string()))?;
    services::team_access_service::publish_this_device(&state, &config_dir).await
}

#[tauri::command]
pub async fn cloud_list_devices(state: State<'_, CloudState>) -> AppResult<Vec<crate::models::CloudDeviceKey>> {
    services::cloud_list_device_keys(&state).await
}

/// Forgets a device. Removes it from the team's view; the key comes off the
/// Nodes it was installed on at the next `sync_team_node_access` for each of
/// them, which writes `authorized_keys` whole from the keys still published.
#[tauri::command]
pub async fn cloud_revoke_device(state: State<'_, CloudState>, key_id: uuid::Uuid) -> AppResult<()> {
    services::cloud_revoke_device_key(&state, key_id).await
}

/// What this person logs in as on this team's Nodes, and with which key.
#[tauri::command]
pub async fn cloud_my_node_access(
    app: tauri::AppHandle,
    state: State<'_, CloudState>,
    team_id: uuid::Uuid,
) -> AppResult<services::team_access_service::MyNodeAccess> {
    let config_dir = app.path().app_config_dir().map_err(|err| crate::errors::AppError::Storage(err.to_string()))?;
    services::team_access_service::my_node_access(&state, team_id, &config_dir).await
}

/// What this team has asked to be taken off its Nodes and has not been.
///
/// Read from the backend rather than remembered locally: the install that
/// removed somebody is often not the one that can reach the machine, and
/// this is the only thing that carries the fact between them.
#[tauri::command]
pub async fn cloud_list_pending_revocations(
    state: State<'_, CloudState>,
    team_id: uuid::Uuid,
) -> AppResult<Vec<crate::models::CloudNodeRevocation>> {
    services::cloud_list_pending_revocations(&state, team_id).await
}

/// Makes one Node hold exactly the access this team describes: every current
/// member's account and currently published keys, then every revocation the
/// team is still owed on that machine.
#[tauri::command]
pub async fn sync_team_node_access(
    server_repo: State<'_, crate::storage::server_repository::ServerRepository>,
    sessions: State<'_, crate::state::SshSessionManager>,
    cloud: State<'_, CloudState>,
    server_id: uuid::Uuid,
    team_id: uuid::Uuid,
    team_server_id: uuid::Uuid,
) -> AppResult<services::team_access_service::NodeAccessSync> {
    services::team_access_service::sync_team_access(&server_repo, &sessions, &cloud, server_id, team_id, team_server_id).await
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
pub async fn cloud_provision_member(
    state: State<'_, CloudState>,
    team_id: Uuid,
    email: String,
    display_name: Option<String>,
    role_id: Option<Uuid>,
) -> AppResult<CloudProvisionedMember> {
    services::cloud_provision_member(&state, team_id, &email, display_name.as_deref(), role_id).await
}

#[tauri::command]
pub async fn cloud_change_password(state: State<'_, CloudState>, current_password: String, new_password: String) -> AppResult<CloudUserProfile> {
    services::cloud_change_password(&state, &current_password, &new_password).await
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
