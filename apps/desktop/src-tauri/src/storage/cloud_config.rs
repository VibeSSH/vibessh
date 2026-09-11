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
pub const DEFAULT_BACKEND_URL: &str = "http://localhost:8787";

/// Whether an address is a real choice rather than the untouched default.
///
/// Asked by the interface so it can say "accounts are off" instead of
/// showing a failed request, and kept here so there is one literal rather
/// than a copy of it in TypeScript that would drift silently the day this
/// becomes a hosted address.
pub fn is_configured(backend_url: &str) -> bool {
    let trimmed = backend_url.trim();
    !trimmed.is_empty() && trimmed.trim_end_matches('/') != DEFAULT_BACKEND_URL
}

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

    /// The default is "accounts are off", not "accounts are broken" - the
    /// interface says so on the strength of this rather than by comparing
    /// against a literal of its own.
    #[test]
    fn the_untouched_default_does_not_count_as_configured() {
        assert!(!is_configured(DEFAULT_BACKEND_URL));
        // A trailing slash is the same address, and somebody will type one.
        assert!(!is_configured("http://localhost:8787/"));
        assert!(!is_configured("  http://localhost:8787  "));
        assert!(!is_configured(""));
    }

    #[test]
    fn a_real_address_counts() {
        assert!(is_configured("https://konta.example.com"));
        // Somebody self-hosting on their own machine on a different port has
        // made a choice, and it is not the default.
        assert!(is_configured("http://localhost:9000"));
    }

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
