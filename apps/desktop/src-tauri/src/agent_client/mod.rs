//! Desktop-side half of the Agent Mode transport (Etap D transport, Etap E
//! pairing, Etap K TLS, Etap M3 the send path). Owns the WebSocket
//! connection to one `vibe-agent`: handshake, reconnect with backoff, a
//! read timeout that treats a silent connection (no heartbeat, no events)
//! as dead, and (Etap M3) a `command_rx` this task drains and forwards to
//! the agent on every (re)connection - queued commands sent while
//! disconnected are simply sent once the next connection succeeds, since
//! the same receiver is reused across reconnect attempts rather than
//! recreated. `AgentClientConfig::auth_token` is either a pairing code
//! (first connection) or a previously issued credential (every one after);
//! callers are responsible for persisting a freshly `issued_credential`
//! (OS keyring, never plaintext) so the next connection can use it instead
//! of the one-time code. Wired into a real, app-session-long connection by
//! `state::AgentSessionManager` (Etap M3) - see that module's own doc
//! comment for why "for the life of the pairing modal" (the only thing
//! that used this before) wasn't enough to build revisioning against.
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

use vibessh_protocol::{AgentCapabilities, DesktopCommand, HandshakeRequest, HandshakeResponse, ServerEvent, PROTOCOL_VERSION};

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
    /// The agent certificate's SHA-256 fingerprint, as seen and recorded on
    /// a previous successful connection. `None` means "not pinned yet" -
    /// the first connection records whatever it sees.
    ///
    /// This is trust-on-first-use, the same model `ssh::client`'s
    /// `TofuHandler` already implements for SSH host keys, and it exists
    /// for the same reason: the agent presents a self-signed certificate,
    /// so ordinary chain validation can never succeed and was simply turned
    /// off (`danger_accept_invalid_certs`). With no pin on top of that, the
    /// connection was trivially interceptable on *every* connection, not
    /// just the first - and the very next thing sent over it is the bearer
    /// credential (see `connect_and_stream`), so an interceptor got a
    /// durable secret rather than a single session.
    pub known_fingerprint: Option<String>,
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
        /// The SHA-256 fingerprint of the certificate this connection
        /// actually used. Reported on every successful handshake so the
        /// caller can record it the first time (trust on first use) - this
        /// module deliberately doesn't know about `storage`, the same way
        /// it doesn't persist `issued_credential` itself.
        ///
        /// `None` only for a non-TLS `ws://` connection, which is this
        /// module's own tests against a mock server.
        certificate_fingerprint: Option<String>,
    },
    #[serde(rename = "disconnected")]
    Disconnected { reason: String },
}

/// Runs until `events_tx`'s receiver is dropped: connects, hands off
/// `ServerEvent`s (heartbeats are swallowed here, callers only see events
/// worth reacting to), forwards anything sent on `command_rx` to the agent
/// (Etap M3 - `command_rx` is reused across reconnects, so a command queued
/// while disconnected just goes out once the next connection succeeds), and
/// on any failure reconnects with exponential backoff that resets after
/// each successful handshake.
pub async fn run(
    config: AgentClientConfig,
    events_tx: mpsc::Sender<ServerEvent>,
    state_tx: watch::Sender<AgentConnectionState>,
    mut command_rx: mpsc::Receiver<DesktopCommand>,
) {
    let mut backoff = INITIAL_BACKOFF;

    loop {
        let _ = state_tx.send(AgentConnectionState::Connecting);

        match connect_and_stream(&config, &events_tx, &state_tx, &mut command_rx, &mut backoff).await {
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
    command_rx: &mut mpsc::Receiver<DesktopCommand>,
    backoff: &mut Duration,
) -> Result<(), String> {
    // Only consulted for wss:// URLs - a plain ws:// URL (used by this
    // module's own tests against a bare mock server) ignores it and
    // connects unencrypted, same as always.
    let connector = permissive_tls_connector()?;
    let (mut ws, _) = connect_async_tls_with_config(&config.url, None, false, Some(connector))
        .await
        .map_err(|err| format!("connect failed: {err}"))?;

    // **Before anything is sent.** The handshake below carries the bearer
    // credential, so the certificate has to be checked while the connection
    // is still worthless to an interceptor. Chain validation is off by
    // necessity (the agent's certificate is self-signed), so this pin is
    // the only thing standing between a MITM and a durable secret.
    let mut observed_fingerprint = None;
    match peer_fingerprint(&ws) {
        Some(fingerprint) => {
            if let Some(expected) = &config.known_fingerprint {
                if expected != &fingerprint {
                    return Err(HOST_KEY_MISMATCH.to_string());
                }
            } else {
                // Trust on first use - the caller records it when the
                // handshake below succeeds.
                log::info!("agent_client: first connection to this agent, pinning certificate {fingerprint}");
            }
            observed_fingerprint = Some(fingerprint);
        }
        None => {
            // A plain `ws://` connection has no certificate to pin. That is
            // only ever this module's own tests against a mock server; a
            // real agent endpoint is always `wss://`.
            if config.url.starts_with("wss://") {
                return Err("the agent's TLS certificate could not be read, so its identity can't be verified".to_string());
            }
        }
    }

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
        certificate_fingerprint: observed_fingerprint,
    });

    // Etap M3: once `command_rx`'s sender is dropped, `recv()` resolves to
    // `None` immediately on every poll - without this guard, `select!`
    // would pick that branch (or hang inside it) on essentially every loop
    // iteration and starve the read side. A caller with nothing to send
    // (e.g. the pairing flow's own short-lived connection) drops its sender
    // the moment it stops needing it - that must silently disable this
    // branch for the rest of the connection's life, not end it (only
    // `events_tx` being closed means "stop the connection," see below).
    let mut command_channel_open = true;

    loop {
        tokio::select! {
            incoming = timeout(READ_TIMEOUT, read_json::<ServerEvent>(&mut ws)) => {
                let event = match incoming {
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
            // `command_rx` outlives any single connection attempt (owned by
            // the caller, passed in by `&mut`), so a command sent while
            // disconnected or mid-reconnect just waits in the channel and
            // goes out the moment this arm is next reached on a fresh
            // connection - no separate "flush queued commands" step needed.
            command = command_rx.recv(), if command_channel_open => {
                let Some(command) = command else {
                    command_channel_open = false;
                    continue;
                };
                let payload = serde_json::to_string(&command).expect("DesktopCommand always serializes");
                ws.send(Message::Text(payload)).await.map_err(|err| format!("failed to send a command to the agent: {err}"))?;
            }
        }
    }
}

/// The message a fingerprint mismatch produces. Deliberately the same shape
/// as `ssh::client::classify_connect_error`'s host-key message: same
/// situation, same two possible causes, and the operator should not have to
/// learn two different vocabularies for it.
const HOST_KEY_MISMATCH: &str = "the agent's TLS certificate doesn't match the one VibeSSH saw before - this can mean the agent was reinstalled, \
     but it can also mean someone is intercepting the connection. Re-pair the Node only if you know why the certificate changed.";

/// Chain validation is disabled because the agent's certificate is
/// self-signed and generated on the Node itself - there is no CA to check
/// it against, and there never will be. Identity comes from the fingerprint
/// pin in `connect_and_stream` instead, which is what makes this safe;
/// on its own this connector trusts anything.
fn permissive_tls_connector() -> Result<Connector, String> {
    native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .build()
        .map(Connector::NativeTls)
        .map_err(|err| format!("failed to build TLS connector: {err}"))
}

/// SHA-256 over the peer certificate's DER encoding, lowercase hex.
///
/// `None` for a non-TLS stream, which in practice only happens for the
/// `ws://` mock server this module's own tests use.
fn peer_fingerprint(ws: &WsStream) -> Option<String> {
    let tokio_tungstenite::MaybeTlsStream::NativeTls(tls) = ws.get_ref() else {
        return None;
    };
    let certificate = tls.get_ref().peer_certificate().ok().flatten()?;
    let der = certificate.to_der().ok()?;
    Some(fingerprint_of(&der))
}

fn fingerprint_of(der: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(der);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A fingerprint is what makes the pin comparable at all, so it has to
    /// be stable and to actually depend on the certificate bytes.
    #[test]
    fn fingerprint_is_stable_lowercase_hex_of_the_certificate_bytes() {
        let a = fingerprint_of(b"certificate one");
        assert_eq!(a, fingerprint_of(b"certificate one"));
        assert_eq!(a.len(), 64, "SHA-256 is 32 bytes, 64 hex characters");
        assert!(a.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()), "{a}");
        assert_ne!(a, fingerprint_of(b"certificate two"));
    }

    /// The mismatch message is the operator's only signal that something
    /// may be intercepting the connection, so it must say both things it
    /// could mean - the same way the SSH host-key message does.
    #[test]
    fn the_mismatch_message_explains_both_possible_causes() {
        assert!(HOST_KEY_MISMATCH.contains("reinstalled"), "{HOST_KEY_MISMATCH}");
        assert!(HOST_KEY_MISMATCH.contains("intercepting"), "{HOST_KEY_MISMATCH}");
    }
}
