//! Where the S3-compatible backup destination's non-secret settings live -
//! a plain JSON file in the app config dir, same load-or-create pattern
//! `cloud_config.rs` already uses for the cloud backend URL (a single,
//! global, per-install setting, not per-Application data that belongs in
//! the SQLite store). The secret access key itself never touches this
//! file - see `credentials::store_backup_destination_secret`.

use std::path::{Path, PathBuf};

use crate::errors::{AppError, AppResult};
use crate::models::BackupDestinationConfig;

const CONFIG_FILE_NAME: &str = "backup_destination.json";

fn config_path(config_dir: &Path) -> PathBuf {
    config_dir.join(CONFIG_FILE_NAME)
}

/// `BackupDestinationConfig::default()` (`enabled: false`, everything else
/// empty) when nothing has been configured yet - the same "absent means
/// unset, not an error" shape every other optional per-install setting in
/// this codebase already uses.
pub fn load_backup_destination(config_dir: &Path) -> AppResult<BackupDestinationConfig> {
    let path = config_path(config_dir);
    if !path.exists() {
        return Ok(BackupDestinationConfig::default());
    }
    let bytes = std::fs::read(&path).map_err(|err| AppError::Storage(format!("failed to read backup_destination.json: {err}")))?;
    serde_json::from_slice(&bytes).map_err(|err| AppError::Storage(format!("backup_destination.json is corrupt: {err}")))
}

pub fn save_backup_destination(config_dir: &Path, config: &BackupDestinationConfig) -> AppResult<()> {
    std::fs::create_dir_all(config_dir).map_err(|err| AppError::Storage(format!("failed to create the config directory: {err}")))?;
    let bytes = serde_json::to_vec_pretty(config).map_err(|err| AppError::Storage(format!("failed to encode backup_destination.json: {err}")))?;
    std::fs::write(config_path(config_dir), bytes).map_err(|err| AppError::Storage(format!("failed to write backup_destination.json: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_config_file_falls_back_to_a_disabled_default() {
        let dir = std::env::temp_dir().join(format!("vibessh-backup-destination-test-{}", uuid::Uuid::new_v4()));
        let loaded = load_backup_destination(&dir).unwrap();
        assert!(!loaded.enabled);
        assert_eq!(loaded.endpoint, "");
    }

    #[test]
    fn saving_then_loading_round_trips_every_field() {
        let dir = std::env::temp_dir().join(format!("vibessh-backup-destination-test-{}", uuid::Uuid::new_v4()));
        let config = BackupDestinationConfig {
            enabled: true,
            endpoint: "https://s3.amazonaws.com".to_string(),
            region: "us-east-1".to_string(),
            bucket: "my-bucket".to_string(),
            access_key_id: "AKIAEXAMPLE".to_string(),
            path_prefix: "vibessh-backups".to_string(),
            path_style: false,
        };
        save_backup_destination(&dir, &config).unwrap();
        let loaded = load_backup_destination(&dir).unwrap();
        assert_eq!(loaded.enabled, config.enabled);
        assert_eq!(loaded.endpoint, config.endpoint);
        assert_eq!(loaded.region, config.region);
        assert_eq!(loaded.bucket, config.bucket);
        assert_eq!(loaded.access_key_id, config.access_key_id);
        assert_eq!(loaded.path_prefix, config.path_prefix);
        assert_eq!(loaded.path_style, config.path_style);
        std::fs::remove_dir_all(&dir).ok();
    }
}
