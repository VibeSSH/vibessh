//! Desktop-side half of the Agent Mode transport (Etap D transport, Etap E
//! pairing, Etap K TLS). Owns the WebSocket connection to one `vibe-agent`:
//! handshake, reconnect with backoff, and a read timeout that treats a
//! silent connection (no heartbeat, no events) as dead.
//! `AgentClientConfig::auth_token` is either a pairing code (first
//! connection) or a previously issued credential (every one after);
//! callers are responsible for persisting a freshly `issued_credential`
//! (OS keyring, never plaintext) so the next connection can use it instead
//! of the one-time code. Not wired into `ServerConnection` yet — that's
//! Etap H, once there's a server record to attach a running client to. For
//! now it's a self-contained, independently testable client.
//!
//! Connections are `wss://` against the agent's self-signed certificate
//! (Etap K - there's no CA for an arbitrary self-hosted VPS agent), with
//! certificate validation disabled: this defeats *passive* eavesdropping
//! on the pairing code / credential in transit, which is the realistic
//! everyday threat (a shared network, a malicious router, an ISP). It does
//! NOT defeat an *active* attacker on the very first connection, who could
//! present their own certificate before there's anything to compare it
//! against - the desktop has no side channel to learn the real
//! fingerprint ahead of time (the pairing code is generated before the
//! desktop has ever talked to the agent). Pinning the certificate after a
//! first successful connection and rejecting a mismatch on every one after
//! is the concrete next step; see `agent::tls` for the matching note on
//! why the agent's certificate persists across restarts instead of being
//! regenerated, specifically so a future pin doesn't break on every reboot.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async_tls_with_config, Connector, MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

use vibessh_protocol::{AgentCapabilities, HandshakeRequest, HandshakeResponse, ServerEvent, PROTOCOL_VERSION};

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

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum AgentConnectionState {
    #[serde(rename = "connecting")]
    Connecting,
    #[serde(rename = "connected")]
    Connected {
        agent_id: Uuid,
        agent_version: String,
        /// `Some` exactly once, on the handshake that consumed a pairing
        /// code. The caller must persist this (OS keyring) and use it as
        /// `auth_token` from then on - it's not stored by this module,
        /// which deliberately doesn't know about `storage`/keyring itself.
        issued_credential: Option<String>,
        /// Freshly detected on every handshake (Etap I) - the UI hides or
        /// marks features this particular agent/host can't do instead of
        /// assuming every Linux box has Docker/systemd/etc.
        capabilities: AgentCapabilities,
    },
    #[serde(rename = "disconnected")]
    Disconnected { reason: String },
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
    // Only consulted for wss:// URLs - a plain ws:// URL (used by this
    // module's own tests against a bare mock server) ignores it and
    // connects unencrypted, same as always.
    let connector = insecure_tls_connector()?;
    let (mut ws, _) = connect_async_tls_with_config(&config.url, None, false, Some(connector))
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
        capabilities: response.capabilities,
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

fn insecure_tls_connector() -> Result<Connector, String> {
    native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .build()
        .map(Connector::NativeTls)
        .map_err(|err| format!("failed to build TLS connector: {err}"))
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
