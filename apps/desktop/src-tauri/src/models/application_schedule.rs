//! Scheduled power actions for an Application - see
//! `services::schedule_service` for where they run and why there.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};

/// What a schedule does when it fires. Power actions only, on purpose: these
/// are the ones the Node can carry out on its own with nothing but `docker`,
/// which is what lets a schedule run while VibeSSH is closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScheduleAction {
    Start,
    Stop,
    Restart,
}

impl ScheduleAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
        }
    }

    pub fn parse(value: &str) -> AppResult<Self> {
        match value {
            "start" => Ok(Self::Start),
            "stop" => Ok(Self::Stop),
            "restart" => Ok(Self::Restart),
            other => Err(AppError::Storage(format!("unknown schedule action '{other}'"))),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationSchedule {
    pub id: Uuid,
    pub application_id: Uuid,
    pub name: String,
    /// Five cron fields - minute, hour, day of month, month, day of week - in
    /// the Node's own time zone, since the Node's cron is what reads them.
    pub cron: String,
    pub action: ScheduleAction,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleInput {
    pub name: String,
    pub cron: String,
    pub action: ScheduleAction,
    pub enabled: bool,
}

/// The last time a schedule actually ran on the Node, as the Node recorded it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleRun {
    pub schedule_id: Uuid,
    pub ran_at: DateTime<Utc>,
    pub action: String,
    pub exit_code: i32,
    /// What `docker` printed, trimmed - the reason when it failed.
    pub message: String,
}

/// The Node's clock, which is the one a cron expression is read against.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeTimeZone {
    /// `Europe/Warsaw`, or `None` when the Node does not say.
    pub name: Option<String>,
    /// Minutes east of UTC right now, from `date +%z`.
    pub offset_minutes: i32,
}

/// What the Node's last disk check found - see `schedule_service::set_disk_limit`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsage {
    pub checked_at: DateTime<Utc>,
    pub used_bytes: u64,
    pub limit_bytes: u64,
    /// Whether that check stopped the server for being over the limit.
    pub stopped: bool,
}

/// Everything the Schedules tab shows, fetched in one call.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationSchedules {
    pub schedules: Vec<ApplicationSchedule>,
    pub last_runs: Vec<ScheduleRun>,
    pub time_zone: Option<NodeTimeZone>,
    /// Why the Node could not be asked, when it could not - the error in the
    /// same shape a failed command returns, so the interface translates it
    /// (and a changed host key reads as the warning it is) instead of showing
    /// English. The schedules themselves still come from the local database.
    pub node_error: Option<serde_json::Value>,
}
