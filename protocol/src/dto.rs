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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSummary {
    pub name: String,
    pub active: bool,
    pub enabled: bool,
    pub description: String,
}
