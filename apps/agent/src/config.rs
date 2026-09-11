use std::path::PathBuf;

/// Where the agent keeps its persistent identity and (later) its local
/// configuration file. Etap G (systemd) pins these to `/var/lib/vibessh/agent`
/// and `/etc/vibessh/agent` in production via the unit file's `Environment=`;
/// here they default to a per-user data dir so `cargo run` works without
/// root during development.
pub struct AgentConfig {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
}

impl AgentConfig {
    pub fn resolve() -> Self {
        let data_dir = std::env::var("VIBESSH_AGENT_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| default_data_dir());

        let config_dir = std::env::var("VIBESSH_AGENT_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| default_config_dir());

        Self {
            data_dir,
            config_dir,
        }
    }
}

fn default_data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("vibessh-agent")
}

fn default_config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("vibessh-agent")
}
