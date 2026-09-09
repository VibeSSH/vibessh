use tauri::{Manager, State};

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

/// Whether containers can run on this machine - what the wizard asks before
/// offering the Docker runtime for a Local application.
#[tauri::command]
pub async fn local_docker_available() -> bool {
    services::local_docker_available().await
}

/// Where a local application's files should go, unless somebody says
/// otherwise.
///
/// **Why the app answers this rather than the wizard guessing.** A remote
/// application gets `/home/container/<name>` suggested for it, and a local
/// one got nothing - the field was left empty and required, so the first
/// thing somebody does on Windows is invent a path, and what they invent is
/// usually the Desktop. That is a working directory a server writes worlds
/// and logs into, sitting where files get dragged around and deleted.
///
/// The path is built here because only this side knows it: it comes from
/// Tauri's own resolver, so it is `%APPDATA%` on Windows and the XDG data
/// directory elsewhere, with the platform's own separator already in it.
#[tauri::command]
pub fn local_applications_root(app: tauri::AppHandle) -> AppResult<String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| crate::errors::AppError::Storage(format!("couldn't resolve the application data directory: {err}")))?
        .join("applications");
    Ok(dir.to_string_lossy().into_owned())
}
