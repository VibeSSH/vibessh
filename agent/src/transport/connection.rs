use axum::extract::ws::{Message, WebSocket};
use tokio::time::{interval, timeout, Duration};

use vibessh_protocol::{
    HandshakeRequest, HandshakeResponse, ProtocolErrorCode, ServerEvent, PROTOCOL_VERSION,
};

use super::{SharedState, HANDSHAKE_TIMEOUT_SECS};

/// Runs the whole lifetime of one client connection: handshake, then the
/// heartbeat/event loop until the client disconnects or errors out.
pub async fn handle(mut socket: WebSocket, state: SharedState) {
    if !perform_handshake(&mut socket, &state).await {
        return;
    }
    log::info!("agent: client connected");

    let mut heartbeat = interval(state.heartbeat_interval);
    heartbeat.tick().await; // first tick is immediate; consume it so we don't heartbeat on connect

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if !send_event(&mut socket, &ServerEvent::Heartbeat).await {
                    log::info!("agent: client disconnected (heartbeat send failed)");
                    return;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => {
                        log::info!("agent: client closed the connection");
                        return;
                    }
                    Some(Ok(_)) => {
                        // No client->agent messages are defined yet beyond the
                        // handshake (Etap D scope) - ignore anything else.
                    }
                    Some(Err(err)) => {
                        log::warn!("agent: websocket error: {err}");
                        return;
                    }
                }
            }
        }
    }
}

async fn perform_handshake(socket: &mut WebSocket, state: &SharedState) -> bool {
    let first_message = match timeout(Duration::from_secs(HANDSHAKE_TIMEOUT_SECS), socket.recv()).await {
        Ok(Some(Ok(Message::Text(text)))) => text,
        Ok(_) => {
            log::warn!("agent: connection closed or sent a non-text frame before handshake");
            return false;
        }
        Err(_) => {
            log::warn!("agent: client did not send a handshake within {HANDSHAKE_TIMEOUT_SECS}s");
            return false;
        }
    };

    let request: HandshakeRequest = match serde_json::from_str(&first_message) {
        Ok(request) => request,
        Err(err) => {
            log::warn!("agent: malformed handshake: {err}");
            let _ = send_handshake_rejection(socket, state, ProtocolErrorCode::InvalidMessage).await;
            return false;
        }
    };

    if request.protocol_version != PROTOCOL_VERSION {
        log::warn!(
            "agent: rejecting client on protocol version {} (agent runs {PROTOCOL_VERSION})",
            request.protocol_version
        );
        let _ = send_handshake_rejection(socket, state, ProtocolErrorCode::VersionMismatch).await;
        return false;
    }

    // Etap E (pairing) is what actually issues and verifies this token. Until
    // then every client is accepted, but loudly, so this isn't mistaken for
    // real authentication in the meantime.
    if request.auth_token.is_none() {
        log::warn!("agent: client '{}' connected with no auth token - accepting anyway, pairing isn't implemented yet", request.client_name);
    }

    let response = HandshakeResponse {
        accepted: true,
        agent_id: state.info.id,
        agent_version: state.info.version.clone(),
        protocol_version: PROTOCOL_VERSION,
        error: None,
    };
    send_json(socket, &response).await
}

async fn send_handshake_rejection(
    socket: &mut WebSocket,
    state: &SharedState,
    code: ProtocolErrorCode,
) -> bool {
    let response = HandshakeResponse {
        accepted: false,
        agent_id: state.info.id,
        agent_version: state.info.version.clone(),
        protocol_version: PROTOCOL_VERSION,
        error: Some(code),
    };
    send_json(socket, &response).await
}

async fn send_event(socket: &mut WebSocket, event: &ServerEvent) -> bool {
    send_json(socket, event).await
}

async fn send_json<T: serde::Serialize>(socket: &mut WebSocket, value: &T) -> bool {
    let payload = serde_json::to_string(value).expect("protocol DTOs always serialize");
    socket.send(Message::Text(payload)).await.is_ok()
}
