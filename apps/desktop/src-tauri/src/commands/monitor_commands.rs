use tauri::State;
use uuid::Uuid;

use crate::errors::AppResult;
use crate::services;
use crate::state::SshSessionManager;
use crate::storage::server_repository::ServerRepository;
use crate::transport::{ProcessSummary, ServerMetrics};

#[tauri::command]
pub async fn get_server_metrics(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
) -> AppResult<ServerMetrics> {
    services::get_server_metrics(&repo, &sessions, server_id).await
}

#[tauri::command]
pub async fn list_server_processes(
    repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Uuid,
) -> AppResult<Vec<ProcessSummary>> {
    services::list_server_processes(&repo, &sessions, server_id).await
}

/// Starts a live metrics stream for one SSH-mode server; the interface then
/// receives `metrics://<server_id>` events until it calls `stop_metrics_stream`.
/// Idempotent - a second call while a stream is running does nothing.
#[tauri::command]
pub fn start_metrics_stream(app: tauri::AppHandle, streams: State<'_, crate::state::MetricsStreamManager>, server_id: Uuid) {
    streams.start(app, server_id);
}

/// Stops the live metrics stream for one server.
#[tauri::command]
pub fn stop_metrics_stream(streams: State<'_, crate::state::MetricsStreamManager>, server_id: Uuid) {
    streams.stop(server_id);
}
