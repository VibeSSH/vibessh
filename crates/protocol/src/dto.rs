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
    /// What the Node calls itself - `PRETTY_NAME` from `/etc/os-release`,
    /// e.g. "Ubuntu 24.04.1 LTS".
    ///
    /// Optional because it genuinely can be absent: a container image
    /// without the file, a distribution that does not ship it, or an older
    /// Agent that predates this field. A missing name is shown as nothing
    /// rather than as a guess.
    #[serde(default)]
    pub os_name: Option<String>,
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
    /// POSIX mode bits (e.g. `0o755`), when the source actually reports
    /// them - `None` rather than a fabricated value on a provider that
    /// doesn't have a meaningful concept of Unix permissions (Local on
    /// Windows, for one).
    #[serde(default)]
    pub permissions: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSummary {
    pub name: String,
    pub active: bool,
    pub enabled: bool,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerSummary {
    /// Short (12-char) container ID, exactly as `docker ps` shows it -
    /// restart accepts this or `name` interchangeably, same as the `docker`
    /// CLI itself does.
    pub id: String,
    pub name: String,
    pub image: String,
    /// Docker's own human-readable status text, e.g. "Up 3 hours" or
    /// "Exited (0) 2 days ago" - not reparsed into a duration, since the
    /// exact wording is more informative than a normalized number here.
    pub status: String,
    pub running: bool,
}

/// One poll of a Minecraft server's own health - TPS, tick time and who is
/// online - read over RCON through the SSH tunnel, never a public link the
/// way spark's web view works. `ServerMetrics` above is the host (CPU, RAM);
/// this is the game server running on it, which only the JVM can report.
///
/// Every field is what the server itself said, parsed from `/tps`, `/mspt`
/// and `/list`. A server that answers `/list` but not `/tps` (a Spigot build
/// without the Paper commands) still fills the player fields, so the TPS ones
/// are optional rather than a fabricated 20.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinecraftMetrics {
    /// Ticks per second over the last 1m, 5m, 15m. 20 is healthy. `None` when
    /// the server has no `/tps` command (not Paper/Purpur), because a guessed
    /// number here is worse than an honest gap.
    pub tps_1m: Option<f32>,
    pub tps_5m: Option<f32>,
    pub tps_15m: Option<f32>,
    /// Milliseconds per tick over the shortest window the server reports -
    /// avg and max. Max catches a single bad tick that the smoothed TPS hides.
    pub mspt_avg: Option<f32>,
    pub mspt_max: Option<f32>,
    pub players_online: u32,
    pub players_max: u32,
    pub player_names: Vec<String>,
}
