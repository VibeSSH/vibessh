//! In-memory copy of the S3-compatible backup destination's non-secret
//! settings (`models::BackupDestinationConfig`) - loaded once from
//! `storage::backup_destination_config` at startup, kept live here so
//! `services::application_backup_service` never has to re-read the config
//! file on every single backup. Persisting a change is the caller's own
//! job (`services::set_backup_destination` writes the file *and* calls
//! `set` here) - same split `state::CloudState`'s own
//! `set_backend_url`/the settings command that calls `save_backend_url`
//! already uses.

use tokio::sync::Mutex;

use crate::models::BackupDestinationConfig;

pub struct BackupDestinationState {
    inner: Mutex<BackupDestinationConfig>,
}

impl BackupDestinationState {
    pub fn new(config: BackupDestinationConfig) -> Self {
        Self { inner: Mutex::new(config) }
    }

    pub async fn get(&self) -> BackupDestinationConfig {
        self.inner.lock().await.clone()
    }

    pub async fn set(&self, config: BackupDestinationConfig) {
        *self.inner.lock().await = config;
    }
}
