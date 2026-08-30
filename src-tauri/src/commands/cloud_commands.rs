use tauri::{Manager, State};
use uuid::Uuid;

use crate::errors::AppResult;
use crate::models::{CloudSessionInfo, CloudTeam, CloudTeamMember, CloudUserProfile};
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
