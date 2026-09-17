//! Whether closing the window quits VibeSSH or leaves it running in the
//! tray - a plain JSON file in the app config dir, the same load-or-create
//! shape `ai_config` and `cloud_config` already use.
//!
//! **Why "closing quits" is not simply the answer.** VibeSSH holds live SSH
//! sessions, port forwards and log follows. Closing the window to get it off
//! the screen and thereby dropping every one of them is a surprise in the
//! other direction, and the one people hit first.
//!
//! **So the surprise is spent once, deliberately.** The first time the window
//! is closed to the tray, a notification says so. An application that keeps
//! running after you closed it and says nothing is indistinguishable from one
//! that failed to quit - and this one holds keys to your servers, which makes
//! "is it still running?" a question the user is entitled to have answered
//! without opening Task Manager.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

const CONFIG_FILE_NAME: &str = "tray_config.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayConfig {
    /// `true` (the default) means the close button hides the window and
    /// leaves the tray icon behind; `false` means it quits, as it always did.
    #[serde(default = "default_true")]
    pub minimize_to_tray: bool,
    /// Whether the "it is still running" notification has been shown. Kept
    /// here rather than in memory so it is once per installation, not once
    /// per launch - the point is to tell somebody something they do not know
    /// yet, and after the first time they know.
    #[serde(default)]
    pub notice_shown: bool,
}

fn default_true() -> bool {
    true
}

impl Default for TrayConfig {
    fn default() -> Self {
        Self { minimize_to_tray: true, notice_shown: false }
    }
}

fn config_path(config_dir: &Path) -> PathBuf {
    config_dir.join(CONFIG_FILE_NAME)
}

/// A missing file means a fresh install, which is a normal state rather than
/// an error. A *corrupt* one is different from every other config in this
/// app: the others refuse to load, because guessing what somebody's AI
/// endpoint or backup destination was is worse than saying so. Here the
/// stake is one boolean about a close button, and refusing to start over it
/// would be the larger fault - so a broken file falls back to the default
/// and says so in the log.
pub fn load_tray_config(config_dir: &Path) -> TrayConfig {
    let path = config_path(config_dir);
    if !path.exists() {
        return TrayConfig::default();
    }
    match std::fs::read(&path).map(|bytes| serde_json::from_slice::<TrayConfig>(&bytes)) {
        Ok(Ok(config)) => config,
        Ok(Err(err)) => {
            log::warn!("tray_config.json is corrupt, using defaults: {err}");
            TrayConfig::default()
        }
        Err(err) => {
            log::warn!("couldn't read tray_config.json, using defaults: {err}");
            TrayConfig::default()
        }
    }
}

pub fn save_tray_config(config_dir: &Path, config: &TrayConfig) -> AppResult<()> {
    std::fs::create_dir_all(config_dir).map_err(|err| AppError::Storage(format!("failed to create the config directory: {err}")))?;
    let bytes = serde_json::to_vec_pretty(config).map_err(|err| AppError::Storage(format!("failed to encode tray_config.json: {err}")))?;
    std::fs::write(config_path(config_dir), bytes).map_err(|err| AppError::Storage(format!("failed to write tray_config.json: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vibessh-tray-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The answer the user chose: closing leaves it in the tray, and the
    /// explanation has not been given yet.
    #[test]
    fn a_fresh_install_minimizes_to_the_tray_and_still_owes_an_explanation() {
        let dir = temp_dir();
        let config = load_tray_config(&dir);
        assert!(config.minimize_to_tray);
        assert!(!config.notice_shown);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn what_is_saved_is_what_comes_back() {
        let dir = temp_dir();
        let saved = TrayConfig { minimize_to_tray: false, notice_shown: true };
        save_tray_config(&dir, &saved).unwrap();
        assert_eq!(load_tray_config(&dir), saved);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A file written before `noticeShown` existed must not read as "the
    /// explanation was already given" - that would silently swallow the one
    /// notification this whole design rests on.
    #[test]
    fn a_file_from_before_the_notice_existed_still_owes_the_explanation() {
        let dir = temp_dir();
        std::fs::write(dir.join(CONFIG_FILE_NAME), br#"{"minimizeToTray":false}"#).unwrap();
        let config = load_tray_config(&dir);
        assert!(!config.minimize_to_tray, "the setting that was in the file must survive");
        assert!(!config.notice_shown);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Every other config in this app refuses to load when corrupt. This one
    /// is a boolean about a close button, and refusing to start over it would
    /// be worse than the fault it is reporting.
    #[test]
    fn a_corrupt_file_falls_back_instead_of_stopping_the_app() {
        let dir = temp_dir();
        std::fs::write(dir.join(CONFIG_FILE_NAME), b"{not json at all").unwrap();
        assert_eq!(load_tray_config(&dir), TrayConfig::default());
        std::fs::remove_dir_all(&dir).ok();
    }
}
