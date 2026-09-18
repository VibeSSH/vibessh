//! Whether VibeSSH opens a local door for Claude, and on which port.
//!
//! **Off by default, and that is not timidity.** This door lets something
//! outside VibeSSH ask about your servers. Even read-only, that is a listening
//! socket on a machine that holds keys to production, so it exists only where
//! somebody has said they want it - the same stance the rest of this app takes
//! about anything that widens what can reach in.
//!
//! **Loopback only, always.** There is no setting for the bind address,
//! because there is no version of this worth exposing to a network: an
//! attacker on your LAN is not somebody who should be able to list your
//! servers, and "it is behind my router" is how that ends up true anyway.
//! The port is configurable only because 7422 may already be taken.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

const CONFIG_FILE_NAME: &str = "mcp_config.json";

/// Next to the agent's own 7420/7421, so the three VibeSSH ports sit
/// together and none of them is a number somebody has to remember out of
/// context.
pub const DEFAULT_PORT: u16 = 7422;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_port")]
    pub port: u16,
    /// Whether the tools that change something are offered at all.
    ///
    /// Separate from `enabled` because the two questions are genuinely
    /// different: "may Claude see my servers" and "may Claude restart them"
    /// have different answers for most people, and folding them into one
    /// switch would make the safe choice unavailable.
    #[serde(default)]
    pub allow_changes: bool,
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

impl Default for McpConfig {
    fn default() -> Self {
        Self { enabled: false, port: DEFAULT_PORT, allow_changes: false }
    }
}

fn config_path(config_dir: &Path) -> PathBuf {
    config_dir.join(CONFIG_FILE_NAME)
}

/// A missing or unreadable file means "not set up", which for this feature
/// is the same as "off" - and off is the safe answer, so a corrupt file
/// closes the door rather than leaving it open on defaults.
pub fn load_mcp_config(config_dir: &Path) -> McpConfig {
    let path = config_path(config_dir);
    if !path.exists() {
        return McpConfig::default();
    }
    match std::fs::read(&path).map(|bytes| serde_json::from_slice::<McpConfig>(&bytes)) {
        Ok(Ok(config)) => config,
        Ok(Err(err)) => {
            log::warn!("mcp_config.json is corrupt, leaving the local endpoint off: {err}");
            McpConfig::default()
        }
        Err(err) => {
            log::warn!("couldn't read mcp_config.json, leaving the local endpoint off: {err}");
            McpConfig::default()
        }
    }
}

pub fn save_mcp_config(config_dir: &Path, config: &McpConfig) -> AppResult<()> {
    std::fs::create_dir_all(config_dir).map_err(|err| AppError::Storage(format!("failed to create the config directory: {err}")))?;
    let bytes = serde_json::to_vec_pretty(config).map_err(|err| AppError::Storage(format!("failed to encode mcp_config.json: {err}")))?;
    std::fs::write(config_path(config_dir), bytes).map_err(|err| AppError::Storage(format!("failed to write mcp_config.json: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vibessh-mcp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The whole premise: a fresh install opens no port and grants nothing.
    #[test]
    fn a_fresh_install_is_off_and_read_only() {
        let dir = temp_dir();
        let config = load_mcp_config(&dir);
        assert!(!config.enabled);
        assert!(!config.allow_changes);
        assert_eq!(config.port, DEFAULT_PORT);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn what_is_saved_is_what_comes_back() {
        let dir = temp_dir();
        let saved = McpConfig { enabled: true, port: 9999, allow_changes: true };
        save_mcp_config(&dir, &saved).unwrap();
        assert_eq!(load_mcp_config(&dir), saved);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Unlike the tray's config, a broken file here must not fall back to
    /// something permissive - and the default happens to be the closed door,
    /// which is exactly why the default is what it is.
    #[test]
    fn a_corrupt_file_closes_the_door_rather_than_guessing() {
        let dir = temp_dir();
        std::fs::write(dir.join(CONFIG_FILE_NAME), b"{ this is not json").unwrap();
        let config = load_mcp_config(&dir);
        assert!(!config.enabled, "a file nobody can read must never leave the endpoint listening");
        assert!(!config.allow_changes);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// An older file, written before changes could be allowed at all, must
    /// not read as permission to make them.
    #[test]
    fn a_file_from_before_changes_existed_grants_none() {
        let dir = temp_dir();
        std::fs::write(dir.join(CONFIG_FILE_NAME), br#"{"enabled":true,"port":7422}"#).unwrap();
        let config = load_mcp_config(&dir);
        assert!(config.enabled);
        assert!(!config.allow_changes);
        std::fs::remove_dir_all(&dir).ok();
    }
}
