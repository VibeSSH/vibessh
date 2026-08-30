//! Low-level SSH connection: connect, verify the host key (TOFU), authenticate,
//! run one command. No knowledge of `Server`/keyring/SQLite here on purpose -
//! `ssh_service` resolves those into a `SshCredentials` + the previously known
//! fingerprint before calling `connect`, keeping this module pure protocol
//! mechanics and independently testable against a bare SSH server.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::keys::{load_secret_key, HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{client, ChannelMsg, Disconnect};
use russh_sftp::client::SftpSession;
use tokio::sync::{mpsc, OnceCell};

use crate::errors::{AppError, AppResult};
use vibessh_protocol::CommandOutput;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// SSH's "stderr" extended-data stream id, per RFC 4254 5.2.
const SSH_EXTENDED_DATA_STDERR: u32 = 1;

pub struct SshCredentials {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: SshAuth,
}

pub enum SshAuth {
    Password(String),
    PrivateKey { path: String, passphrase: Option<String> },
}

/// A live, authenticated connection to one server. Each `execute_command`
/// opens its own channel over this connection - cheap, and means one long
/// command can't block a concurrent one the way a single shared channel would.
/// `sftp` is different: negotiating the subsystem is comparatively
/// expensive, and every `SftpSession` method only needs `&self`, so it's
/// opened lazily on first use and reused for every SFTP call after - see
/// `ssh/sftp.rs`.
pub struct SshSession {
    handle: client::Handle<TofuHandler>,
    sftp: OnceCell<SftpSession>,
}

/// What `connect` produced: the session itself, plus the host key fingerprint
/// it saw. Callers persist that fingerprint (keyed by server id) the first
/// time they connect to a given server, then pass it back in on every later
/// call so a changed fingerprint gets caught instead of silently trusted.
pub struct ConnectOutcome {
    pub session: SshSession,
    pub host_key_fingerprint: String,
}

pub async fn connect(credentials: &SshCredentials, known_fingerprint: Option<String>) -> AppResult<ConnectOutcome> {
    let seen = Arc::new(Mutex::new(SeenHostKey::default()));
    let handler = TofuHandler {
        expected_fingerprint: known_fingerprint,
        seen: seen.clone(),
    };

    let config = Arc::new(client::Config {
        inactivity_timeout: Some(Duration::from_secs(60)),
        ..Default::default()
    });

    let addr = (credentials.host.as_str(), credentials.port);
    let mut handle = tokio::time::timeout(CONNECT_TIMEOUT, client::connect(config, addr, handler))
        .await
        .map_err(|_| AppError::Connection(format!("timed out connecting to {}:{}", credentials.host, credentials.port)))?
        .map_err(|err| classify_connect_error(&err, &seen))?;

    let auth_result = match &credentials.auth {
        SshAuth::Password(password) => handle
            .authenticate_password(&credentials.username, password)
            .await
            .map_err(|err| AppError::Connection(format!("SSH authentication failed: {err}")))?,
        SshAuth::PrivateKey { path, passphrase } => {
            let key_pair = load_secret_key(Path::new(path), passphrase.as_deref())
                .map_err(|err| AppError::InvalidInput(format!("couldn't load the private key at {path}: {err}")))?;
            let hash_alg = handle
                .best_supported_rsa_hash()
                .await
                .map_err(|err| AppError::Connection(format!("SSH negotiation failed: {err}")))?
                .flatten();
            handle
                .authenticate_publickey(&credentials.username, PrivateKeyWithHashAlg::new(Arc::new(key_pair), hash_alg))
                .await
                .map_err(|err| AppError::Connection(format!("SSH authentication failed: {err}")))?
        }
    };

    if !auth_result.success() {
        return Err(AppError::InvalidInput(
            "SSH authentication was rejected - check the username, password, or key".into(),
        ));
    }

    let host_key_fingerprint = seen
        .lock()
        .expect("host key mutex poisoned")
        .fingerprint
        .clone()
        .expect("check_server_key always runs during key exchange, before connect() can resolve");

    Ok(ConnectOutcome {
        session: SshSession {
            handle,
            sftp: OnceCell::new(),
        },
        host_key_fingerprint,
    })
}

impl SshSession {
    pub async fn execute_command(&self, command: &str) -> AppResult<CommandOutput> {
        let mut channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(|err| AppError::Connection(format!("couldn't open an SSH channel: {err}")))?;
        channel
            .exec(true, command)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't run the command: {err}")))?;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_code = None;

        while let Some(msg) = channel.wait().await {
            match msg {
                ChannelMsg::Data { data } => stdout.extend_from_slice(&data),
                ChannelMsg::ExtendedData { data, ext } if ext == SSH_EXTENDED_DATA_STDERR => {
                    stderr.extend_from_slice(&data);
                }
                ChannelMsg::ExitStatus { exit_status } => exit_code = Some(exit_status as i32),
                _ => {}
            }
        }

        Ok(CommandOutput {
            exit_code: exit_code.unwrap_or(-1),
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        })
    }

    pub async fn close(&self) {
        let _ = self.handle.disconnect(Disconnect::ByApplication, "", "en").await;
    }

    /// Lazily negotiates the SFTP subsystem on first use and reuses it for
    /// every call after - see `ssh/sftp.rs`, which is the only other caller.
    pub(super) async fn sftp(&self) -> AppResult<&SftpSession> {
        self.sftp
            .get_or_try_init(|| async {
                let channel = self
                    .handle
                    .channel_open_session()
                    .await
                    .map_err(|err| AppError::Connection(format!("couldn't open an SFTP channel: {err}")))?;
                channel
                    .request_subsystem(true, "sftp")
                    .await
                    .map_err(|err| AppError::Connection(format!("couldn't start the SFTP subsystem: {err}")))?;
                SftpSession::new(channel.into_stream())
                    .await
                    .map_err(|err| AppError::Connection(format!("SFTP handshake failed: {err}")))
            })
            .await
    }

    /// Opens an interactive PTY + shell and spawns a background task that
    /// drives it for as long as the returned `TerminalHandle` lives: remote
    /// output is forwarded to `on_output` as it arrives, and the task ends
    /// (calling `on_closed` once) when either side closes the channel or
    /// the handle - and with it, its input channel - is dropped.
    pub async fn open_terminal(
        &self,
        cols: u32,
        rows: u32,
        mut on_output: impl FnMut(String) + Send + 'static,
        on_closed: impl FnOnce(Option<String>) + Send + 'static,
    ) -> AppResult<TerminalHandle> {
        let channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(|err| AppError::Connection(format!("couldn't open a terminal channel: {err}")))?;
        channel
            .request_pty(true, "xterm-256color", cols, rows, 0, 0, &[])
            .await
            .map_err(|err| AppError::Connection(format!("couldn't request a PTY: {err}")))?;
        channel
            .request_shell(true)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't start a shell: {err}")))?;

        let (input_tx, mut input_rx) = mpsc::unbounded_channel::<TerminalInput>();

        tokio::spawn(async move {
            let mut channel = channel;
            let close_reason = loop {
                tokio::select! {
                    input = input_rx.recv() => {
                        match input {
                            Some(TerminalInput::Data(data)) => {
                                if channel.data_bytes(data).await.is_err() {
                                    break Some("failed to send input to the remote shell".to_string());
                                }
                            }
                            Some(TerminalInput::Resize { cols, rows }) => {
                                let _ = channel.window_change(cols, rows, 0, 0).await;
                            }
                            // The TerminalHandle (and its sender) was dropped -
                            // the UI closed this terminal from its side.
                            None => {
                                let _ = channel.close().await;
                                break None;
                            }
                        }
                    }
                    msg = channel.wait() => {
                        match msg {
                            Some(ChannelMsg::Data { data }) => on_output(String::from_utf8_lossy(&data).into_owned()),
                            Some(ChannelMsg::ExtendedData { data, .. }) => {
                                on_output(String::from_utf8_lossy(&data).into_owned());
                            }
                            Some(ChannelMsg::Close) | None => break None,
                            _ => {}
                        }
                    }
                }
            };
            on_closed(close_reason);
        });

        Ok(TerminalHandle { input_tx })
    }
}

/// A handle to a running interactive shell, opened by `SshSession::open_terminal`.
/// Dropping it ends the underlying background task and closes the remote
/// channel - there's no separate `close()` to remember to call.
pub struct TerminalHandle {
    input_tx: mpsc::UnboundedSender<TerminalInput>,
}

enum TerminalInput {
    Data(Vec<u8>),
    Resize { cols: u32, rows: u32 },
}

impl TerminalHandle {
    pub fn write(&self, data: Vec<u8>) {
        let _ = self.input_tx.send(TerminalInput::Data(data));
    }

    pub fn resize(&self, cols: u32, rows: u32) {
        let _ = self.input_tx.send(TerminalInput::Resize { cols, rows });
    }
}

#[derive(Default)]
struct SeenHostKey {
    fingerprint: Option<String>,
    mismatched: bool,
}

/// Trust-On-First-Use: `expected_fingerprint` is `None` on a server's very
/// first connection (nothing recorded yet, so anything is trusted and
/// reported back via `seen` for the caller to persist) and `Some` on every
/// connection after (a mismatch is rejected, not silently accepted - a
/// changed host key means the server was reinstalled or something is
/// intercepting the connection, and either way the user should be told
/// rather than have VibeSSH quietly proceed).
struct TofuHandler {
    expected_fingerprint: Option<String>,
    seen: Arc<Mutex<SeenHostKey>>,
}

impl client::Handler for TofuHandler {
    type Error = russh::Error;

    async fn check_server_key(&mut self, server_public_key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let fingerprint = server_public_key.public_key().fingerprint(HashAlg::Sha256).to_string();
        let mut seen = self.seen.lock().expect("host key mutex poisoned");
        seen.fingerprint = Some(fingerprint.clone());

        match &self.expected_fingerprint {
            None => Ok(true),
            Some(expected) if *expected == fingerprint => Ok(true),
            Some(_) => {
                seen.mismatched = true;
                Ok(false)
            }
        }
    }
}

fn classify_connect_error(err: &russh::Error, seen: &Arc<Mutex<SeenHostKey>>) -> AppError {
    if seen.lock().expect("host key mutex poisoned").mismatched {
        AppError::Connection(
            "the server's SSH host key doesn't match the one VibeSSH saw before - this can mean the \
             server was reinstalled, but it can also mean someone is intercepting the connection. \
             Verify the server before trusting it again."
                .to_string(),
        )
    } else {
        AppError::Connection(format!("SSH connection failed: {err}"))
    }
}
