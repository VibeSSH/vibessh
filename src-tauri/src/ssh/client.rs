//! Low-level SSH connection: connect, verify the host key (TOFU), authenticate,
//! run one command. No knowledge of `Server`/keyring/SQLite here on purpose -
//! `ssh_service` resolves those into a `SshCredentials` + the previously known
//! fingerprint before calling `connect`, keeping this module pure protocol
//! mechanics and independently testable against a bare SSH server.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use russh::keys::{load_secret_key, HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{client, Channel, ChannelMsg, Disconnect};
use russh_sftp::client::SftpSession;
use tokio::sync::{mpsc, OnceCell};

use crate::errors::{AppError, AppResult};
use vibessh_protocol::CommandOutput;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// How long a single `execute_command` may run before it is given up on.
///
/// There was no command timeout at all: `CONNECT_TIMEOUT` covered only the
/// initial connect, so a command that never finished - a hung `apt-get`
/// waiting on a lock, a `docker pull` against an unreachable registry -
/// blocked the calling Tauri command forever, with no cancellation and no
/// way for the UI to recover.
///
/// Generous on purpose. This codebase legitimately runs slow commands over
/// this channel (`apt-get install mariadb-server`, `docker pull` of a
/// multi-gigabyte image), so the value has to be "something is wrong", not
/// "this is taking a while". Ten minutes is well past any of them and still
/// far short of forever.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(600);

/// Keepalive, not a deadline.
///
/// This used to be a 60-second `inactivity_timeout`, which tore down the
/// whole session - every channel on it - after a minute of silence on the
/// wire. That directly contradicted the long-running commands above: a real
/// `apt-get install` routinely produces no output for longer than a minute
/// while it unpacks, and the session died underneath it.
///
/// Sending a keepalive every 30 seconds keeps the connection demonstrably
/// alive instead, and the per-command `COMMAND_TIMEOUT` is what bounds a
/// command that genuinely never returns.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);

/// How much of a command's output is kept before the rest is discarded.
///
/// There was no cap: `execute_command` accumulated stdout and stderr into
/// unbounded `Vec<u8>`s, so a mistyped `cat` of a large file, a wide `find`,
/// or a runaway process writing to stderr pulled the whole thing into the
/// desktop's memory. With `panic = "abort"` in the release profile, running
/// out of memory there kills the app rather than failing the command.
///
/// 8 MiB is far past every command this codebase actually issues - the
/// largest are `docker logs --tail 5000` and a `find` listing - so hitting
/// it means something has gone wrong, and the truncation notice says so.
const MAX_COMMAND_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

/// Appends up to the cap, and reports whether anything had to be dropped.
fn append_capped(buffer: &mut Vec<u8>, data: &[u8], truncated: &mut bool) {
    let remaining = MAX_COMMAND_OUTPUT_BYTES.saturating_sub(buffer.len());
    if remaining == 0 {
        *truncated = true;
        return;
    }
    if data.len() > remaining {
        buffer.extend_from_slice(&data[..remaining]);
        *truncated = true;
    } else {
        buffer.extend_from_slice(data);
    }
}
/// SSH's "stderr" extended-data stream id, per RFC 4254 5.2.
const SSH_EXTENDED_DATA_STDERR: u32 = 1;

/// Routes an incoming `forwarded-tcpip` channel (someone connected to a
/// Remote-forwarded port on the Node's side) back to whichever
/// `start_remote_forward` call registered that port - keyed by the actual
/// bound port, shared between `TofuHandler` (which receives the channel
/// from the SSH protocol layer) and `SshSession` (which registers/
/// unregisters ports as forwards start/stop). See `ssh::port_forward`'s own
/// doc comment for why the rest of the forwarding logic lives there instead
/// of here.
type ForwardRegistry = Arc<Mutex<HashMap<u32, mpsc::UnboundedSender<Channel<client::Msg>>>>>;

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
    /// CPU% and network rates are deltas between two samples, not values a
    /// single `/proc` read gives you directly - see `ssh/monitor.rs`. `None`
    /// on the very first call, same as `agent::metrics::MetricsCollector`
    /// reports 0 rather than a meaningless number for a sample it has no
    /// prior point to compare against.
    metrics_sample: Mutex<Option<MetricsSample>>,
    forward_registry: ForwardRegistry,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct MetricsSample {
    pub(super) cpu_idle_jiffies: u64,
    pub(super) cpu_total_jiffies: u64,
    pub(super) rx_bytes: u64,
    pub(super) tx_bytes: u64,
    pub(super) at: Instant,
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
    let forward_registry: ForwardRegistry = Arc::new(Mutex::new(HashMap::new()));
    let handler = TofuHandler {
        expected_fingerprint: known_fingerprint,
        seen: seen.clone(),
        forward_registry: forward_registry.clone(),
    };

    let config = Arc::new(client::Config {
        keepalive_interval: Some(KEEPALIVE_INTERVAL),
        inactivity_timeout: None,
        ..Default::default()
    });

    let addr = (credentials.host.as_str(), credentials.port);
    let mut handle = tokio::time::timeout(CONNECT_TIMEOUT, client::connect(config, addr, handler))
        .await
        .map_err(|_| AppError::Timeout { operation: "connecting", seconds: CONNECT_TIMEOUT.as_secs() })?
        .map_err(|err| classify_connect_error(&err, &seen, &credentials.host))?;

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
            metrics_sample: Mutex::new(None),
            forward_registry,
        },
        host_key_fingerprint,
    })
}

impl SshSession {
    /// Bounded by `COMMAND_TIMEOUT` - see that constant for why there is a
    /// bound at all, and why it is as generous as it is.
    pub async fn execute_command(&self, command: &str) -> AppResult<CommandOutput> {
        tokio::time::timeout(COMMAND_TIMEOUT, self.execute_command_inner(command))
            .await
            .unwrap_or_else(|_| Err(AppError::Timeout { operation: "the command", seconds: COMMAND_TIMEOUT.as_secs() }))
    }

    async fn execute_command_inner(&self, command: &str) -> AppResult<CommandOutput> {
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
        let mut truncated = false;

        while let Some(msg) = channel.wait().await {
            match msg {
                ChannelMsg::Data { data } => append_capped(&mut stdout, &data, &mut truncated),
                ChannelMsg::ExtendedData { data, ext } if ext == SSH_EXTENDED_DATA_STDERR => {
                    append_capped(&mut stderr, &data, &mut truncated);
                }
                ChannelMsg::ExitStatus { exit_status } => exit_code = Some(exit_status as i32),
                _ => {}
            }
        }

        let mut stderr = String::from_utf8_lossy(&stderr).into_owned();
        if truncated {
            // Appended to stderr rather than silently dropped: a caller that
            // parses stdout would otherwise see a plausible-looking but
            // incomplete result with nothing to indicate it.
            stderr.push_str(&format!(
                "\n[vibessh] output exceeded {} MiB and was truncated",
                MAX_COMMAND_OUTPUT_BYTES / (1024 * 1024)
            ));
        }
        Ok(CommandOutput { exit_code: exit_code.unwrap_or(-1), stdout: String::from_utf8_lossy(&stdout).into_owned(), stderr })
    }

    pub async fn close(&self) {
        let _ = self.handle.disconnect(Disconnect::ByApplication, "", "en").await;
    }

    /// Swaps in a freshly parsed sample and returns whatever was there
    /// before (`None` on the first call for this session) - `ssh/monitor.rs`
    /// diffs the two to get a real rate instead of a single-point-in-time
    /// number that doesn't mean anything for CPU%/network throughput.
    pub(super) fn swap_metrics_sample(&self, new_sample: MetricsSample) -> Option<MetricsSample> {
        self.metrics_sample.lock().expect("metrics sample mutex poisoned").replace(new_sample)
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


    /// Runs a command and streams its output line by line until the caller
    /// drops the returned handle.
    ///
    /// Its own method rather than a flag on `execute_command`, because the
    /// two have opposite shapes: `execute_command` collects everything,
    /// caps it and returns once, under a timeout. A follow never returns on
    /// its own and must not be capped or timed out - `docker logs -f` on a
    /// quiet container is *supposed* to sit there producing nothing.
    ///
    /// Lines are assembled here rather than in the caller. A channel
    /// boundary lands wherever TCP put it, frequently mid-line, and every
    /// consumer would otherwise have to reimplement the same buffering -
    /// the mistake `ai::openai_compatible` had to get right for SSE.
    ///
    /// stderr is folded into the same stream: `docker logs` writes a
    /// container's stderr there, and separating them would drop half of
    /// what a crashing process said.
    pub async fn follow_command(
        &self,
        command: &str,
        mut on_line: impl FnMut(String) + Send + 'static,
        on_closed: impl FnOnce(Option<String>) + Send + 'static,
    ) -> AppResult<FollowHandle> {
        let mut channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(|err| AppError::Connection(format!("couldn't open a log channel: {err}")))?;
        channel
            .exec(true, command)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't start following the log: {err}")))?;

        let (stop_tx, mut stop_rx) = mpsc::unbounded_channel::<()>();

        tokio::spawn(async move {
            let mut buffer = String::new();
            let mut exit_failure: Option<u32> = None;
            // How much arrived before it ended - the difference between "it
            // never produced anything" and "it worked and then stopped".
            let mut lines_seen: u64 = 0;
            let close_reason = loop {
                tokio::select! {
                    // The handle was dropped - the UI closed the console, or
                    // the whole page went away. Closing the channel stops
                    // `docker logs -f` on the Node rather than leaving it
                    // running and writing into a socket nobody reads.
                    stop = stop_rx.recv() => {
                        if stop.is_none() {
                            let _ = channel.close().await;
                            break None;
                        }
                    }
                    msg = channel.wait() => {
                        match msg {
                            Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                                buffer.push_str(&String::from_utf8_lossy(&data));
                                while let Some(newline) = buffer.find('\n') {
                                    let line = buffer[..newline].trim_end_matches('\r').to_string();
                                    buffer.drain(..=newline);
                                    lines_seen += 1;
                                    on_line(line);
                                }
                            }
                            // A follow that ends because its command failed
                            // has to say so. Without this the caller sees an
                            // ordinary close and silently falls back, which
                            // is indistinguishable from "this runtime has no
                            // follow" - and that ambiguity cost real time to
                            // diagnose once already.
                            Some(ChannelMsg::ExitStatus { exit_status }) => {
                                if exit_status != 0 {
                                    exit_failure = Some(exit_status);
                                }
                            }
                            Some(ChannelMsg::Close) | None => break None,
                            _ => {}
                        }
                    }
                }
            };
            // Whatever was left without a trailing newline is still output -
            // a process killed mid-line said it, and dropping it would hide
            // the last thing it managed to write.
            if !buffer.is_empty() {
                on_line(buffer);
            }
            let close_reason = close_reason.or_else(|| exit_failure.map(|code| format!("the command exited with status {code}")));
            // Logged whether or not there is a reason. A follow that ends
            // "normally" still ends, and the caller still falls back - so
            // logging only the explained endings left the common case
            // invisible, which is precisely the case that needed
            // explaining.
            match &close_reason {
                Some(reason) => log::warn!("a log follow ended after {lines_seen} line(s): {reason}"),
                None => log::info!("a log follow ended normally after {lines_seen} line(s)"),
            }
            on_closed(close_reason);
        });

        Ok(FollowHandle { _stop_tx: stop_tx })
    }

    /// Opens a `direct-tcpip` channel to `host_to_connect:port_to_connect` -
    /// the primitive `ssh::port_forward`'s Local/Dynamic forward accept
    /// loops call once per accepted local connection. `originator_*`
    /// identifies the local peer to the server (informational only, per RFC
    /// 4254 7.2 - no server this app talks to acts on it).
    pub(super) async fn open_direct_tcpip(
        &self,
        host_to_connect: &str,
        port_to_connect: u16,
        originator_address: &str,
        originator_port: u16,
    ) -> AppResult<Channel<client::Msg>> {
        self.handle
            .channel_open_direct_tcpip(host_to_connect, port_to_connect as u32, originator_address, originator_port as u32)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't open a tunnel to {host_to_connect}:{port_to_connect}: {err}")))
    }

    /// Asks the Node to start listening on `bind_address:bind_port` (`0` =
    /// any free port) and registers this session to receive whatever
    /// `forwarded-tcpip` channels that listener produces - see
    /// `TofuHandler::server_channel_open_forwarded_tcpip` below for the
    /// other half of the handoff. Returns the port actually bound (russh's
    /// own `tcpip_forward` only reports a real value back when `0` was
    /// requested - it returns `0` for an explicitly chosen port, so the
    /// request's own port is what gets used in that case).
    pub(super) async fn register_remote_forward(&self, bind_address: &str, bind_port: u16) -> AppResult<(u16, mpsc::UnboundedReceiver<Channel<client::Msg>>)> {
        let returned_port = self
            .handle
            .tcpip_forward(bind_address, bind_port as u32)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't ask the Node to listen on {bind_address}:{bind_port}: {err}")))?;
        let actual_port = if bind_port == 0 { returned_port as u16 } else { bind_port };

        let (sender, receiver) = mpsc::unbounded_channel();
        self.forward_registry.lock().expect("forward registry mutex poisoned").insert(actual_port as u32, sender);
        Ok((actual_port, receiver))
    }

    /// Tears down a remote forward - stops routing incoming channels for
    /// this port (further ones are rejected, see
    /// `server_channel_open_forwarded_tcpip`) and asks the Node to stop
    /// listening. Best-effort on the Node side deliberately: the registry
    /// removal already stops anything new from being handed to a forward
    /// that's going away, so a `cancel_tcpip_forward` failure (session
    /// already gone, Node unreachable) isn't worth surfacing as an error to
    /// a caller that's already in the middle of stopping this forward.
    pub(super) async fn unregister_remote_forward(&self, bind_address: &str, bound_port: u16) {
        self.forward_registry.lock().expect("forward registry mutex poisoned").remove(&(bound_port as u32));
        let _ = self.handle.cancel_tcpip_forward(bind_address, bound_port as u32).await;
    }
}

/// A handle to a running interactive shell, opened by `SshSession::open_terminal`.
/// Dropping it ends the underlying background task and closes the remote
/// channel - there's no separate `close()` to remember to call.
/// Keeps a `follow_command` stream alive. Dropping it closes the remote
/// channel, which is the only way the follow ever ends - there is no
/// "finished" for `docker logs -f`.
pub struct FollowHandle {
    _stop_tx: mpsc::UnboundedSender<()>,
}

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
    forward_registry: ForwardRegistry,
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

    /// The other half of `SshSession::register_remote_forward` - the Node
    /// just accepted a connection on a port we asked it to forward, and is
    /// handing us the channel to carry it over. Routed by port to whichever
    /// `ssh::port_forward::start_remote_forward` call registered it; a port
    /// with nothing registered (already stopped, or somehow not ours) is
    /// rejected rather than silently accepted and then dropped, which would
    /// leave the connecting peer hanging until it times out instead of
    /// seeing an immediate refusal.
    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: Channel<client::Msg>,
        _connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let sender = self.forward_registry.lock().expect("forward registry mutex poisoned").get(&connected_port).cloned();
        if let Some(sender) = sender {
            if sender.send(channel).is_ok() {
                reply.accept().await;
                return Ok(());
            }
        }
        reply.reject(russh::ChannelOpenFailure::ConnectFailed).await;
        Ok(())
    }
}

fn classify_connect_error(err: &russh::Error, seen: &Arc<Mutex<SeenHostKey>>, host: &str) -> AppError {
    if seen.lock().expect("host key mutex poisoned").mismatched {
        // Its own code rather than a generic connection error: this is the
        // one failure here where the right UI is a warning the user has to
        // read and decide about, not a retry button. The full explanation
        // ("reinstalled, or someone is intercepting") now lives in the
        // frontend's own translated copy, where it can be phrased properly
        // in the user's language instead of assembled in Rust.
        AppError::HostKeyMismatch { host: host.to_string() }
    } else {
        AppError::Connection(format!("SSH connection failed: {err}"))
    }
}
