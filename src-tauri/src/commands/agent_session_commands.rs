use tauri::State;
use uuid::Uuid;

use crate::agent_client::AgentClientConfig;
use crate::errors::AppResult;
use crate::models::{NodeSyncStatus, ReconcileOutcome};
use crate::services;
use crate::state::AgentSessionManager;
use crate::storage::node_state_repository::NodeStateRepository;
use crate::storage::server_repository::ServerRepository;

/// Starts (or confirms already-running) the persistent Etap M3 connection
/// for a just-paired Node - called by the frontend right after
/// `upsert_agent_server` succeeds, while it still has `host`/`port` and the
/// freshly issued credential in hand (see `AgentSessionManager`'s own doc
/// comment for why that exact moment is the only one this can hook into
/// today).
#[tauri::command]
pub async fn start_agent_session(
    sessions: State<'_, AgentSessionManager>,
    server_id: Uuid,
    host: String,
    port: u16,
    auth_token: String,
) -> AppResult<()> {
    let config = AgentClientConfig {
        url: format!("wss://{host}:{port}/ws"),
        client_name: "VibeSSH Desktop".to_string(),
        client_version: env!("CARGO_PKG_VERSION").to_string(),
        auth_token: Some(auth_token),
    };
    sessions.ensure_connected(server_id, config).await;
    Ok(())
}

#[tauri::command]
pub fn get_node_sync_status(node_repo: State<NodeStateRepository>, server_id: Uuid) -> AppResult<NodeSyncStatus> {
    services::node_sync_status(&node_repo, server_id)
}

#[tauri::command]
pub async fn reconcile_agent_node(
    node_repo: State<'_, NodeStateRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, AgentSessionManager>,
    server_id: Uuid,
) -> AppResult<ReconcileOutcome> {
    services::reconcile_node(&node_repo, &server_repo, &sessions, server_id).await
}
