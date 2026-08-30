//! Drives `SshSession::open_terminal` against a real, local `russh::server`
//! that implements an interactive PTY + shell (not just one-shot exec, see
//! `tests/ssh_client.rs`) - proves the request_pty/request_shell/data/
//! window_change/close plumbing in `ssh/client.rs` actually round-trips.

use std::sync::Arc;
use std::time::Duration;

use russh::keys::{Algorithm, PrivateKey};
use russh::server::{Auth, ChannelOpenHandle, Handler, Msg, Server as _, Session};
use russh::{Channel, ChannelId, Pty, Preferred};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
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

/// Simulates a shell that echoes back whatever it receives, prefixed so a
/// test can tell an echoed response apart from anything else.
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

    #[allow(clippy::too_many_arguments)]
    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        _col_width: u32,
        _row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        Ok(())
    }

    async fn shell_request(&mut self, channel: ChannelId, session: &mut Session) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        Ok(())
    }

    async fn data(&mut self, channel: ChannelId, data: &[u8], session: &mut Session) -> Result<(), Self::Error> {
        let mut response = b"echo: ".to_vec();
        response.extend_from_slice(data);
        session.data(channel, response)?;
        Ok(())
    }
}

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

fn credentials(port: u16) -> SshCredentials {
    SshCredentials {
        host: "127.0.0.1".to_string(),
        port,
        username: TEST_USER.to_string(),
        auth: SshAuth::Password(TEST_PASSWORD.to_string()),
    }
}

#[tokio::test]
async fn opens_a_terminal_round_trips_input_and_output_and_closes_cleanly() {
    let port = spawn_mock_server().await;
    let outcome = timeout(Duration::from_secs(5), connect(&credentials(port), None))
        .await
        .expect("timed out connecting")
        .expect("connect should succeed");

    let (output_tx, mut output_rx) = mpsc::unbounded_channel::<String>();
    let (closed_tx, closed_rx) = oneshot::channel::<Option<String>>();

    let terminal = timeout(
        Duration::from_secs(5),
        outcome.session.open_terminal(
            80,
            24,
            move |chunk| {
                let _ = output_tx.send(chunk);
            },
            move |reason| {
                let _ = closed_tx.send(reason);
            },
        ),
    )
    .await
    .expect("timed out opening the terminal")
    .expect("opening a terminal should succeed");

    terminal.write(b"hello\n".to_vec());
    let received = timeout(Duration::from_secs(5), output_rx.recv())
        .await
        .expect("timed out waiting for echoed output")
        .expect("output channel closed unexpectedly");
    assert_eq!(received, "echo: hello\n");

    // A resize must not disrupt the session - one more round trip after it
    // should still work exactly as before.
    terminal.resize(100, 40);
    terminal.write(b"still alive\n".to_vec());
    let received = timeout(Duration::from_secs(5), output_rx.recv())
        .await
        .expect("timed out waiting for echoed output after resize")
        .expect("output channel closed unexpectedly");
    assert_eq!(received, "echo: still alive\n");

    drop(terminal);
    let closed_reason = timeout(Duration::from_secs(5), closed_rx)
        .await
        .expect("timed out waiting for the closed callback")
        .expect("closed sender dropped without sending");
    assert_eq!(closed_reason, None, "dropping the handle should be a clean close, not an error");

    outcome.session.close().await;
}
