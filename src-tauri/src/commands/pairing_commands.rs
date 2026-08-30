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

/// Frontend listens with `listen(PAIRING_STATE_EVENT, ...)` from
/// `@tauri-apps/api/event`. Payload is `AgentConnectionState`'s JSON shape.
const PAIRING_STATE_EVENT: &str = "agent-pairing://state";

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
/// calling `on_state_change` from a spawned task every time its connection
/// state changes. Returns immediately - callers don't block until connected.
pub fn spawn_pairing_session<F>(session: &PairingSession, config: AgentClientConfig, mut on_state_change: F)
where
    F: FnMut(AgentConnectionState) + Send + 'static,
{
    let (events_tx, mut events_rx) = mpsc::channel(16);
    let (state_tx, mut state_rx) = watch::channel(AgentConnectionState::Connecting);

    tauri::async_runtime::spawn(async move {
        loop {
            if state_rx.changed().await.is_err() {
                return; // state_tx dropped - the run task ended or was aborted
            }
            on_state_change(state_rx.borrow_and_update().clone());
        }
    });

    // No pairing-flow feature reads ServerEvents yet (Etap D defined the
    // channel for the realtime data feed, not for pairing itself) - this
    // just drains it so agent_client::run never blocks on a full buffer.
    // It ends on its own once events_tx drops, same as the loop above.
    tauri::async_runtime::spawn(async move { while events_rx.recv().await.is_some() {} });

    let run_handle = tauri::async_runtime::spawn(agent_client::run(config, events_tx, state_tx));
    session.replace(run_handle);
}

#[tauri::command]
pub fn start_agent_pairing(app: AppHandle, session: State<PairingSession>, host: String, port: u16, pairing_code: String) {
    let config = AgentClientConfig {
        url: format!("ws://{host}:{port}/ws"),
        client_name: "VibeSSH Desktop".to_string(),
        client_version: env!("CARGO_PKG_VERSION").to_string(),
        auth_token: Some(pairing_code),
    };
    spawn_pairing_session(&session, config, move |state| {
        let _ = app.emit(PAIRING_STATE_EVENT, &state);
    });
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
            };
            let _ = ws
                .send(Message::Text(serde_json::to_string(&response).unwrap()))
                .await;
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
        spawn_pairing_session(&session, config, move |state| {
            let _ = states_tx.send(state);
        });

        let connected = timeout(Duration::from_secs(2), async {
            loop {
                match states_rx.recv().await.expect("channel closed") {
                    AgentConnectionState::Connected {
                        agent_id,
                        issued_credential,
                        ..
                    } => return (agent_id, issued_credential),
                    _ => continue,
                }
            }
        })
        .await
        .expect("timed out waiting for Connected");

        assert_eq!(connected.0, agent_id);
        assert_eq!(connected.1.as_deref(), Some("mock-credential"));

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
