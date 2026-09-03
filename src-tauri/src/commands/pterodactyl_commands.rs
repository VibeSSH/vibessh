//! Tauri command bridge for the Pterodactyl migration.
//!
//! **The API key crosses this bridge exactly once, inward.** The wizard
//! sends it when the panel is first connected; it goes straight into the OS
//! keyring and every later call reads it from there. Nothing here ever
//! returns it, and `has_stored_key` is the whole of what the interface is
//! told about it - the same shape the Vibe AI provider key already uses, for
//! the same reason: a key the frontend never holds is a key a webview cannot
//! leak.

use crate::errors::{AppError, AppResult};
use crate::pterodactyl::PterodactylClient;
use crate::services::{self, PterodactylMigrationPlan, PterodactylNodeOverride};
use crate::storage::credentials;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::server_repository::ServerRepository;
use tauri::State;

/// What the wizard knows about the connection, with no key in it.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PterodactylConnectionView {
    pub has_stored_key: bool,
}

/// Builds a client from the stored key, or says plainly that there is none.
fn client_from_storage(base_url: &str) -> AppResult<PterodactylClient> {
    let key = credentials::load_pterodactyl_api_key()?
        .filter(|key| !key.trim().is_empty())
        .ok_or_else(|| AppError::Unauthorized("no Pterodactyl API key is stored - connect to the panel again".to_string()))?;
    PterodactylClient::new(base_url, &key)
}

#[tauri::command]
pub fn pterodactyl_connection() -> AppResult<PterodactylConnectionView> {
    Ok(PterodactylConnectionView { has_stored_key: credentials::load_pterodactyl_api_key()?.is_some_and(|key| !key.trim().is_empty()) })
}

/// Checks the address and the key, and only stores the key once the panel
/// has actually accepted it.
///
/// Storing first and validating later would leave a wrong key in the keyring
/// behind an error message, which is how somebody ends up debugging a
/// migration against a credential they already replaced.
#[tauri::command]
pub async fn pterodactyl_connect(base_url: String, api_key: String) -> AppResult<u32> {
    let client = PterodactylClient::new(&base_url, &api_key)?;
    let total = client.check_access().await?;
    credentials::store_pterodactyl_api_key(api_key.trim())?;
    Ok(total)
}

/// Reads the whole panel and returns what it would become here. Writes
/// nothing, on either side.
#[tauri::command]
pub async fn pterodactyl_plan(
    base_url: String,
    node_overrides: Option<Vec<PterodactylNodeOverride>>,
    servers: State<'_, ServerRepository>,
    applications: State<'_, ApplicationRepository>,
) -> AppResult<PterodactylMigrationPlan> {
    let client = client_from_storage(&base_url)?;
    let mut plan = services::build_pterodactyl_plan(&client, &servers, &applications, &node_overrides.unwrap_or_default()).await?;
    plan.panel_url = base_url.trim().trim_end_matches('/').to_string();
    Ok(plan)
}

/// Forgets the key. An admin key for a panel being decommissioned has no
/// reason to outlive the migration.
#[tauri::command]
pub fn pterodactyl_forget() -> AppResult<()> {
    credentials::delete_pterodactyl_api_key()
}
