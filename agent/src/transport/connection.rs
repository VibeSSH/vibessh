use axum::extract::ws::{Message, WebSocket};
use chrono::Utc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use uuid::Uuid;
use tokio::time::{interval, timeout, Duration, MissedTickBehavior};

use vibessh_protocol::{
    DesktopCommand, HandshakeRequest, HandshakeResponse, LogLine, ProtocolErrorCode, ServerEvent, PROTOCOL_VERSION,
};

use crate::metrics::MetricsCollector;
use crate::pairing::{issue_credential, verify_credential};

use super::{SharedState, HANDSHAKE_TIMEOUT_SECS};

/// Runs the whole lifetime of one client connection: handshake, then the
/// heartbeat/metrics/event loop until the client disconnects or errors out.
pub async fn handle(mut socket: WebSocket, state: SharedState) {
    if !perform_handshake(&mut socket, &state).await {
        return;
    }
    log::info!("agent: client connected");

    let mut heartbeat = ticker(state.heartbeat_interval).await;
    let mut metrics_tick = ticker(state.metrics_interval).await;
    // One collector per connection, not shared across connections - simple
    // and correct for the single-viewer case this UI has today. Broadcasting
    // one shared sample to multiple simultaneous viewers is a real future
    // optimization, not something worth building before anything needs it.
    let mut metrics = MetricsCollector::new();
    // Follows write here rather than to the socket: the socket is only ever
    // touched by this loop, which is what keeps heartbeats and metrics
    // flowing while a container is producing output.
    let (lines_tx, mut lines_rx) = mpsc::unbounded_channel::<LogLine>();
    let mut follows: FollowRegistry = FollowRegistry::new();

    loop {
        tokio::select! {
            line = lines_rx.recv() => {
                match line {
                    Some(line) => {
                        if !send_event(&mut socket, &ServerEvent::LogsLine(line)).await {
                            log::info!("agent: client disconnected (log line send failed)");
                            return;
                        }
                    }
                    // `lines_tx` is held by this function for the whole
                    // connection, so this only happens at shutdown.
                    None => return,
                }
            }
            _ = heartbeat.tick() => {
                if !send_event(&mut socket, &ServerEvent::Heartbeat).await {
                    log::info!("agent: client disconnected (heartbeat send failed)");
                    return;
                }
            }
            _ = metrics_tick.tick() => {
                let sample = metrics.collect();
                if !send_event(&mut socket, &ServerEvent::MetricsUpdate { metrics: sample }).await {
                    log::info!("agent: client disconnected (metrics send failed)");
                    return;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => {
                        log::info!("agent: client closed the connection");
                        return;
                    }
                    Some(Ok(Message::Text(text))) => {
                        // A malformed/unrecognized frame is logged and
                        // ignored, not a reason to drop the connection -
                        // this is the same "don't let one bad message end
                        // an otherwise-healthy session" stance the
                        // handshake's own timeout/version-mismatch paths
                        // take by returning a rejection instead of just
                        // silently hanging up.
                        match serde_json::from_str::<DesktopCommand>(&text) {
                            Ok(command) => {
                                if !handle_command(&mut socket, command, &mut follows, &lines_tx).await {
                                    log::info!("agent: client disconnected (command result send failed)");
                                    return;
                                }
                            }
                            Err(err) => log::warn!("agent: ignoring an unrecognized message from Desktop: {err}"),
                        }
                    }
                    Some(Ok(_)) => {
                        // Every real Desktop->Agent message is text/JSON
                        // (DesktopCommand) - any other frame kind (binary,
                        // ping/pong is handled by axum itself) has nothing
                        // defined for it yet.
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

/// An interval whose missed ticks are delayed, not burst-fired. Without
/// this, a slow client (send() blocked waiting for it to drain a full TCP
/// buffer) would make tokio's default interval behavior fire a *burst* of
/// queued-up ticks the moment the connection catches up - exactly the
/// "sending data absurdly often" the metrics interval is meant to avoid.
/// This is this connection's actual backpressure mechanism: a slow reader
/// naturally throttles how often the agent samples and sends, instead of
/// either blocking forever or firing an unbounded backlog.
async fn ticker(period: Duration) -> tokio::time::Interval {
    let mut interval = interval(period);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    interval.tick().await; // first tick is immediate; consume it so we don't fire on connect
    interval
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

/// The `docker logs -f` processes this connection has running, by the
/// Desktop's follow id.
///
/// Killed when the connection ends, not just when the Desktop asks. A
/// dropped connection is the common case - a laptop closing, a network
/// blip - and without this every disconnect would leave a `docker logs -f`
/// running on the Node until somebody noticed. That is the same class of
/// leftover `runtime::docker`'s teardown exists to prevent.
type FollowRegistry = std::collections::HashMap<Uuid, tokio::process::Child>;

/// Starts one follow, streaming its output into `lines_tx`.
///
/// The child's stdout is read here rather than by the caller because the
/// caller owns the socket and must stay free to serve heartbeats and
/// metrics while a container is quiet - or noisy.
///
/// stderr is merged into stdout by `2>&1` for the same reason the Desktop's
/// SSH path does it: `docker logs` writes a container's stderr there, and
/// separating them would drop half of what a crashing process said.
fn start_follow(follow_id: Uuid, container: &str, tail: u32, lines_tx: mpsc::UnboundedSender<LogLine>) -> std::io::Result<tokio::process::Child> {
    let mut child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(format!("docker logs --tail {} -f {} 2>&1", tail.clamp(1, 5000), shell_quote(container)))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()?;

    let Some(stdout) = child.stdout.take() else {
        return Ok(child);
    };

    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            // A closed receiver means the connection ended; stop reading
            // rather than filling a channel nobody drains.
            if lines_tx.send(LogLine { source: "docker".to_string(), line, timestamp: Utc::now(), follow_id: Some(follow_id) }).is_err() {
                break;
            }
        }
    });

    Ok(child)
}

/// POSIX single-quoting for the one value that reaches a shell here.
///
/// The container name comes from the Desktop over the network, and this is
/// the Agent's side of `AGENTS.md` §1 - nothing built into a command line
/// goes in unquoted, whoever it came from. The Desktop validates the name
/// too; neither end relies on the other having done it.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Applies one `DesktopCommand` and reports the result back - Etap M3's
/// `ApplyDesiredState` carries an empty `NodeDesiredState`, so there is
/// nothing yet that could actually fail to apply; this always acks `ok:
/// true`. The real apply step (writing firewall rules / WireGuard config /
/// DNS fragments to disk and reconciling the host to match) is later,
/// deferred work that replaces the `Ok(true)` below - the Agent stays a
/// "dumb applier" per the control-plane design (Desktop is the only place
/// that decides whether a Node is in or out of sync), it never second-
/// guesses or re-derives what it was asked to apply.
async fn handle_command(
    socket: &mut WebSocket,
    command: DesktopCommand,
    follows: &mut FollowRegistry,
    lines_tx: &mpsc::UnboundedSender<LogLine>,
) -> bool {
    match command {
        DesktopCommand::ApplyDesiredState { revision, .. } => {
            send_event(socket, &ServerEvent::StateApplied { revision, ok: true, error: None }).await
        }
        DesktopCommand::FollowLogs { follow_id, container, tail } => {
            // Replacing rather than refusing: a Desktop that reconnects and
            // re-asks for the same follow would otherwise leave the first
            // process running and receive every line twice.
            if let Some(mut previous) = follows.remove(&follow_id) {
                let _ = previous.kill().await;
            }
            match start_follow(follow_id, &container, tail, lines_tx.clone()) {
                Ok(child) => {
                    follows.insert(follow_id, child);
                }
                // Not fatal to the connection: one console failing to open
                // must not disconnect a Node. The Desktop sees no lines and
                // falls back, which is the same outcome as a runtime with
                // no follow at all.
                Err(err) => log::warn!("agent: couldn't follow logs for {container}: {err}"),
            }
            true
        }
        DesktopCommand::StopFollowingLogs { follow_id } => {
            if let Some(mut child) = follows.remove(&follow_id) {
                let _ = child.kill().await;
            }
            true
        }
    }
}

async fn send_event(socket: &mut WebSocket, event: &ServerEvent) -> bool {
    send_json(socket, event).await
}

async fn send_json<T: serde::Serialize>(socket: &mut WebSocket, value: &T) -> bool {
    let payload = serde_json::to_string(value).expect("protocol DTOs always serialize");
    socket.send(Message::Text(payload)).await.is_ok()
}
