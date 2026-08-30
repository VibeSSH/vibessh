//! Drives `ssh::connect`/`SshSession::execute_command` against a real, local
//! `russh::server` instance - not a reimplementation of the SSH protocol,
//! the genuine article, just on 127.0.0.1 instead of a real box. Proves the
//! connect/auth/exec/TOFU logic in `ssh/client.rs` actually works, the same
//! way `tests/agent_client.rs` proves `agent_client` against a mock WS
//! server rather than only against the real dedicated test server.

use std::sync::Arc;
use std::time::Duration;

use russh::keys::{Algorithm, PrivateKey};
use russh::server::{Auth, ChannelOpenHandle, Handler, Msg, Server as _, Session};
use russh::{Channel, ChannelId, Preferred};
use tokio::net::TcpListener;
use tokio::time::timeout;

use vibessh_lib::ssh::{connect, SshAuth, SshCredentials};

const TEST_USER: &str = "tester";
const TEST_PASSWORD: &str = "correct-horse-battery-staple";

#[derive(Clone)]
struct MockServer;

impl russh::server::Server for MockServer {
    type Handler = MockHandler;
    fn new_client(&mut self, _peer_addr: Option<std::net::SocketAddr>) -> MockHandler {
        MockHandler
    }
}

struct MockHandler;

impl Handler for MockHandler {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        if user == TEST_USER && password == TEST_PASSWORD {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    async fn exec_request(&mut self, channel: ChannelId, data: &[u8], session: &mut Session) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        let command = String::from_utf8_lossy(data);
        session.data(channel, format!("ran: {command}\n").into_bytes())?;
        session.extended_data(channel, 1, b"a warning on stderr\n".to_vec())?;
        session.exit_status_request(channel, 7)?;
        session.eof(channel)?;
        session.close(channel)?;
        Ok(())
    }
}

/// Binds on an ephemeral port, serves forever in the background, and
/// returns the port to connect to.
async fn spawn_mock_server() -> u16 {
    let config = Arc::new(russh::server::Config {
        keys: vec![PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap()],
        preferred: Preferred::default(),
        ..Default::default()
    });
    let socket = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = socket.local_addr().unwrap().port();

    tokio::spawn(async move {
        let mut server = MockServer;
        let _ = server.run_on_socket(config, &socket).await;
    });

    port
}

fn credentials(port: u16, password: &str) -> SshCredentials {
    SshCredentials {
        host: "127.0.0.1".to_string(),
        port,
        username: TEST_USER.to_string(),
        auth: SshAuth::Password(password.to_string()),
    }
}

#[tokio::test]
async fn connects_authenticates_and_runs_a_real_command() {
    let port = spawn_mock_server().await;

    let outcome = timeout(Duration::from_secs(5), connect(&credentials(port, TEST_PASSWORD), None))
        .await
        .expect("timed out connecting")
        .expect("connect should succeed with the right password");

    assert!(!outcome.host_key_fingerprint.is_empty());
    assert!(outcome.host_key_fingerprint.starts_with("SHA256:"));

    let output = outcome
        .session
        .execute_command("echo hi")
        .await
        .expect("command should run");

    assert_eq!(output.exit_code, 7);
    assert_eq!(output.stdout, "ran: echo hi\n");
    assert_eq!(output.stderr, "a warning on stderr\n");

    outcome.session.close().await;
}

#[tokio::test]
async fn wrong_password_is_rejected() {
    let port = spawn_mock_server().await;

    let result = timeout(Duration::from_secs(5), connect(&credentials(port, "not-the-password"), None))
        .await
        .expect("timed out connecting");

    let err = match result {
        Ok(_) => panic!("auth should have been rejected"),
        Err(err) => err,
    };
    assert!(err.to_string().to_lowercase().contains("rejected"), "unexpected error: {err}");
}

#[tokio::test]
async fn first_connection_trusts_and_reports_the_host_key() {
    let port = spawn_mock_server().await;

    let outcome = timeout(Duration::from_secs(5), connect(&credentials(port, TEST_PASSWORD), None))
        .await
        .expect("timed out connecting")
        .expect("first connection should succeed and trust the key");

    // Reconnecting with the exact fingerprint just reported back should
    // still succeed - this is the "every connection after the first" case.
    let second = timeout(
        Duration::from_secs(5),
        connect(&credentials(port, TEST_PASSWORD), Some(outcome.host_key_fingerprint.clone())),
    )
    .await
    .expect("timed out connecting")
    .expect("a matching known fingerprint should be accepted");

    assert_eq!(second.host_key_fingerprint, outcome.host_key_fingerprint);
}

#[tokio::test]
async fn a_changed_host_key_is_rejected_not_silently_trusted() {
    let port = spawn_mock_server().await;

    // A fingerprint that doesn't match anything the mock server will ever
    // present - simulates connecting to a server whose key changed
    // (reinstall, or an active MITM presenting a different key).
    let bogus_expected_fingerprint = "SHA256:this-is-definitely-not-the-real-key".to_string();

    let result = timeout(
        Duration::from_secs(5),
        connect(&credentials(port, TEST_PASSWORD), Some(bogus_expected_fingerprint)),
    )
    .await
    .expect("timed out connecting");

    let err = match result {
        Ok(_) => panic!("a host key mismatch must not be silently accepted"),
        Err(err) => err,
    };
    let message = err.to_string().to_lowercase();
    assert!(
        message.contains("host key") && message.contains("match"),
        "expected a host-key-mismatch error, got: {err}"
    );
}

