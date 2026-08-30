use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Result of a one-shot command execution. Used by `ServerConnection::execute_command`
/// for both transports: `SshTransport` builds one from the SSH exec channel's
/// output, `AgentTransport` builds one from the agent's HTTPS response — same
/// shape either way, so callers never need to know which one they got.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Point-in-time resource snapshot. SSH mode polls for this; Agent mode
/// receives it as a push (`metrics.update`) but normalizes to the same shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerMetrics {
    pub cpu_usage_percent: f32,
    pub ram_used_bytes: u64,
    pub ram_total_bytes: u64,
    pub disk_used_bytes: u64,
    pub disk_total_bytes: u64,
    pub load_average_1m: f32,
    pub uptime_seconds: u64,
    /// Rate since the previous sample, not a cumulative total - what a
    /// "realtime" dashboard actually wants to plot. Zero on the very first
    /// sample after a connection opens, since there's no prior point yet.
    pub network_rx_bytes_per_sec: u64,
    pub network_tx_bytes_per_sec: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessSummary {
    pub pid: u32,
    pub user: String,
    pub cpu_percent: f32,
    pub ram_bytes: u64,
    pub command: String,
}

/// One entry from a directory listing (`ServerConnection::list_directory`).
/// `path` is the full remote path, already joined with the directory that
/// was listed - callers never need to do their own path-joining to
/// navigate into a subdirectory or open a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub modified_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSummary {
    pub name: String,
    pub active: bool,
    pub enabled: bool,
    pub description: String,
}
