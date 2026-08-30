use tauri::State;

use crate::errors::AppResult;
use crate::models::AppInfo;
use crate::services;
use crate::state::AppState;

/// Thin wrapper: commands only translate between Tauri's calling convention
/// and the service layer, they never contain business logic themselves.
#[tauri::command]
pub fn get_app_info(state: State<AppState>) -> AppResult<AppInfo> {
    services::get_app_info(&state)
}
