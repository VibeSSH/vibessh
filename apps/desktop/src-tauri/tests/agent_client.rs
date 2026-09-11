//! Proves Etap D/E from the desktop side against a bare mock agent (no real
//! `vibe-agent` binary involved, just raw tokio-tungstenite): handshake
//! succeeds, an issued credential surfaces to the caller, heartbeats never
//! reach the caller, and a real event does.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;

use vibessh_lib::agent_client::{run, AgentClientConfig, AgentConnectionState};
use vibessh_protocol::{
    AgentCapabilities, DesktopCommand, HandshakeRequest, HandshakeResponse, LogLine, NodeDesiredState, ServerEvent, PROTOCOL_VERSION,
};

#[tokio::test]
async fn connects_swallows_heartbeats_and_forwards_real_events() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let agent_id = uuid::Uuid::new_v4();

    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();

        // Read (and ignore the contents of) the client's handshake request.
        let Some(Ok(Message::Text(text))) = ws.next().await else {
            panic!("expected a handshake request");
        };
        let _: HandshakeRequest = serde_json::from_str(&text).unwrap();

        let response = HandshakeResponse {
            accepted: true,
            agent_id,
            agent_version: "0.0.0-mock".into(),
            protocol_version: PROTOCOL_VERSION,
            error: None,
            issued_credential: Some("mock-issued-credential".into()),
            capabilities: AgentCapabilities {
                terminal: true,
                ..Default::default()
            },
        };
        ws.send(Message::Text(serde_json::to_string(&response).unwrap()))
            .await
            .unwrap();

        // A heartbeat the client must swallow, then a real event it must forward.
        ws.send(Message::Text(
            serde_json::to_string(&ServerEvent::Heartbeat).unwrap(),
        ))
        .await
        .unwrap();
        let log_event = ServerEvent::LogsLine(LogLine {
            source: "test".into(),
            line: "hello from mock agent".into(),
            timestamp: chrono::Utc::now(),
            // No follow id: this asserts the transport carries a log event
            // at all, which predates follows and must keep working for an
            // Agent that never asks for one.
            follow_id: None,
        });
        ws.send(Message::Text(serde_json::to_string(&log_event).unwrap()))
            .await
            .unwrap();

        // Keep the socket open for the rest of the test instead of racing its teardown.
        tokio::time::sleep(Duration::from_secs(5)).await;
    });

    let config = AgentClientConfig {
        url: format!("ws://{addr}/ws"),
        client_name: "vibessh-desktop-test".into(),
        client_version: "0.0.0".into(),
        auth_token: Some("VIBE-TEST-PAIRING-CODE".into()),
        known_fingerprint: None,
    };
    let (events_tx, mut events_rx) = mpsc::channel(8);
    let (state_tx, mut state_rx) = watch::channel(AgentConnectionState::Connecting);
    let (_command_tx, command_rx) = mpsc::channel(8);

    let client_task = tokio::spawn(run(config, events_tx, state_tx, command_rx));

    // Wait for the Connected state instead of a fixed sleep.
    let (connected, issued_credential, capabilities) = timeout(Duration::from_secs(2), async {
        loop {
            if let AgentConnectionState::Connected {
                agent_id: got_id,
                issued_credential,
                capabilities,
                ..
            } = &*state_rx.borrow_and_update()
            {
                return (*got_id, issued_credential.clone(), *capabilities);
            }
            state_rx.changed().await.unwrap();
        }
    })
    .await
    .expect("timed out waiting for Connected state");
    assert_eq!(connected, agent_id);
    assert_eq!(issued_credential.as_deref(), Some("mock-issued-credential"));
    assert!(capabilities.terminal);
    assert!(!capabilities.docker);

    let event = timeout(Duration::from_secs(2), events_rx.recv())
        .await
        .expect("timed out waiting for an event")
        .expect("event channel closed unexpectedly");
    match event {
        ServerEvent::LogsLine(line) => assert_eq!(line.line, "hello from mock agent"),
        other => panic!("expected LogsLine, got {other:?}"),
    }

    // The heartbeat must never have been forwarded - there should be nothing
    // else waiting on the channel besides what we already consumed.
    assert!(events_rx.try_recv().is_err());

    client_task.abort();
}

#[tokio::test]
async fn a_command_sent_before_the_connection_exists_is_queued_and_delivered_once_connected() {
    // Etap M3: proves `command_rx` is genuinely reused across the
    // connect-then-stream lifecycle, not just accepted by the function
    // signature - the command is sent into the channel before the mock
    // agent even starts listening, so this only passes if `run` really
    // holds onto it and flushes it once the handshake completes.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let (events_tx, _events_rx) = mpsc::channel(8);
    let (state_tx, mut state_rx) = watch::channel(AgentConnectionState::Connecting);
    let (command_tx, command_rx) = mpsc::channel(8);

    command_tx
        .send(DesktopCommand::ApplyDesiredState { revision: 3, state: NodeDesiredState::default() })
        .await
        .unwrap();

    let config = AgentClientConfig {
        url: format!("ws://{addr}/ws"),
        client_name: "vibessh-desktop-test".into(),
        client_version: "0.0.0".into(),
        auth_token: Some("VIBE-TEST-PAIRING-CODE".into()),
        known_fingerprint: None,
    };
    let client_task = tokio::spawn(run(config, events_tx, state_tx, command_rx));

    let (tcp, _) = listener.accept().await.unwrap();
    let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
    let Some(Ok(Message::Text(_))) = ws.next().await else {
        panic!("expected a handshake request");
    };
    let response = HandshakeResponse {
        accepted: true,
        agent_id: uuid::Uuid::new_v4(),
        agent_version: "0.0.0-mock".into(),
        protocol_version: PROTOCOL_VERSION,
        error: None,
        issued_credential: None,
        capabilities: AgentCapabilities::default(),
    };
    ws.send(Message::Text(serde_json::to_string(&response).unwrap())).await.unwrap();

    let received = timeout(Duration::from_secs(2), ws.next())
        .await
        .expect("timed out waiting for the queued command to arrive")
        .expect("stream ended")
        .expect("websocket error");
    let Message::Text(text) = received else { panic!("expected a text frame") };
    let command: DesktopCommand = serde_json::from_str(&text).unwrap();
    let DesktopCommand::ApplyDesiredState { revision, .. } = command else {
        panic!("expected state.apply, got {command:?}")
    };
    assert_eq!(revision, 3);

    let _ = state_rx.changed().await;
    client_task.abort();
}
