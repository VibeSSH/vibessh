//! The assistant's non-secret settings, as a plain JSON file in the app
//! config dir - the same load-or-create shape `cloud_config` and
//! `backup_destination_config` already use.
//!
//! The API key is not in this file and never will be. It lives in the OS
//! credential store (`storage::credentials::store_ai_api_key`), for the
//! same reason every other secret in this app does: a config file is
//! world-readable to anything running as this user, gets copied into
//! backups, and shows up in screenshots.

use std::path::{Path, PathBuf};

use crate::errors::{AppError, AppResult};
use crate::models::AiConfig;

const CONFIG_FILE_NAME: &str = "ai_config.json";

fn config_path(config_dir: &Path) -> PathBuf {
    config_dir.join(CONFIG_FILE_NAME)
}

/// A missing file means the assistant has never been configured, which is
/// a normal state rather than an error - `AiConfig::default()` is disabled
/// with no endpoint, which is exactly right for a fresh install.
pub fn load_ai_config(config_dir: &Path) -> AppResult<AiConfig> {
    let path = config_path(config_dir);
    if !path.exists() {
        return Ok(AiConfig::default());
    }
    let bytes = std::fs::read(&path).map_err(|err| AppError::Storage(format!("failed to read ai_config.json: {err}")))?;
    serde_json::from_slice(&bytes).map_err(|err| AppError::Storage(format!("ai_config.json is corrupt: {err}")))
}

pub fn save_ai_config(config_dir: &Path, config: &AiConfig) -> AppResult<()> {
    std::fs::create_dir_all(config_dir).map_err(|err| AppError::Storage(format!("failed to create the config directory: {err}")))?;
    let bytes = serde_json::to_vec_pretty(config).map_err(|err| AppError::Storage(format!("failed to encode ai_config.json: {err}")))?;
    std::fs::write(config_path(config_dir), bytes).map_err(|err| AppError::Storage(format!("failed to write ai_config.json: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AiProviderKind;

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("vibessh-ai-config-test-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn a_fresh_install_reads_back_as_disabled_with_nothing_configured() {
        let config = load_ai_config(&temp_dir()).unwrap();
        assert!(!config.enabled);
        assert!(config.base_url.is_empty());
        assert!(config.model.is_empty());
    }

    #[test]
    fn saving_then_loading_round_trips_every_field() {
        let dir = temp_dir();
        let config = AiConfig {
            enabled: true,
            provider: AiProviderKind::OpenAiCompatible,
            base_url: "https://openrouter.ai/api/v1".to_string(),
            model: "meta-llama/llama-3.3-70b-instruct".to_string(),
        };
        save_ai_config(&dir, &config).unwrap();
        let loaded = load_ai_config(&dir).unwrap();
        assert!(loaded.enabled);
        assert_eq!(loaded.base_url, "https://openrouter.ai/api/v1");
        assert_eq!(loaded.model, "meta-llama/llama-3.3-70b-instruct");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The file is the one place someone might look for a leaked key, so
    /// this asserts the absence rather than trusting the struct definition
    /// to stay that way.
    #[test]
    fn the_written_file_contains_no_key_field_at_all() {
        let dir = temp_dir();
        let config = AiConfig {
            enabled: true,
            provider: AiProviderKind::OpenAiCompatible,
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4o-mini".to_string(),
        };
        save_ai_config(&dir, &config).unwrap();
        let written = std::fs::read_to_string(config_path(&dir)).unwrap().to_lowercase();
        assert!(!written.contains("apikey"));
        assert!(!written.contains("api_key"));
        assert!(!written.contains("sk-"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
