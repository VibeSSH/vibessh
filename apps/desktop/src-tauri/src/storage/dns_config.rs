//! Where the Vibe Network's own DNS suffix lives - a plain JSON file in
//! the app config dir, same load-or-create pattern `cloud_config.rs`
//! already uses for the cloud backend URL (a single, global, per-install
//! setting, not per-Node/per-Application data that belongs in the SQLite
//! store).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

const CONFIG_FILE_NAME: &str = "dns_config.json";
/// The suffix every install used before this was configurable - never
/// `.local` (see the design doc's own reasoning: that suffix collides with
/// mDNS), and short enough that `db01.vibe`-style aliases stay readable.
pub const DEFAULT_SUFFIX: &str = ".vibe";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DnsConfig {
    suffix: String,
}

fn config_path(config_dir: &Path) -> PathBuf {
    config_dir.join(CONFIG_FILE_NAME)
}

pub fn load_dns_suffix(config_dir: &Path) -> AppResult<String> {
    let path = config_path(config_dir);
    if !path.exists() {
        return Ok(DEFAULT_SUFFIX.to_string());
    }
    let bytes = std::fs::read(&path).map_err(|err| AppError::Storage(format!("failed to read dns_config.json: {err}")))?;
    let config: DnsConfig = serde_json::from_slice(&bytes).map_err(|err| AppError::Storage(format!("dns_config.json is corrupt: {err}")))?;
    Ok(config.suffix)
}

pub fn save_dns_suffix(config_dir: &Path, suffix: &str) -> AppResult<()> {
    std::fs::create_dir_all(config_dir).map_err(|err| AppError::Storage(format!("failed to create the config directory: {err}")))?;
    let config = DnsConfig { suffix: suffix.to_string() };
    let bytes = serde_json::to_vec_pretty(&config).map_err(|err| AppError::Storage(format!("failed to encode dns_config.json: {err}")))?;
    std::fs::write(config_path(config_dir), bytes).map_err(|err| AppError::Storage(format!("failed to write dns_config.json: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_config_file_falls_back_to_the_default_suffix() {
        let dir = std::env::temp_dir().join(format!("vibessh-dns-config-test-{}", uuid::Uuid::new_v4()));
        assert_eq!(load_dns_suffix(&dir).unwrap(), DEFAULT_SUFFIX);
    }

    #[test]
    fn saving_then_loading_round_trips_a_custom_suffix() {
        let dir = std::env::temp_dir().join(format!("vibessh-dns-config-test-{}", uuid::Uuid::new_v4()));
        save_dns_suffix(&dir, ".internal").unwrap();
        assert_eq!(load_dns_suffix(&dir).unwrap(), ".internal");
        std::fs::remove_dir_all(&dir).ok();
    }
}
