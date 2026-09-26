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

/// Why VibeSSH did not start, or `None` when it did. The interface asks this
/// before anything else, and shows its startup-failure screen instead of the
/// app when there is an answer - see `crash_report`. Takes no state: when
/// startup failed, none was set up.
#[tauri::command]
pub fn get_startup_failure() -> Option<crate::crash_report::StartupFailure> {
    crate::crash_report::startup_failure().cloned()
}

/// Opens the file manager on the startup failure's report file.
#[tauri::command]
pub fn reveal_crash_report() -> AppResult<()> {
    crate::crash_report::reveal_startup_report()
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

/// What the close button currently does, for the Settings switch to show.
#[tauri::command]
pub fn get_tray_settings(state: State<crate::tray::TrayState>) -> AppResult<crate::storage::tray_config::TrayConfig> {
    state
        .config
        .lock()
        .map(|config| *config)
        .map_err(|_| crate::errors::AppError::Internal("the tray setting is unreadable".into()))
}

/// Turns hiding-on-close on or off, and writes it down.
///
/// Saved immediately rather than on exit, because the setting's whole subject
/// is what happens when the application goes away - a value that only reached
/// disk during a clean shutdown would be the one value most likely to be lost.
#[tauri::command]
pub fn set_minimize_to_tray(app: tauri::AppHandle, state: State<crate::tray::TrayState>, enabled: bool) -> AppResult<()> {
    let config = {
        let mut config = state
            .config
            .lock()
            .map_err(|_| crate::errors::AppError::Internal("the tray setting is unwritable".into()))?;
        config.minimize_to_tray = enabled;
        *config
    };
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|err| crate::errors::AppError::Storage(format!("couldn't resolve the config directory: {err}")))?;
    crate::storage::tray_config::save_tray_config(&config_dir, &config)
}

/// The language the interface settled on, so the tray menu can be in it.
///
/// The interface decides the language - from a saved choice or the system's
/// own - and the tray menu is built in Rust, where none of that is visible.
/// This is the one wire between them. Called on startup and whenever the
/// language is changed, and cheap enough to be called redundantly: an
/// unchanged language rebuilds nothing.
#[tauri::command]
pub fn set_tray_language(app: tauri::AppHandle, language: String) {
    crate::tray::set_language(&app, &language);
}

/// What the local MCP endpoint is set to, and how to point a client at it.
///
/// The token travels to the interface so it can be copied into a client's
/// configuration - that is the whole purpose of showing it. It is read from
/// the keyring on demand rather than held anywhere, and it is generated on
/// this first read if there was none.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpSettings {
    pub enabled: bool,
    pub allow_changes: bool,
    pub port: u16,
    pub url: String,
    pub token: String,
}

fn mcp_settings_from(config: crate::storage::mcp_config::McpConfig) -> AppResult<McpSettings> {
    Ok(McpSettings {
        enabled: config.enabled,
        allow_changes: config.allow_changes,
        port: config.port,
        url: format!("http://127.0.0.1:{}/mcp", config.port),
        token: crate::mcp::token()?,
    })
}

#[tauri::command]
pub fn get_mcp_settings(app: tauri::AppHandle) -> AppResult<McpSettings> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|err| crate::errors::AppError::Storage(format!("couldn't resolve the config directory: {err}")))?;
    mcp_settings_from(crate::storage::mcp_config::load_mcp_config(&config_dir))
}

/// Saves the setting and makes it true in the same call.
///
/// Written to disk *after* the endpoint has actually opened, so a port
/// already taken by something else leaves the setting off rather than
/// recording a state the app is not in - the next launch would otherwise
/// fail the same way with nobody watching.
#[tauri::command]
pub async fn set_mcp_settings(
    app: tauri::AppHandle,
    state: State<'_, std::sync::Arc<crate::mcp::McpState>>,
    enabled: bool,
    allow_changes: bool,
    port: u16,
) -> AppResult<McpSettings> {
    let config = crate::storage::mcp_config::McpConfig { enabled, allow_changes, port };
    crate::mcp::apply(&app, &state, config).await?;

    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|err| crate::errors::AppError::Storage(format!("couldn't resolve the config directory: {err}")))?;
    crate::storage::mcp_config::save_mcp_config(&config_dir, &config)?;
    mcp_settings_from(config)
}

/// A new token, and every client configured with the old one stops working.
#[tauri::command]
pub async fn rotate_mcp_token(app: tauri::AppHandle, state: State<'_, std::sync::Arc<crate::mcp::McpState>>) -> AppResult<McpSettings> {
    crate::mcp::rotate_token()?;
    // Restarted so the running endpoint stops accepting the old one - a
    // rotation that left the old token working until the next launch would
    // be worse than no rotation, because it would look like it had worked.
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|err| crate::errors::AppError::Storage(format!("couldn't resolve the config directory: {err}")))?;
    let config = crate::storage::mcp_config::load_mcp_config(&config_dir);
    crate::mcp::apply(&app, &state, config).await?;
    mcp_settings_from(config)
}
