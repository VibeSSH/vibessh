use axum::extract::ws::{Message, WebSocket};
use tokio::time::{interval, timeout, Duration};

use vibessh_protocol::{
    HandshakeRequest, HandshakeResponse, ProtocolErrorCode, ServerEvent, PROTOCOL_VERSION,
};

use crate::pairing::{issue_credential, verify_credential};

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

    let issued_credential = match authenticate(state, request.auth_token.as_deref()) {
        AuthOutcome::AlreadyPaired => None,
        AuthOutcome::NewlyPaired(raw_credential) => Some(raw_credential),
        AuthOutcome::CredentialIssueFailed => {
            let _ = send_handshake_rejection(socket, state, ProtocolErrorCode::Internal).await;
            return false;
        }
        AuthOutcome::Rejected => {
            log::warn!(
                "agent: rejecting client '{}' - no valid credential or pairing code presented",
                request.client_name
            );
            let _ = send_handshake_rejection(socket, state, ProtocolErrorCode::Unauthorized).await;
            return false;
        }
    };

    let response = HandshakeResponse {
        accepted: true,
        agent_id: state.info.id,
        agent_version: state.info.version.clone(),
        protocol_version: PROTOCOL_VERSION,
        error: None,
        issued_credential,
        capabilities: crate::capabilities::detect(),
    };
    send_json(socket, &response).await
}

enum AuthOutcome {
    AlreadyPaired,
    NewlyPaired(String),
    CredentialIssueFailed,
    Rejected,
}

/// Checked in this order: an already-issued credential always wins over a
/// pairing code (so a stale/leaked pairing code can't be replayed against
/// an agent that's already paired to someone), then falls back to
/// consuming a pending pairing code.
fn authenticate(state: &SharedState, token: Option<&str>) -> AuthOutcome {
    let Some(token) = token else {
        return AuthOutcome::Rejected;
    };

    if verify_credential(&state.data_dir, token) {
        return AuthOutcome::AlreadyPaired;
    }

    if state.pairing.try_consume(token) {
        return match issue_credential(&state.data_dir) {
            Ok(raw) => AuthOutcome::NewlyPaired(raw),
            Err(err) => {
                log::error!("agent: failed to issue a credential after successful pairing: {err}");
                AuthOutcome::CredentialIssueFailed
            }
        };
    }

    AuthOutcome::Rejected
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
        issued_credential: None,
        // No capability fingerprinting for an unauthenticated attempt.
        capabilities: Default::default(),
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
