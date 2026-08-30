//! Desktop-side half of the Agent Mode transport (Etap D transport, Etap E
//! pairing). Owns the WebSocket connection to one `vibe-agent`: handshake,
//! reconnect with backoff, and a read timeout that treats a silent
//! connection (no heartbeat, no events) as dead. `AgentClientConfig::auth_token`
//! is either a pairing code (first connection) or a previously issued
//! credential (every one after); callers are responsible for persisting a
//! freshly `issued_credential` (OS keyring, never plaintext) so the next
//! connection can use it instead of the one-time code. Not wired into
//! `ServerConnection` yet — that's Etap H, once there's a server record to
//! attach a running client to. For now it's a self-contained, independently
//! testable client.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

use vibessh_protocol::{HandshakeRequest, HandshakeResponse, ServerEvent, PROTOCOL_VERSION};

const READ_TIMEOUT: Duration = Duration::from_secs(30);
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Debug, Clone)]
pub struct AgentClientConfig {
    pub url: String,
    pub client_name: String,
    pub client_version: String,
    /// A pairing code on first connection, the stored credential thereafter.
    pub auth_token: Option<String>,
}

#[derive(Debug, Clone)]
pub enum AgentConnectionState {
    Connecting,
    Connected {
        agent_id: Uuid,
        agent_version: String,
        /// `Some` exactly once, on the handshake that consumed a pairing
        /// code. The caller must persist this (OS keyring) and use it as
        /// `auth_token` from then on - it's not stored by this module,
        /// which deliberately doesn't know about `storage`/keyring itself.
        issued_credential: Option<String>,
    },
    Disconnected {
        reason: String,
    },
}

/// Runs until `events_tx`'s receiver is dropped: connects, hands off
/// `ServerEvent`s (heartbeats are swallowed here, callers only see events
/// worth reacting to), and on any failure reconnects with exponential
/// backoff that resets after each successful handshake.
pub async fn run(
    config: AgentClientConfig,
    events_tx: mpsc::Sender<ServerEvent>,
    state_tx: watch::Sender<AgentConnectionState>,
) {
    let mut backoff = INITIAL_BACKOFF;

    loop {
        let _ = state_tx.send(AgentConnectionState::Connecting);

        match connect_and_stream(&config, &events_tx, &state_tx, &mut backoff).await {
            Ok(()) => return, // caller dropped the receiver - stop for good
            Err(reason) => {
                log::warn!("agent_client: {reason}; retrying in {backoff:?}");
                let _ = state_tx.send(AgentConnectionState::Disconnected {
                    reason: reason.clone(),
                });
                tokio::time::sleep(backoff).await;
                backoff = std::cmp::min(backoff * 2, MAX_BACKOFF);
            }
        }
    }
}

async fn connect_and_stream(
    config: &AgentClientConfig,
    events_tx: &mpsc::Sender<ServerEvent>,
    state_tx: &watch::Sender<AgentConnectionState>,
    backoff: &mut Duration,
) -> Result<(), String> {
    let (mut ws, _) = connect_async(&config.url)
        .await
        .map_err(|err| format!("connect failed: {err}"))?;

    let request = HandshakeRequest {
        protocol_version: PROTOCOL_VERSION,
        client_name: config.client_name.clone(),
        client_version: config.client_version.clone(),
        auth_token: config.auth_token.clone(),
    };
    let payload = serde_json::to_string(&request).expect("HandshakeRequest always serializes");
    ws.send(Message::Text(payload))
        .await
        .map_err(|err| format!("failed to send handshake: {err}"))?;

    let response: HandshakeResponse = read_json(&mut ws).await.map_err(|reason| format!("handshake failed: {reason}"))?;
    if !response.accepted {
        return Err(match response.error {
            Some(code) => format!("handshake rejected: {code:?}"),
            None => "handshake rejected".to_string(),
        });
    }

    // A successful handshake means the agent is reachable and speaking our
    // protocol - forget how long it took to get here.
    *backoff = INITIAL_BACKOFF;
    log::info!("agent_client: connected to agent {}", response.agent_id);
    let _ = state_tx.send(AgentConnectionState::Connected {
        agent_id: response.agent_id,
        agent_version: response.agent_version,
        issued_credential: response.issued_credential,
    });

    loop {
        let event: ServerEvent = match timeout(READ_TIMEOUT, read_json(&mut ws)).await {
            Ok(Ok(event)) => event,
            Ok(Err(reason)) => return Err(reason),
            Err(_) => return Err("no message received within the read timeout".to_string()),
        };

        if matches!(event, ServerEvent::Heartbeat) {
            continue;
        }

        if events_tx.send(event).await.is_err() {
            return Ok(());
        }
    }
}

async fn read_json<T: serde::de::DeserializeOwned>(ws: &mut WsStream) -> Result<T, String> {
    match ws.next().await {
        Some(Ok(Message::Text(text))) => {
            serde_json::from_str(&text).map_err(|err| format!("invalid message: {err}"))
        }
        Some(Ok(Message::Close(_))) | None => Err("connection closed".to_string()),
        Some(Ok(_)) => Err("unexpected non-text frame".to_string()),
        Some(Err(err)) => Err(format!("websocket error: {err}")),
    }
}
