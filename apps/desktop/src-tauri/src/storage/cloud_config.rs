//! Which cloud backend this device talks to - a plain JSON file in the app
//! config dir (same load-or-create pattern as the agent's own identity.rs),
//! not hardcoded, since a self-hosted backend's URL is a real per-install
//! setting (see the Invitations stage's "no hardcoded invitation URLs" -
//! same reasoning applies to the backend's own address).
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

const CONFIG_FILE_NAME: &str = "cloud_config.json";
/// The hosted VibeSSH backend. This is a real service now, so a fresh
/// install has working accounts without anybody typing an address; a
/// self-hoster still points this at their own instance via settings.
pub const DEFAULT_BACKEND_URL: &str = "https://api.vibessh.dev";

/// What the default used to be, while there was no hosted backend to point
/// at. Kept because the config file of an existing install still says it -
/// nobody chose it, it was simply the default at the time - and reading it
/// as a deliberate self-hosting choice would leave those installs talking to
/// a `localhost` that has nothing on it.
const LEGACY_LOCAL_DEFAULT: &str = "http://localhost:8787";

/// Whether this install has a backend worth talking to.
///
/// Asked by the interface so it can say "accounts are off" instead of
/// showing a failed request. This used to mean "not the default", because
/// the default was a `localhost` address that only ever existed on the
/// developer's own machine. Now the default is the hosted service, so the
/// test is the other way round: anything except the old local default
/// counts, including the new default itself.
pub fn is_configured(backend_url: &str) -> bool {
    let trimmed = backend_url.trim().trim_end_matches('/');
    !trimmed.is_empty() && trimmed != LEGACY_LOCAL_DEFAULT
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

    /// An install that never touched the setting, from back when the default
    /// was `localhost`, is "accounts are off" rather than "accounts are
    /// broken" - the interface says so on the strength of this rather than
    /// by comparing against a literal of its own.
    #[test]
    fn the_old_local_default_does_not_count_as_configured() {
        assert!(!is_configured(LEGACY_LOCAL_DEFAULT));
        // A trailing slash is the same address, and somebody will type one.
        assert!(!is_configured("http://localhost:8787/"));
        assert!(!is_configured("  http://localhost:8787  "));
        assert!(!is_configured(""));
    }

    /// The point of the change: a fresh install has working accounts without
    /// anybody typing anything.
    #[test]
    fn the_current_default_counts_as_configured() {
        assert!(is_configured(DEFAULT_BACKEND_URL));
    }

    #[test]
    fn a_real_address_counts() {
        assert!(is_configured("https://konta.example.com"));
        // Somebody self-hosting on their own machine on a different port has
        // made a choice, and it is not the old default.
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
