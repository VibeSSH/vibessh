//! Proves Etap D (transport) and Etap E (pairing) end-to-end against the
//! real router - a bare WebSocket client, and for the pairing cases the
//! real local control HTTP endpoint too, not a reimplementation of either.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream};

use vibe_agent::identity::AgentIdentity;
use vibe_agent::info::AgentInfo;
use vibe_agent::pairing::PairingRegistry;
use vibe_agent::transport::{self, SharedState};
use vibessh_protocol::{
    DesktopCommand, HandshakeRequest, HandshakeResponse, NodeDesiredState, ProtocolErrorCode, ServerEvent, PROTOCOL_VERSION,
};

/// The agent's certificate is self-signed (Etap K - no CA for an arbitrary
/// self-hosted VPS), so tests need the same "accept it anyway" connector
/// real desktop clients use (`agent_client`) instead of the default
/// validating one, which would reject every connection here.
async fn connect_insecure<R: IntoClientRequest + Unpin>(
    request: R,
) -> WebSocketStream<MaybeTlsStream<TcpStream>> {
    let connector = native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .build()
        .expect("failed to build a permissive TLS connector for tests");
    let (ws, _) = tokio_tungstenite::connect_async_tls_with_config(
        request,
        None,
        false,
        Some(Connector::NativeTls(connector)),
    )
    .await
    .expect("connect");
    ws
}

struct TestAgent {
    ws_url: String,
    control_url: String,
    agent_id: uuid::Uuid,
    data_dir: PathBuf,
}

impl Drop for TestAgent {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.data_dir).ok();
    }
}

async fn spawn_test_agent(heartbeat_interval: Duration, metrics_interval: Duration) -> TestAgent {
    let identity = AgentIdentity {
        id: uuid::Uuid::new_v4(),
        created_at: chrono::Utc::now(),
    };
    let info = Arc::new(AgentInfo::collect(&identity));
    let data_dir = std::env::temp_dir().join(format!("vibessh-agent-test-{}", uuid::Uuid::new_v4()));

    let tls_paths = vibe_agent::tls::load_or_create(&data_dir).expect("failed to generate a test TLS certificate");
    let tls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(&tls_paths.cert_path, &tls_paths.key_path)
        .await
        .expect("failed to load the test TLS certificate");

    let ws_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ws_addr = ws_listener.local_addr().unwrap();
    let control_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let control_addr = control_listener.local_addr().unwrap();

    let state = SharedState {
        info: info.clone(),
        data_dir: data_dir.clone(),
        pairing: PairingRegistry::new(),
        heartbeat_interval,
        metrics_interval,
    };

    tokio::spawn(transport::serve(ws_listener, state.clone(), tls_config));
    tokio::spawn(transport::serve_control(control_listener, state));

    TestAgent {
        ws_url: format!("wss://{ws_addr}/ws"),
        control_url: format!("http://{control_addr}/internal/pair"),
        agent_id: info.id,
        data_dir,
    }
}

async fn handshake_with(url: &str, auth_token: Option<&str>) -> HandshakeResponse {
    let mut ws = connect_insecure(url).await;
    let request = HandshakeRequest {
        protocol_version: PROTOCOL_VERSION,
        client_name: "vibessh-desktop-test".into(),
        client_version: "0.0.0".into(),
        auth_token: auth_token.map(str::to_string),
    };
    ws.send(Message::Text(serde_json::to_string(&request).unwrap()))
        .await
        .unwrap();
    next_json(&mut ws).await
}

/// Registers a code with the real control HTTP endpoint, using the same
/// `ureq` client the `vibe-agent pair` CLI does (on a blocking thread so it
/// doesn't stall the single-threaded test runtime the axum server task also
/// needs to make progress on).
async fn register_code_via_http(control_url: &str, code: &str) {
    let url = control_url.to_string();
    let code = code.to_string();
    let ok = tokio::task::spawn_blocking(move || {
        ureq::post(&url)
            .send_json(ureq::json!({ "code": code }))
            .map(|response| response.status() == 200)
            .unwrap_or(false)
    })
    .await
    .unwrap();
    assert!(ok, "control endpoint did not accept the pairing code");
}

#[tokio::test]
async fn handshake_rejects_mismatched_protocol_version() {
    let agent = spawn_test_agent(Duration::from_secs(30), Duration::from_secs(30)).await;
    let mut ws = connect_insecure(&agent.ws_url).await;

    let request = HandshakeRequest {
        protocol_version: PROTOCOL_VERSION + 1,
        client_name: "vibessh-desktop-test".into(),
        client_version: "0.0.0".into(),
        auth_token: None,
    };
    ws.send(Message::Text(serde_json::to_string(&request).unwrap()))
        .await
        .unwrap();

    let response: HandshakeResponse = next_json(&mut ws).await;
    assert!(!response.accepted);
    assert_eq!(response.error, Some(ProtocolErrorCode::VersionMismatch));
}

#[tokio::test]
async fn handshake_rejects_missing_or_unknown_token() {
    let agent = spawn_test_agent(Duration::from_secs(30), Duration::from_secs(30)).await;

    let response = handshake_with(&agent.ws_url, None).await;
    assert!(!response.accepted);
    assert_eq!(response.error, Some(ProtocolErrorCode::Unauthorized));

    let response = handshake_with(&agent.ws_url, Some("not-a-real-code-or-credential")).await;
    assert!(!response.accepted);
    assert_eq!(response.error, Some(ProtocolErrorCode::Unauthorized));
}

#[tokio::test]
async fn pairing_via_control_endpoint_issues_a_credential_and_burns_the_code() {
    let agent = spawn_test_agent(Duration::from_millis(200), Duration::from_secs(30)).await;
    register_code_via_http(&agent.control_url, "VIBE-TEST-CODE").await;

    let response = handshake_with(&agent.ws_url, Some("VIBE-TEST-CODE")).await;
    assert!(response.accepted);
    assert_eq!(response.agent_id, agent.agent_id);
    let credential = response.issued_credential.expect("expected a newly issued credential");

    // The code was single-use - a second attempt with it must fail now.
    let replay = handshake_with(&agent.ws_url, Some("VIBE-TEST-CODE")).await;
    assert!(!replay.accepted);
    assert_eq!(replay.error, Some(ProtocolErrorCode::Unauthorized));

    // The issued credential, on the other hand, works for reconnecting -
    // and does NOT mint another credential.
    let reconnect = handshake_with(&agent.ws_url, Some(&credential)).await;
    assert!(reconnect.accepted);
    assert!(reconnect.issued_credential.is_none());
}

#[tokio::test]
async fn heartbeat_still_follows_a_successful_pairing_handshake() {
    let agent = spawn_test_agent(Duration::from_millis(100), Duration::from_secs(30)).await;
    register_code_via_http(&agent.control_url, "VIBE-TEST-CODE").await;

    let mut ws = connect_insecure(&agent.ws_url).await;
    let request = HandshakeRequest {
        protocol_version: PROTOCOL_VERSION,
        client_name: "vibessh-desktop-test".into(),
        client_version: "0.0.0".into(),
        auth_token: Some("VIBE-TEST-CODE".into()),
    };
    ws.send(Message::Text(serde_json::to_string(&request).unwrap()))
        .await
        .unwrap();
    let response: HandshakeResponse = next_json(&mut ws).await;
    assert!(response.accepted);

    let event: ServerEvent = next_json(&mut ws).await;
    assert!(matches!(event, ServerEvent::Heartbeat));
}

#[tokio::test]
async fn metrics_update_follows_a_successful_handshake_with_sane_values() {
    // Fast metrics, slow (effectively disabled) heartbeat - deterministic
    // proof this is a metrics.update, not a race against the heartbeat.
    let agent = spawn_test_agent(Duration::from_secs(30), Duration::from_millis(100)).await;
    register_code_via_http(&agent.control_url, "VIBE-TEST-CODE").await;

    let response = handshake_with(&agent.ws_url, Some("VIBE-TEST-CODE")).await;
    assert!(response.accepted);

    // Reconnect to read the event stream from a clean handshake instead of
    // consuming handshake_with's own connection, which it already closed.
    let mut ws = connect_insecure(&agent.ws_url).await;
    let request = HandshakeRequest {
        protocol_version: PROTOCOL_VERSION,
        client_name: "vibessh-desktop-test".into(),
        client_version: "0.0.0".into(),
        auth_token: Some(response.issued_credential.unwrap()),
    };
    ws.send(Message::Text(serde_json::to_string(&request).unwrap()))
        .await
        .unwrap();
    let _: HandshakeResponse = next_json(&mut ws).await;

    let event: ServerEvent = next_json(&mut ws).await;
    match event {
        ServerEvent::MetricsUpdate { metrics } => {
            assert!(metrics.ram_total_bytes > 0, "a real host always has some RAM");
            assert!(
                metrics.ram_used_bytes <= metrics.ram_total_bytes,
                "used RAM can't exceed total RAM"
            );
            assert!((0.0..=100.0 * num_cpus()).contains(&metrics.cpu_usage_percent));
        }
        other => panic!("expected MetricsUpdate, got {other:?}"),
    }
}

#[tokio::test]
async fn apply_desired_state_is_acked_with_the_same_revision_over_a_real_connection() {
    // Etap M3: proves the Desktop->Agent direction actually round-trips
    // over the real router (`agent::transport::connection::handle`), not
    // just that the two DTOs serialize (see `protocol::commands`'s own
    // unit test for that) - this is the one thing a pure-JSON test can't
    // cover.
    let agent = spawn_test_agent(Duration::from_secs(30), Duration::from_secs(30)).await;
    register_code_via_http(&agent.control_url, "VIBE-TEST-CODE").await;

    let mut ws = connect_insecure(&agent.ws_url).await;
    let request = HandshakeRequest {
        protocol_version: PROTOCOL_VERSION,
        client_name: "vibessh-desktop-test".into(),
        client_version: "0.0.0".into(),
        auth_token: Some("VIBE-TEST-CODE".into()),
    };
    ws.send(Message::Text(serde_json::to_string(&request).unwrap())).await.unwrap();
    let _: HandshakeResponse = next_json(&mut ws).await;

    let command = DesktopCommand::ApplyDesiredState { revision: 42, state: NodeDesiredState::default() };
    ws.send(Message::Text(serde_json::to_string(&command).unwrap())).await.unwrap();

    let event: ServerEvent = next_json(&mut ws).await;
    match event {
        ServerEvent::StateApplied { revision, ok, error } => {
            assert_eq!(revision, 42);
            assert!(ok);
            assert_eq!(error, None);
        }
        other => panic!("expected StateApplied, got {other:?}"),
    }
}

/// sysinfo's global_cpu_usage() is the sum across cores, so its sane upper
/// bound is 100% times the core count, not a flat 100.
fn num_cpus() -> f32 {
    std::thread::available_parallelism().map(|n| n.get() as f32).unwrap_or(1.0)
}

async fn next_json<T: serde::de::DeserializeOwned>(
    ws: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
) -> T {
    let message = timeout(Duration::from_secs(2), ws.next())
        .await
        .expect("timed out waiting for a message")
        .expect("stream ended")
        .expect("websocket error");
    match message {
        Message::Text(text) => serde_json::from_str(&text).expect("valid JSON for expected type"),
        other => panic!("expected a text frame, got {other:?}"),
    }
}
