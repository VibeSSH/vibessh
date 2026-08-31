use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::{mpsc, watch, Mutex};
use uuid::Uuid;

use vibessh_protocol::{DesktopCommand, ServerEvent};

use crate::agent_client::{self, AgentClientConfig, AgentConnectionState};

/// The Agent's reply to one `DesktopCommand::ApplyDesiredState`, pulled out
/// of the raw `ServerEvent` stream (see `ensure_connected`'s spawned drain
/// task) into its own `watch` channel so `await_ack` can wait for a
/// specific revision without racing every other event type.
#[derive(Debug, Clone)]
pub struct AppliedAck {
    pub revision: u64,
    pub ok: bool,
    pub error: Option<String>,
}

struct Session {
    command_tx: mpsc::Sender<DesktopCommand>,
    state_rx: watch::Receiver<AgentConnectionState>,
    applied_rx: watch::Receiver<Option<AppliedAck>>,
    handle: tauri::async_runtime::JoinHandle<()>,
}

/// Keeps one Agent-mode Node's WebSocket connection alive for the life of
/// the app (Etap M3) - distinct from `PairingSession`, which only keeps a
/// connection alive for the "Add Server: Install Agent" modal's own
/// lifetime and is aborted the moment that modal closes (see its own doc
/// comment). Before this, an Agent-mode Server's connection existed ONLY
/// while that modal was open - the instant a user clicked "Done," queuing a
/// command (or even knowing whether a Node was still reachable) became
/// impossible, since nothing kept the socket open.
///
/// **Known, deliberate scope boundary**: reconnecting automatically after
/// the *app itself* restarts isn't attempted here. The agent's WS *port* is
/// never persisted anywhere (only `host` is, on the `Server` row - the
/// pairing form's port field only ever lived in that form's own local
/// state) - there is nothing to reconnect *to* until a future phase adds
/// that. What this session lives for: `ensure_connected` is called once,
/// right after a real pairing succeeds (the one moment host+port+a fresh
/// credential are all in hand at once), and the connection - with its own
/// `agent_client::run`-provided reconnect-with-backoff - then runs for as
/// long as the app keeps running.
#[derive(Default)]
pub struct AgentSessionManager {
    sessions: Mutex<HashMap<Uuid, Session>>,
}

impl AgentSessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// A second call for a `server_id` that's already connected is a safe
    /// no-op - re-pairing an already-live Node shouldn't spawn a duplicate
    /// connection racing the first one.
    pub async fn ensure_connected(&self, server_id: Uuid, config: AgentClientConfig) {
        let mut sessions = self.sessions.lock().await;
        if sessions.contains_key(&server_id) {
            return;
        }

        let (events_tx, mut events_rx) = mpsc::channel(16);
        let (state_tx, state_rx) = watch::channel(AgentConnectionState::Connecting);
        let (command_tx, command_rx) = mpsc::channel(16);
        let (applied_tx, applied_rx) = watch::channel(None);

        // Pulls `StateApplied` acks out for `await_ack` below; every other
        // event (metrics, logs, ...) is drained and dropped - nothing
        // outside the pairing flow's own short-lived connection consumes
        // live Node events yet (a future live Dashboard is the real
        // consumer of those; this channel exists so `agent_client::run`
        // always has somewhere to send without blocking, not to fan them
        // out anywhere yet).
        tokio::spawn(async move {
            while let Some(event) = events_rx.recv().await {
                if let ServerEvent::StateApplied { revision, ok, error } = event {
                    let _ = applied_tx.send(Some(AppliedAck { revision, ok, error }));
                }
            }
        });

        let handle = tauri::async_runtime::spawn(agent_client::run(config, events_tx, state_tx, command_rx));
        sessions.insert(server_id, Session { command_tx, state_rx, applied_rx, handle });
    }

    pub async fn state_of(&self, server_id: Uuid) -> Option<AgentConnectionState> {
        let sessions = self.sessions.lock().await;
        sessions.get(&server_id).map(|session| session.state_rx.borrow().clone())
    }

    /// `false` means there's no live session for this Node at all (never
    /// paired this app run, or the app hasn't reconnected to it yet) - the
    /// command was never sent, not "sent but failed." A live-but-currently-
    /// reconnecting session still accepts the send (`agent_client::run`
    /// queues it, see that module's own doc comment); only a torn-down
    /// session's dropped receiver makes this `false`.
    pub async fn send_command(&self, server_id: Uuid, command: DesktopCommand) -> bool {
        let sessions = self.sessions.lock().await;
        match sessions.get(&server_id) {
            Some(session) => session.command_tx.send(command).await.is_ok(),
            None => false,
        }
    }

    /// Waits up to `timeout` for a `StateApplied` ack matching `revision` -
    /// `None` on timeout (the Node may still be applying it, or may be
    /// offline; either way this is exactly the "never claim SUCCESS for an
    /// unreachable Node" case `services::node_state_service::reconcile_node`
    /// turns into `OfflinePending`, not a false positive). A stale ack for
    /// an older revision (superseded by a newer reconcile before this one
    /// answered) is skipped, not returned - only the exact revision asked
    /// for counts.
    pub async fn await_ack(&self, server_id: Uuid, revision: u64, timeout: Duration) -> Option<AppliedAck> {
        let mut applied_rx = {
            let sessions = self.sessions.lock().await;
            sessions.get(&server_id)?.applied_rx.clone()
        };

        tokio::time::timeout(timeout, async {
            loop {
                if let Some(ack) = applied_rx.borrow().clone() {
                    if ack.revision == revision {
                        return ack;
                    }
                }
                if applied_rx.changed().await.is_err() {
                    // The session was torn down mid-wait - no ack is ever
                    // coming.
                    std::future::pending::<()>().await;
                }
            }
        })
        .await
        .ok()
    }

    pub async fn stop(&self, server_id: Uuid) {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.remove(&server_id) {
            session.handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use futures_util::{SinkExt, StreamExt};
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::Message;
    use vibessh_protocol::{AgentCapabilities, HandshakeResponse, NodeDesiredState, PROTOCOL_VERSION};

    use super::*;

    /// A bare mock agent, same shape `tests/agent_client.rs`'s own mock
    /// uses: accepts one connection, handshakes, then echoes back a
    /// `StateApplied` ack for whatever revision it receives - just enough
    /// to prove `ensure_connected`/`send_command`/`await_ack` round-trip
    /// for real over a real WebSocket, not a reimplementation of the
    /// protocol itself.
    async fn spawn_mock_agent() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();

            let Some(Ok(Message::Text(_))) = ws.next().await else { return };
            let response = HandshakeResponse {
                accepted: true,
                agent_id: Uuid::new_v4(),
                agent_version: "0.0.0-mock".into(),
                protocol_version: PROTOCOL_VERSION,
                error: None,
                issued_credential: None,
                capabilities: AgentCapabilities::default(),
            };
            ws.send(Message::Text(serde_json::to_string(&response).unwrap())).await.unwrap();

            let Some(Ok(Message::Text(text))) = ws.next().await else { return };
            let DesktopCommand::ApplyDesiredState { revision, .. } = serde_json::from_str(&text).unwrap();
            let ack = ServerEvent::StateApplied { revision, ok: true, error: None };
            ws.send(Message::Text(serde_json::to_string(&ack).unwrap())).await.unwrap();

            tokio::time::sleep(Duration::from_secs(5)).await;
        });

        format!("ws://{addr}/ws")
    }

    #[tokio::test]
    async fn ensure_connected_send_command_and_await_ack_round_trip_over_a_real_socket() {
        let url = spawn_mock_agent().await;
        let manager = AgentSessionManager::new();
        let server_id = Uuid::new_v4();
        let config = AgentClientConfig { url, client_name: "test".into(), client_version: "0.0.0".into(), auth_token: Some("code".into()) };

        manager.ensure_connected(server_id, config).await;

        // A second call for the same server_id must not spawn a duplicate
        // connection - if it did, the mock agent (which only accepts one
        // connection) would make the second `send_command` below hang
        // against a session no one ever handshakes, and the test would
        // time out.
        manager
            .ensure_connected(server_id, AgentClientConfig { url: "ws://127.0.0.1:1".into(), client_name: "x".into(), client_version: "x".into(), auth_token: None })
            .await;

        let sent = manager.send_command(server_id, DesktopCommand::ApplyDesiredState { revision: 5, state: NodeDesiredState::default() }).await;
        assert!(sent, "a live session must accept the command");

        let ack = manager.await_ack(server_id, 5, Duration::from_secs(2)).await;
        assert!(matches!(ack, Some(AppliedAck { revision: 5, ok: true, error: None })), "{ack:?}");

        // An unknown server_id has no session at all - sending must report
        // that honestly rather than silently succeeding.
        assert!(!manager.send_command(Uuid::new_v4(), DesktopCommand::ApplyDesiredState { revision: 1, state: NodeDesiredState::default() }).await);
    }
}
