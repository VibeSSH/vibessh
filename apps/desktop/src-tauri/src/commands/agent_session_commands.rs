use tauri::{Manager, State};
use uuid::Uuid;

use crate::agent_client::{AgentClientConfig, AgentConnectionState};
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
    app: tauri::AppHandle,
    sessions: State<'_, AgentSessionManager>,
    server_repo: State<'_, ServerRepository>,
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
        // Trust on first use: NULL until a handshake succeeds, then every
        // later connection is checked against it. See
        // `AgentClientConfig::known_fingerprint`.
        known_fingerprint: server_repo.get(server_id)?.and_then(|server| server.agent_certificate_fingerprint),
    };
    sessions.ensure_connected(server_id, config).await;

    // Trust on first use. The fingerprint is only known once the handshake
    // succeeds, so this waits for the first `Connected` and records it.
    // `pin_agent_certificate_fingerprint` only writes when the column is
    // still NULL - a *different* fingerprint is rejected by
    // `agent_client` before the credential is ever sent, and must never be
    // quietly written over the old one here.
    if let Some(mut states) = sessions.state_updates(server_id).await {
        // `AppHandle` rather than the `State` reference: this outlives the
        // command, and Tauri-managed state can only be borrowed for the
        // command's own lifetime.
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            while states.changed().await.is_ok() {
                let fingerprint = match &*states.borrow() {
                    AgentConnectionState::Connected { certificate_fingerprint: Some(fingerprint), .. } => fingerprint.clone(),
                    _ => continue,
                };
                if let Err(err) = app.state::<ServerRepository>().pin_agent_certificate_fingerprint(server_id, &fingerprint) {
                    log::warn!("couldn't record the agent certificate fingerprint for {server_id}: {err}");
                }
                return;
            }
        });
    }
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
