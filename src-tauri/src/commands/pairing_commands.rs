//! Tauri command bridge for Etap H's "Install Vibe Agent" flow. Thin on
//! purpose: the real work (handshake, reconnect, backoff) is
//! `agent_client::run`, already built and tested in Etap D/E. This module's
//! only job is starting/stopping that background task per app session and
//! forwarding its state to the frontend as events, since a long-running
//! background task can't just return a value the way a normal command does.
//!
//! `spawn_pairing_session` holds all of that logic and takes a plain
//! callback instead of an `AppHandle` specifically so it's testable with
//! nothing but a real tokio runtime - `tauri::test::MockRuntime` hits a
//! DLL-loading crash (STATUS_ENTRYPOINT_NOT_FOUND) in this project's dev
//! environment (Windows 10 19045); a plain `cargo build`d app binary runs
//! fine, so the fault is in that test harness on this OS build, not in our
//! code. The `#[tauri::command]` wrappers below stay a couple of lines of
//! glue that's easy to eyeball-verify instead of routing through it.

use tauri::{AppHandle, Emitter, State};
use tokio::sync::{mpsc, watch};

use crate::agent_client::{self, AgentClientConfig, AgentConnectionState};
use crate::state::PairingSession;
use crate::storage::credentials;
use vibessh_protocol::ServerEvent;

/// Frontend listens with `listen(PAIRING_STATE_EVENT, ...)` from
/// `@tauri-apps/api/event`. Payload is `AgentConnectionState`'s JSON shape.
const PAIRING_STATE_EVENT: &str = "agent-pairing://state";
/// Etap J: realtime data (currently just `metrics.update`) arriving while
/// the pairing flow's connection is open, so the "Connected" panel can show
/// live numbers instead of ending the moment a handshake succeeds. Payload
/// is `ServerEvent`'s JSON shape (heartbeats never reach here - see
/// `agent_client::run`, which swallows them before they reach this channel).
const PAIRING_EVENT_EVENT: &str = "agent-pairing://event";

#[tauri::command]
pub fn generate_pairing_code() -> String {
    vibessh_protocol::generate_pairing_code()
}

#[tauri::command]
pub fn pairing_code_ttl_seconds() -> i64 {
    vibessh_protocol::PAIRING_CODE_TTL.num_seconds()
}

/// Starts (or restarts, if one was already running - see
/// `PairingSession::replace`) `agent_client::run` in the background,
/// calling `on_state_change` every time its connection state changes and
/// `on_event` for every realtime data event (currently just
/// `metrics.update` - Etap J) that arrives on an open connection. Returns
/// immediately - callers don't block until connected.
pub fn spawn_pairing_session<F, E>(
    session: &PairingSession,
    config: AgentClientConfig,
    mut on_state_change: F,
    mut on_event: E,
) where
    F: FnMut(AgentConnectionState) + Send + 'static,
    E: FnMut(ServerEvent) + Send + 'static,
{
    let (events_tx, mut events_rx) = mpsc::channel(16);
    let (state_tx, mut state_rx) = watch::channel(AgentConnectionState::Connecting);

    tauri::async_runtime::spawn(async move {
        loop {
            if state_rx.changed().await.is_err() {
                return; // state_tx dropped - the run task ended or was aborted
            }
            let state = state_rx.borrow_and_update().clone();

            // Etap K: this used to be issued and then simply discarded once
            // the pairing modal closed - the OS-keyring storage existed and
            // was tested (Etap E) but nothing ever called it. Persisting it
            // here, not in the frontend, means every path that produces a
            // credential (this one today, a future "reconnect" flow
            // tomorrow) goes through the same one place.
            if let AgentConnectionState::Connected {
                agent_id,
                issued_credential: Some(credential),
                ..
            } = &state
            {
                let agent_id = *agent_id;
                let credential = credential.clone();
                tokio::task::spawn_blocking(move || {
                    if let Err(err) = credentials::store_agent_credential(agent_id, &credential) {
                        log::error!("failed to store the issued agent credential in the OS keyring: {err}");
                    }
                });
            }

            on_state_change(state);
        }
    });

    tauri::async_runtime::spawn(async move {
        while let Some(event) = events_rx.recv().await {
            on_event(event);
        }
    });

    // This connection only ever needs to show live pairing progress in the
    // "Add Server" modal, never to send anything to the agent - the sender
    // half is simply never used, so `command_rx` just sits empty for this
    // connection's whole (short) lifetime. The real, command-capable
    // connection is `state::AgentSessionManager`'s (Etap M3), started
    // separately once pairing actually succeeds.
    let (_command_tx, command_rx) = mpsc::channel(1);
    let run_handle = tauri::async_runtime::spawn(agent_client::run(config, events_tx, state_tx, command_rx));
    session.replace(run_handle);
}

#[tauri::command]
pub fn start_agent_pairing(app: AppHandle, session: State<PairingSession>, host: String, port: u16, pairing_code: String) {
    let config = AgentClientConfig {
        url: format!("wss://{host}:{port}/ws"),
        client_name: "VibeSSH Desktop".to_string(),
        client_version: env!("CARGO_PKG_VERSION").to_string(),
        auth_token: Some(pairing_code),
    };
    let app_for_events = app.clone();
    spawn_pairing_session(
        &session,
        config,
        move |state| {
            let _ = app.emit(PAIRING_STATE_EVENT, &state);
        },
        move |event| {
            let _ = app_for_events.emit(PAIRING_EVENT_EVENT, &event);
        },
    );
}

#[tauri::command]
pub fn cancel_agent_pairing(session: State<PairingSession>) {
    session.cancel();
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use futures_util::{SinkExt, StreamExt};
    use tokio::net::TcpListener;
    use tokio::sync::mpsc;
    use tokio::time::timeout;
    use tokio_tungstenite::tungstenite::Message;

    use vibessh_protocol::{HandshakeRequest, HandshakeResponse, PROTOCOL_VERSION};

    use super::*;

    /// Same mock-agent shape as agent_client's own tests - just enough to
    /// prove this module's spawn/forward/cancel plumbing against a real
    /// socket, not a reimplementation of the real agent's handshake logic.
    async fn spawn_mock_agent(expected_code: &'static str) -> (String, uuid::Uuid) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let agent_id = uuid::Uuid::new_v4();

        tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();

            let Some(Ok(Message::Text(text))) = ws.next().await else {
                return;
            };
            let request: HandshakeRequest = serde_json::from_str(&text).unwrap();
            let accepted = request.auth_token.as_deref() == Some(expected_code);

            let response = HandshakeResponse {
                accepted,
                agent_id,
                agent_version: "0.0.0-mock".into(),
                protocol_version: PROTOCOL_VERSION,
                error: None,
                issued_credential: accepted.then(|| "mock-credential".to_string()),
                capabilities: vibessh_protocol::AgentCapabilities {
                    docker: true,
                    systemd: true,
                    ..Default::default()
                },
            };
            let _ = ws
                .send(Message::Text(serde_json::to_string(&response).unwrap()))
                .await;

            let event = ServerEvent::MetricsUpdate {
                metrics: vibessh_protocol::ServerMetrics {
                    cpu_usage_percent: 12.5,
                    ram_used_bytes: 1,
                    ram_total_bytes: 2,
                    disk_used_bytes: 1,
                    disk_total_bytes: 2,
                    load_average_1m: 0.5,
                    uptime_seconds: 100,
                    network_rx_bytes_per_sec: 0,
                    network_tx_bytes_per_sec: 0,
                },
            };
            let _ = ws.send(Message::Text(serde_json::to_string(&event).unwrap())).await;

            tokio::time::sleep(Duration::from_secs(5)).await;
        });

        (format!("127.0.0.1:{}", addr.port()), agent_id)
    }

    #[tokio::test]
    async fn spawn_pairing_session_reports_connected_and_cancel_stops_it() {
        let (addr, agent_id) = spawn_mock_agent("VIBE-COMMAND-TEST").await;
        let session = PairingSession::new();
        let config = AgentClientConfig {
            url: format!("ws://{addr}/ws"),
            client_name: "vibessh-desktop-test".into(),
            client_version: "0.0.0".into(),
            auth_token: Some("VIBE-COMMAND-TEST".into()),
        };

        let (states_tx, mut states_rx) = mpsc::unbounded_channel();
        let (events_tx, mut events_rx) = mpsc::unbounded_channel();
        spawn_pairing_session(
            &session,
            config,
            move |state| {
                let _ = states_tx.send(state);
            },
            move |event| {
                let _ = events_tx.send(event);
            },
        );

        let connected = timeout(Duration::from_secs(2), async {
            loop {
                match states_rx.recv().await.expect("channel closed") {
                    AgentConnectionState::Connected {
                        agent_id,
                        issued_credential,
                        capabilities,
                        ..
                    } => return (agent_id, issued_credential, capabilities),
                    _ => continue,
                }
            }
        })
        .await
        .expect("timed out waiting for Connected");

        assert_eq!(connected.0, agent_id);
        assert_eq!(connected.1.as_deref(), Some("mock-credential"));
        assert!(connected.2.docker);
        assert!(connected.2.systemd);
        assert!(!connected.2.minecraft);

        let metrics_event = timeout(Duration::from_secs(2), events_rx.recv())
            .await
            .expect("timed out waiting for a metrics event")
            .expect("event channel closed");
        match metrics_event {
            ServerEvent::MetricsUpdate { metrics } => assert_eq!(metrics.cpu_usage_percent, 12.5),
            other => panic!("expected MetricsUpdate, got {other:?}"),
        }

        session.cancel();

        let saw_another_connected = timeout(Duration::from_millis(500), async {
            loop {
                match states_rx.recv().await {
                    Some(AgentConnectionState::Connected { .. }) => return true,
                    Some(_) => continue,
                    None => return false,
                }
            }
        })
        .await
        .unwrap_or(false);
        assert!(!saw_another_connected, "cancel() did not stop the background task");
    }
}
