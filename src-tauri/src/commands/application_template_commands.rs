//! Saved starting points for the Create Application wizard.
//!
//! Thin on purpose: the rules that matter - a name is required, a secret
//! value never reaches the file - belong to `storage::application_template_config`,
//! which is the last place before the disk and therefore the only place they
//! can actually be guaranteed.

use tauri::{AppHandle, Manager};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::ApplicationTemplate;
use crate::storage::application_template_config;

fn config_dir(app: &AppHandle) -> AppResult<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .map_err(|err| AppError::Storage(format!("couldn't locate the config directory: {err}")))
}

#[tauri::command]
pub fn list_application_templates(app: AppHandle) -> AppResult<Vec<ApplicationTemplate>> {
    application_template_config::load_templates(&config_dir(&app)?)
}

#[tauri::command]
pub fn save_application_template(app: AppHandle, template: ApplicationTemplate) -> AppResult<ApplicationTemplate> {
    application_template_config::save_template(&config_dir(&app)?, template)
}

#[tauri::command]
pub fn delete_application_template(app: AppHandle, template_id: Uuid) -> AppResult<()> {
    application_template_config::delete_template(&config_dir(&app)?, template_id)
}
