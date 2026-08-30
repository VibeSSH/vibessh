//! Which cloud backend this device talks to - a plain JSON file in the app
//! config dir (same load-or-create pattern as the agent's own identity.rs),
//! not hardcoded, since a self-hosted backend's URL is a real per-install
//! setting (see the Invitations stage's "no hardcoded invitation URLs" -
//! same reasoning applies to the backend's own address).
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

const CONFIG_FILE_NAME: &str = "cloud_config.json";
/// This machine's own dev instance (see backend/.env.example) - a
/// reasonable default for local development, not a real hosted service.
/// Anyone else running this needs to point it at their own backend via
/// settings.
const DEFAULT_BACKEND_URL: &str = "http://localhost:8787";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CloudConfig {
    backend_url: String,
}

fn config_path(config_dir: &Path) -> PathBuf {
    config_dir.join(CONFIG_FILE_NAME)
}

pub fn load_backend_url(config_dir: &Path) -> AppResult<String> {
    let path = config_path(config_dir);
    if !path.exists() {
        return Ok(DEFAULT_BACKEND_URL.to_string());
    }
    let bytes = std::fs::read(&path).map_err(|err| AppError::Storage(format!("failed to read cloud_config.json: {err}")))?;
    let config: CloudConfig =
        serde_json::from_slice(&bytes).map_err(|err| AppError::Storage(format!("cloud_config.json is corrupt: {err}")))?;
    Ok(config.backend_url)
}

pub fn save_backend_url(config_dir: &Path, backend_url: &str) -> AppResult<()> {
    std::fs::create_dir_all(config_dir).map_err(|err| AppError::Storage(format!("failed to create the config directory: {err}")))?;
    let config = CloudConfig { backend_url: backend_url.to_string() };
    let bytes = serde_json::to_vec_pretty(&config).map_err(|err| AppError::Storage(format!("failed to encode cloud_config.json: {err}")))?;
    std::fs::write(config_path(config_dir), bytes).map_err(|err| AppError::Storage(format!("failed to write cloud_config.json: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_config_file_falls_back_to_the_default_url() {
        let dir = std::env::temp_dir().join(format!("vibessh-cloud-config-test-{}", uuid::Uuid::new_v4()));
        assert_eq!(load_backend_url(&dir).unwrap(), DEFAULT_BACKEND_URL);
    }

    #[test]
    fn saving_then_loading_round_trips_a_custom_url() {
        let dir = std::env::temp_dir().join(format!("vibessh-cloud-config-test-{}", uuid::Uuid::new_v4()));
        save_backend_url(&dir, "https://vibessh.example.com").unwrap();
        assert_eq!(load_backend_url(&dir).unwrap(), "https://vibessh.example.com");
        std::fs::remove_dir_all(&dir).ok();
    }
}
