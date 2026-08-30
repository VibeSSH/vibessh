use serde::{Deserialize, Serialize};

/// Result of a one-shot command execution, regardless of which transport ran it.
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
pub struct ProcessInfo {
    pub pid: u32,
    pub user: String,
    pub cpu_percent: f32,
    pub ram_bytes: u64,
    pub command: String,
}
