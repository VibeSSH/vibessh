//! Etap M3: desired-state revisioning and the "Reconcile" action. See
//! `state::AgentSessionManager`'s own doc comment for the connection this
//! builds on, and `vibessh_protocol::NodeDesiredState`'s for why the actual
//! payload is still empty this phase - what's real here is the
//! revisioning/reconcile *mechanism*, not yet anything it pushes.

use std::time::Duration;

use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{ConnectionMode, NodeSyncStatus, ReconcileOutcome};
use crate::state::AgentSessionManager;
use crate::storage::node_state_repository::NodeStateRepository;
use crate::storage::server_repository::ServerRepository;
use vibessh_protocol::{DesktopCommand, NodeDesiredState};

/// How long a reconcile waits for the Agent's ack before giving up and
/// reporting `OfflinePending` - generous enough for a slow network hop,
/// short enough that a UI click doesn't hang indefinitely against a Node
/// that's connected but wedged.
const ACK_TIMEOUT: Duration = Duration::from_secs(10);

pub fn sync_status(node_repo: &NodeStateRepository, server_id: Uuid) -> AppResult<NodeSyncStatus> {
    node_repo.sync_status(server_id)
}

/// Bumps the desired revision, pushes it to the Node if a live session
/// exists, and waits for a real ack - never reports `Applied` without one.
/// SSH-mode Nodes have no persistent session concept in this phase (see
/// `AgentSessionManager`'s own doc comment) and are rejected outright
/// rather than silently no-oping, matching the "don't pretend two
/// connection modes have identical capabilities" stance already applied to
/// runtime types (`services::set_application_resource_limits`) and Docker
/// availability (`runtime::docker::DockerRuntime::validate`).
pub async fn reconcile_node(
    node_repo: &NodeStateRepository,
    server_repo: &ServerRepository,
    sessions: &AgentSessionManager,
    server_id: Uuid,
) -> AppResult<ReconcileOutcome> {
    let server = server_repo.get(server_id)?.ok_or_else(|| AppError::NotFound(format!("server {server_id}")))?;
    if server.connection_mode != ConnectionMode::Agent {
        return Err(AppError::InvalidInput("reconcile is only available for Agent-mode Nodes".into()));
    }

    let revision = node_repo.bump_desired_revision(server_id)?;
    let command = DesktopCommand::ApplyDesiredState { revision, state: NodeDesiredState::default() };

    if !sessions.send_command(server_id, command).await {
        node_repo.record_reconcile_failure(server_id, "offline_pending", "no live connection to this Node")?;
        return Ok(ReconcileOutcome::OfflinePending { desired_revision: revision });
    }

    match sessions.await_ack(server_id, revision, ACK_TIMEOUT).await {
        Some(ack) if ack.ok => {
            node_repo.set_applied(server_id, revision, "applied", None)?;
            Ok(ReconcileOutcome::Applied { revision })
        }
        Some(ack) => {
            let error = ack.error.unwrap_or_else(|| "the Node reported a failure with no further detail".to_string());
            node_repo.record_reconcile_failure(server_id, "failed", &error)?;
            Ok(ReconcileOutcome::Failed { revision, error: Some(error) })
        }
        None => {
            let error = "no acknowledgement within the wait window".to_string();
            node_repo.record_reconcile_failure(server_id, "offline_pending", &error)?;
            Ok(ReconcileOutcome::OfflinePending { desired_revision: revision })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AuthenticationType, ServerInput};

    fn temp_setup() -> (NodeStateRepository, ServerRepository, AgentSessionManager) {
        let path = std::env::temp_dir().join(format!("vibessh-node-state-service-test-{}.sqlite3", Uuid::new_v4()));
        (NodeStateRepository::open(&path).unwrap(), ServerRepository::open(&path).unwrap(), AgentSessionManager::new())
    }

    fn ssh_input() -> ServerInput {
        ServerInput {
            name: "SSH Node".into(),
            host: "203.0.113.10".into(),
            ssh_port: 22,
            username: "root".into(),
            authentication_type: AuthenticationType::Password,
            private_key_path: None,
            group_id: None,
            password: Some("x".into()),
            key_passphrase: None,
        }
    }

    #[tokio::test]
    async fn reconcile_rejects_an_ssh_mode_node() {
        let (node_repo, server_repo, sessions) = temp_setup();
        let server = server_repo.create(&ssh_input()).unwrap();

        let err = reconcile_node(&node_repo, &server_repo, &sessions, server.id).await.unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn reconcile_reports_offline_pending_for_an_agent_node_with_no_live_session() {
        let (node_repo, server_repo, sessions) = temp_setup();
        let server = server_repo.upsert_agent("Agent Node", "203.0.113.20", Uuid::new_v4(), None).unwrap();

        let outcome = reconcile_node(&node_repo, &server_repo, &sessions, server.id).await.unwrap();
        assert!(matches!(outcome, ReconcileOutcome::OfflinePending { desired_revision: 1 }));

        let status = node_repo.sync_status(server.id).unwrap();
        assert_eq!(status.desired_revision, 1);
        assert!(!status.in_sync, "recording an offline attempt must not fabricate a successful sync");
    }

    #[tokio::test]
    async fn reconcile_of_an_unknown_server_is_not_found() {
        let (node_repo, server_repo, sessions) = temp_setup();
        let err = reconcile_node(&node_repo, &server_repo, &sessions, Uuid::new_v4()).await.unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }
}
