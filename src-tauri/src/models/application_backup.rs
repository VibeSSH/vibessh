//! Application backups - a point-in-time `.zip` of an Application's working
//! directory, written through the same `ApplicationFileProvider` the Files
//! tab already uses (Local or Remote/SSH, whichever the Application itself
//! is) rather than a separate storage mechanism. See
//! `storage::migrations`'s own doc comment on the two backing tables for why
//! this is metadata-plus-a-file rather than a blob column.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackupKind {
    Manual,
    Scheduled,
}

impl BackupKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BackupKind::Manual => "manual",
            BackupKind::Scheduled => "scheduled",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "scheduled" => BackupKind::Scheduled,
            _ => BackupKind::Manual,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationBackup {
    pub id: Uuid,
    pub application_id: Uuid,
    /// Relative to `.vibessh-backups/` inside the Application's own working
    /// directory - never a full path, since where that directory actually
    /// lives (a Local path vs. a Remote host's own filesystem) is exactly
    /// what `ApplicationFileProvider` already abstracts away.
    pub file_name: String,
    pub size_bytes: u64,
    pub kind: BackupKind,
    /// `Some` when this backup was also uploaded to the configured S3-compatible
    /// destination (see `models::BackupDestinationConfig`) - the object key
    /// it was written under (before the destination's own `path_prefix` is
    /// applied, see `s3::S3Client::prefixed_key`), so `restore_backup` can
    /// fetch it back even if the local `.vibessh-backups/` copy is gone
    /// (a rebuilt Node, a wiped disk - exactly the failure a local-only
    /// backup can't survive). `None` means this backup only ever existed
    /// locally, either because no destination was configured at the time,
    /// or the upload itself failed.
    pub s3_key: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// What `set_backup_schedule` submits - absence of a stored row (rather than
/// `enabled: false`) is what "never configured" looks like, see
/// `storage::migrations`'s own doc comment.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetBackupScheduleInput {
    pub enabled: bool,
    pub interval_hours: u32,
    pub retention_count: u32,
    /// `None` = no age-based pruning. Applied independently of
    /// `retention_count` - a backup is pruned once it fails *either* rule,
    /// see `services::application_backup_service::prune_old_backups`'s own
    /// doc comment.
    pub retention_max_age_days: Option<u32>,
    /// `None` = no total-size cap. Oldest-first, same as the other two
    /// rules - once the running total of *remaining* backups would exceed
    /// this, the oldest ones are pruned until it doesn't.
    pub retention_max_total_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSchedule {
    pub enabled: bool,
    pub interval_hours: u32,
    pub retention_count: u32,
    pub retention_max_age_days: Option<u32>,
    pub retention_max_total_bytes: Option<u64>,
}

impl Default for BackupSchedule {
    /// What every Application reads as before it has a stored schedule row -
    /// `enabled: false` so this is inert until the user actually turns it
    /// on, matching `enabled`'s own `DEFAULT 0` at the schema level.
    fn default() -> Self {
        Self { enabled: false, interval_hours: 24, retention_count: 5, retention_max_age_days: None, retention_max_total_bytes: None }
    }
}
