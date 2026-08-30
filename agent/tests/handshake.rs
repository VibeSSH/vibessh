//! Proves Etap D end-to-end against the real router (not a reimplementation):
//! a bare WebSocket client performs the handshake and receives a heartbeat,
//! and a client on the wrong protocol version gets rejected with a reason.

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use vibe_agent::identity::AgentIdentity;
use vibe_agent::info::AgentInfo;
use vibe_agent::transport::{self, SharedState};
use vibessh_protocol::{HandshakeRequest, HandshakeResponse, ProtocolErrorCode, ServerEvent, PROTOCOL_VERSION};

async fn spawn_test_server(heartbeat_interval: Duration) -> (String, Arc<AgentInfo>) {
    let identity = AgentIdentity {
        id: uuid::Uuid::new_v4(),
        created_at: chrono::Utc::now(),
    };
    let info = Arc::new(AgentInfo::collect(&identity));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let state = SharedState {
        info: info.clone(),
        heartbeat_interval,
    };
    tokio::spawn(transport::serve(listener, state));

    (format!("ws://{addr}/ws"), info)
}

#[tokio::test]
async fn handshake_succeeds_and_heartbeat_follows() {
    let (url, info) = spawn_test_server(Duration::from_millis(100)).await;
    let (mut ws, _) = connect_async(&url).await.expect("connect");

    let request = HandshakeRequest {
        protocol_version: PROTOCOL_VERSION,
        client_name: "vibessh-desktop-test".into(),
        client_version: "0.0.0".into(),
        auth_token: None,
    };
    ws.send(Message::Text(serde_json::to_string(&request).unwrap()))
        .await
        .unwrap();

    let response: HandshakeResponse = next_json(&mut ws).await;
    assert!(response.accepted);
    assert_eq!(response.agent_id, info.id);
    assert_eq!(response.protocol_version, PROTOCOL_VERSION);
    assert!(response.error.is_none());

    let event: ServerEvent = next_json(&mut ws).await;
    assert!(matches!(event, ServerEvent::Heartbeat));
}

#[tokio::test]
async fn handshake_rejects_mismatched_protocol_version() {
    let (url, _info) = spawn_test_server(Duration::from_secs(30)).await;
    let (mut ws, _) = connect_async(&url).await.expect("connect");

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
