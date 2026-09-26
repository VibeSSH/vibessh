//! A live stream of an Application's CPU and memory.
//!
//! Its own module rather than a corner of `logs`: it rides the same
//! `follow_command` channel, but what comes back is a reading, not output,
//! and the two are started, stopped and registered independently - see
//! `state::StatsFollowManager` for why sharing the log registry would have
//! been a bug.

use std::sync::Arc;

use serde::Serialize;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::RuntimeType;
use crate::runtime::docker::parse_stats_stream_line;
use crate::runtime::local_process::LocalProcessManager;
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::server_repository::ServerRepository;

use super::*;

/// One reading off the stream. Uptime is not in it - `docker stats` does not
/// report one - so the page keeps its slower poll for that.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsSample {
    pub cpu_percent: Option<f32>,
    pub ram_bytes: Option<u64>,
}

/// Starts following an Application's resource usage.
///
/// Docker over SSH only, the same boundary as `follow_application_logs` and
/// for the same reason: `docker stats` is a stream the Node produces for us,
/// and nothing else here has one to attach to. Anything else is
/// `InvalidInput`, and the page keeps polling as it always did - the
/// fallback is the old behaviour, not a failure.
pub async fn follow_application_stats(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
    mut on_sample: impl FnMut(StatsSample) + Send + 'static,
    on_closed: impl FnOnce(Option<String>) + Send + 'static,
) -> AppResult<crate::ssh::client::FollowHandle> {
    let (detail, connection, _runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
    if detail.application.runtime_type != RuntimeType::Docker {
        return Err(AppError::InvalidInput("live resource usage is only available for Docker applications".to_string()));
    }
    // `docker stats` takes several containers, so a member's account has no
    // rule for it - see `runtime::member`. The page falls back to polling,
    // which for a shared Application reports uptime.
    if detail.shared.is_some() {
        return Err(AppError::InvalidInput("live resource usage isn't available for a shared application".to_string()));
    }
    let connection = connection.ok_or_else(|| AppError::InvalidInput("live resource usage needs a connection to the Node".to_string()))?;
    let container = format!("vibessh-app-{id}");
    connection
        .follow_container_stats(
            &container,
            move |line| {
                if let Some((cpu_percent, ram_bytes)) = parse_stats_stream_line(&line) {
                    on_sample(StatsSample { cpu_percent, ram_bytes });
                }
            },
            on_closed,
        )
        .await
}
