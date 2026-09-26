//! Low-level SSH connection: connect, verify the host key (TOFU), authenticate,
//! run one command. No knowledge of `Server`/keyring/SQLite here on purpose -
//! `ssh_service` resolves those into a `SshCredentials` + the previously known
//! fingerprint before calling `connect`, keeping this module pure protocol
//! mechanics and independently testable against a bare SSH server.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use russh::keys::{load_secret_key, Algorithm, EcdsaCurve, HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{client, Channel, ChannelMsg, Disconnect, Preferred};
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
/// Hands out `SshSession::id`. Monotonic and never reused, so a value that
/// identified a dropped session can never come to mean a live one.
static NEXT_SESSION_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub struct SshSession {
    /// Identifies *this* connection, for callers that want to remember
    /// something they only had to establish once per connection.
    ///
    /// A server id is not enough for that: a session can be dropped and
    /// replaced (a transport error, an idle subsystem, a Node rebooting)
    /// while the server id stays the same, and anything remembered against
    /// the id would then be remembered about a connection that no longer
    /// exists. Keyed on this instead, a reconnect re-establishes whatever
    /// it needs to, because the new session is a different session.
    id: u64,
    handle: client::Handle<TofuHandler>,
    sftp: OnceCell<SftpSession>,
    /// CPU% and network rates are deltas between two samples, not values a
    /// single `/proc` read gives you directly - see `ssh/monitor.rs`. `None`
    /// on the very first call, same as `agent::metrics::MetricsCollector`
    /// reports 0 rather than a meaningless number for a sample it has no
    /// prior point to compare against.
    metrics_sample: Mutex<Option<MetricsSample>>,
    forward_registry: ForwardRegistry,
    /// Short-lived commands allowed on this connection at once - see
    /// `MAX_CONCURRENT_COMMANDS`.
    command_slots: tokio::sync::Semaphore,
}

/// How many `execute_command`s may hold a channel on one connection at once.
///
/// `sshd` allows ten sessions per connection by default (`MaxSessions`), and
/// past that every new channel is refused with `ConnectFailed` - which is how
/// a file editor ended up unable to open a config while the page around it
/// polled. Long-lived channels share that budget: the SFTP subsystem, a
/// console's log follow, the resource stream, any open terminal. Capping the
/// short ones below the limit leaves them room, and a command beyond the cap
/// waits a moment for a slot instead of failing.
const MAX_CONCURRENT_COMMANDS: usize = 6;

/// Pauses before retrying a refused channel open. Short commands finish in a
/// fraction of a second, so a refusal caused by a momentary crowd clears
/// quickly; the steps stop at a couple of seconds so a Node that is refusing
/// for a lasting reason still answers with its error rather than hanging.
const CHANNEL_OPEN_RETRY_DELAYS_MS: [u64; 5] = [100, 250, 500, 1000, 2000];

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
    /// The kind of host key that fingerprint belongs to - worth remembering,
    /// so the next connection asks for that kind first (see `KnownHostKey`).
    pub host_key_family: Option<HostKeyFamily>,
}

/// A kind of SSH host key. A server holds at most one of each, and which one
/// a connection is shown depends on what both sides prefer - so the kind is
/// part of what "the key we saw before" means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKeyFamily {
    Ed25519,
    Ecdsa,
    Rsa,
}

const ED25519_ALGORITHMS: &[Algorithm] = &[Algorithm::Ed25519];
const ECDSA_ALGORITHMS: &[Algorithm] = &[
    Algorithm::Ecdsa { curve: EcdsaCurve::NistP256 },
    Algorithm::Ecdsa { curve: EcdsaCurve::NistP384 },
    Algorithm::Ecdsa { curve: EcdsaCurve::NistP521 },
];
const RSA_ALGORITHMS: &[Algorithm] = &[Algorithm::Rsa { hash: Some(HashAlg::Sha512) }, Algorithm::Rsa { hash: Some(HashAlg::Sha256) }];

impl HostKeyFamily {
    const ALL: [HostKeyFamily; 3] = [HostKeyFamily::Ed25519, HostKeyFamily::Ecdsa, HostKeyFamily::Rsa];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ed25519 => "ed25519",
            Self::Ecdsa => "ecdsa",
            Self::Rsa => "rsa",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|family| family.as_str() == value)
    }

    fn of(algorithm: &Algorithm) -> Option<Self> {
        match algorithm {
            Algorithm::Ed25519 => Some(Self::Ed25519),
            Algorithm::Ecdsa { .. } => Some(Self::Ecdsa),
            Algorithm::Rsa { .. } => Some(Self::Rsa),
            _ => None,
        }
    }

    fn algorithms(self) -> &'static [Algorithm] {
        match self {
            Self::Ed25519 => ED25519_ALGORITHMS,
            Self::Ecdsa => ECDSA_ALGORITHMS,
            Self::Rsa => RSA_ALGORITHMS,
        }
    }
}

/// The host key recorded for a server: its fingerprint, and the kind it is
/// when that is known (keys recorded before the kind was kept have none).
pub struct KnownHostKey {
    pub fingerprint: String,
    pub family: Option<HostKeyFamily>,
}

/// The host-key preference for a connection: russh's own order, with the
/// recorded kind moved to the front when there is one, so a server holding
/// several keys shows the one we know.
fn key_preference(first: Option<HostKeyFamily>) -> Cow<'static, [Algorithm]> {
    let default = Preferred::DEFAULT.key;
    match first {
        None => default,
        // Reordered, never trimmed: everything russh offers is still offered.
        Some(family) => {
            let (mut order, rest): (Vec<Algorithm>, Vec<Algorithm>) = default.iter().cloned().partition(|algorithm| HostKeyFamily::of(algorithm) == Some(family));
            order.extend(rest);
            Cow::Owned(order)
        }
    }
}

type Handshake = (client::Handle<TofuHandler>, Arc<Mutex<SeenHostKey>>, ForwardRegistry);

/// One SSH handshake - key exchange and host-key check, no login - offering
/// the host-key algorithms in `keys`, in that order. A failure carries the
/// kind of key the server showed, when it got that far.
async fn handshake(
    credentials: &SshCredentials,
    expected: Option<&str>,
    keys: Cow<'static, [Algorithm]>,
) -> Result<Handshake, (AppError, Option<HostKeyFamily>)> {
    let seen = Arc::new(Mutex::new(SeenHostKey::default()));
    let forward_registry: ForwardRegistry = Arc::new(Mutex::new(HashMap::new()));
    let handler = TofuHandler {
        expected_fingerprint: expected.map(str::to_string),
        seen: seen.clone(),
        forward_registry: forward_registry.clone(),
    };
    let config = Arc::new(client::Config {
        keepalive_interval: Some(KEEPALIVE_INTERVAL),
        inactivity_timeout: None,
        preferred: Preferred { key: keys, ..Preferred::DEFAULT },
        ..Default::default()
    });
    let addr = (credentials.host.as_str(), credentials.port);
    let shown = |seen: &Arc<Mutex<SeenHostKey>>| seen.lock().expect("host key mutex poisoned").family;
    let handle = tokio::time::timeout(CONNECT_TIMEOUT, client::connect(config, addr, handler))
        .await
        .map_err(|_| (AppError::Timeout { operation: "connecting", seconds: CONNECT_TIMEOUT.as_secs() }, None))?
        .map_err(|err| (classify_connect_error(&err, &seen, &credentials.host, expected), shown(&seen)))?;
    Ok((handle, seen, forward_registry))
}

/// Handshakes, and on a host-key mismatch asks whether the server simply
/// holds another kind of key as well - OpenSSH's `UpdateHostKeys` situation.
///
/// A server that gains an ECDSA key beside the RSA key it always had now
/// shows ECDSA first, and the recorded RSA fingerprint no longer matches the
/// key on offer. That is not a changed key, and treating it as the
/// interception warning trains people to click through that warning. So on a
/// mismatch each other kind is asked for in turn; if one of them is the
/// recorded key, the connection goes ahead with it. Safe for the same reason
/// the first check is: the handshake only completes if the server signs the
/// exchange with that key's private half, so matching the recorded
/// fingerprint here means holding the recorded key. When no kind matches,
/// the original warning stands.
async fn handshake_with_known_key(credentials: &SshCredentials, known: Option<&KnownHostKey>) -> AppResult<(Handshake, Option<HostKeyFamily>)> {
    let expected = known.map(|known| known.fingerprint.as_str());
    let first_choice = known.and_then(|known| known.family);
    let (mismatch, shown_family) = match handshake(credentials, expected, key_preference(first_choice)).await {
        Ok(result) => {
            let family = result.1.lock().expect("host key mutex poisoned").family;
            return Ok((result, family));
        }
        Err((err @ AppError::HostKeyMismatch { .. }, family)) => (err, family),
        Err((err, _)) => return Err(err),
    };
    let AppError::HostKeyMismatch { presented, .. } = &mismatch else {
        return Err(mismatch);
    };
    // The kind just shown is the one that did not match; asking for it again
    // would only show the same key.
    for family in HostKeyFamily::ALL.into_iter().filter(|family| Some(*family) != shown_family) {
        match handshake(credentials, expected, Cow::Borrowed(family.algorithms())).await {
            Ok(result) => {
                log::info!(
                    "{} also presents a newer host key ({}); connected with the recorded {} key instead",
                    credentials.host,
                    presented.as_deref().unwrap_or("unknown"),
                    family.as_str()
                );
                return Ok((result, Some(family)));
            }
            // This kind is not the recorded key either, or the server does
            // not have one - try the next.
            Err(_) => continue,
        }
    }
    Err(mismatch)
}

pub async fn connect(credentials: &SshCredentials, known_fingerprint: Option<String>) -> AppResult<ConnectOutcome> {
    connect_known(credentials, known_fingerprint.map(|fingerprint| KnownHostKey { fingerprint, family: None })).await
}

pub async fn connect_known(credentials: &SshCredentials, known: Option<KnownHostKey>) -> AppResult<ConnectOutcome> {
    let ((mut handle, seen, forward_registry), host_key_family) = handshake_with_known_key(credentials, known.as_ref()).await?;

    // The handshake above has a deadline; the login used to have none. A Node
    // that accepts the connection and then stalls on authentication - sshd
    // doing a reverse DNS lookup while its network is still coming up after a
    // reboot, say - left the terminal on "connecting..." for as long as the
    // Node cared to wait. Reported as the same "connecting" timeout, because
    // to the person watching it is one step.
    let authenticate = async {
        let result = match &credentials.auth {
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
        Ok::<_, AppError>(result)
    };
    let auth_result = tokio::time::timeout(CONNECT_TIMEOUT, authenticate)
        .await
        .map_err(|_| AppError::Timeout { operation: "connecting", seconds: CONNECT_TIMEOUT.as_secs() })??;

    if !auth_result.success() {
        let method = match credentials.auth {
            SshAuth::Password(_) => "password",
            SshAuth::PrivateKey { .. } => "key",
        };
        return Err(AppError::SshAuthRejected { username: credentials.username.clone(), method, server_id: None });
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
            id: NEXT_SESSION_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            metrics_sample: Mutex::new(None),
            forward_registry,
            command_slots: tokio::sync::Semaphore::new(MAX_CONCURRENT_COMMANDS),
        },
        host_key_fingerprint,
        host_key_family,
    })
}

impl SshSession {
    /// See the field's own note - unique for the life of the process.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Opens a session channel, riding out a momentary refusal.
    ///
    /// `ConnectFailed` or `ResourceShortage` on a channel open is almost
    /// always `sshd`'s per-connection session limit, reached for the moment
    /// by commands about to finish. It is retried after a short pause, a few
    /// times, before being reported - it used to surface straight away as
    /// "couldn't open an SSH channel" over a config somebody was opening.
    async fn open_session_channel(&self) -> Result<Channel<client::Msg>, russh::Error> {
        let mut delays = CHANNEL_OPEN_RETRY_DELAYS_MS.iter();
        loop {
            let result = self.handle.channel_open_session().await;
            let refused_for_now = matches!(
                &result,
                Err(russh::Error::ChannelOpenFailure(russh::ChannelOpenFailure::ConnectFailed | russh::ChannelOpenFailure::ResourceShortage))
            );
            match (refused_for_now, delays.next()) {
                (true, Some(delay_ms)) => tokio::time::sleep(Duration::from_millis(*delay_ms)).await,
                _ => return result,
            }
        }
    }

    /// Bounded by `COMMAND_TIMEOUT` - see that constant for why there is a
    /// bound at all, and why it is as generous as it is.
    pub async fn execute_command(&self, command: &str) -> AppResult<CommandOutput> {
        tokio::time::timeout(COMMAND_TIMEOUT, self.execute_command_inner(command, None))
            .await
            .unwrap_or_else(|_| Err(AppError::Timeout { operation: "the command", seconds: COMMAND_TIMEOUT.as_secs() }))
    }

    /// `execute_command` with a bound of the caller's choosing, for the few
    /// operations whose length is the size of somebody's data - copying a
    /// game server's whole world - and for which ten minutes is not a
    /// generous ceiling but a failure waiting for a big enough world.
    pub async fn execute_command_with_timeout(&self, command: &str, timeout: Duration) -> AppResult<CommandOutput> {
        tokio::time::timeout(timeout, self.execute_command_inner(command, None))
            .await
            .unwrap_or(Err(AppError::Timeout { operation: "the command", seconds: timeout.as_secs() }))
    }

    /// `execute_command`, with `input` sent to the command's stdin and then
    /// closed.
    ///
    /// For content that must not go through the command line: a command
    /// string is visible in `ps` to every local account for as long as it
    /// runs (AGENTS.md, rule 2), and a config file being saved is exactly
    /// the kind of thing that holds a database password.
    pub async fn execute_command_with_input(&self, command: &str, input: &[u8]) -> AppResult<CommandOutput> {
        tokio::time::timeout(COMMAND_TIMEOUT, self.execute_command_inner(command, Some(input)))
            .await
            .unwrap_or_else(|_| Err(AppError::Timeout { operation: "the command", seconds: COMMAND_TIMEOUT.as_secs() }))
    }

    async fn execute_command_inner(&self, command: &str, input: Option<&[u8]>) -> AppResult<CommandOutput> {
        // Held until the channel is done - see `MAX_CONCURRENT_COMMANDS`.
        let _slot = self
            .command_slots
            .acquire()
            .await
            .map_err(|_| AppError::Connection("the SSH connection is closing".into()))?;
        let mut channel = self
            .open_session_channel()
            .await
            .map_err(|err| AppError::Connection(format!("couldn't open an SSH channel: {err}")))?;
        channel
            .exec(true, command)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't run the command: {err}")))?;
        if let Some(input) = input {
            channel
                .data(input)
                .await
                .map_err(|err| AppError::Connection(format!("couldn't send the command its input: {err}")))?;
            channel
                .eof()
                .await
                .map_err(|err| AppError::Connection(format!("couldn't finish sending the command its input: {err}")))?;
        }

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

    /// Runs `command` here and `target_command` on `target`, with this
    /// command's stdout fed into that one's stdin as it arrives - a pipe
    /// between two Nodes that runs through this process, since the Nodes
    /// cannot be assumed to reach each other.
    ///
    /// Built for moving a whole directory as one `tar` stream: one channel
    /// each way instead of several SSH round trips per file. `on_chunk` sees
    /// every chunk on its way through, which is how the caller reports
    /// progress. Backpressure is the SSH window: a slow target stops this
    /// side reading, so nothing piles up in memory.
    ///
    /// Not under `COMMAND_TIMEOUT`, because a large directory legitimately
    /// takes longer than any fixed bound. Succeeds only if both commands
    /// exit 0; otherwise the error carries whatever each wrote to stderr.
    pub async fn pipe_into(&self, command: &str, target: &SshSession, target_command: &str, mut on_chunk: impl FnMut(&[u8]) + Send) -> AppResult<()> {
        let closing = |_| AppError::Connection("the SSH connection is closing".into());
        let _slot = self.command_slots.acquire().await.map_err(closing)?;
        let _target_slot = target.command_slots.acquire().await.map_err(closing)?;

        let target_channel = target
            .open_session_channel()
            .await
            .map_err(|err| AppError::Connection(format!("couldn't open an SSH channel on the target: {err}")))?;
        target_channel
            .exec(true, target_command)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't start the receiving command: {err}")))?;
        // Read concurrently with the writes below: the target's messages
        // (its stderr, its exit status) queue on a bounded channel, and one
        // left unread while this side blocks on the window would stall both.
        let (mut target_read, target_write) = target_channel.split();
        let target_done = tokio::spawn(async move {
            let mut output = Vec::new();
            let mut truncated = false;
            let mut exit_code = None;
            while let Some(msg) = target_read.wait().await {
                match msg {
                    ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. } => append_capped(&mut output, &data, &mut truncated),
                    ChannelMsg::ExitStatus { exit_status } => exit_code = Some(exit_status),
                    _ => {}
                }
            }
            (exit_code, String::from_utf8_lossy(&output).trim().to_string())
        });

        let mut source_channel = self
            .open_session_channel()
            .await
            .map_err(|err| AppError::Connection(format!("couldn't open an SSH channel on the source: {err}")))?;
        source_channel
            .exec(true, command)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't start the sending command: {err}")))?;

        let mut source_stderr = Vec::new();
        let mut truncated = false;
        let mut source_exit = None;
        let mut write_error = None;
        while let Some(msg) = source_channel.wait().await {
            match msg {
                ChannelMsg::Data { data } => {
                    on_chunk(&data);
                    if let Err(err) = target_write.data(&data[..]).await {
                        write_error = Some(err);
                        break;
                    }
                }
                ChannelMsg::ExtendedData { data, ext } if ext == SSH_EXTENDED_DATA_STDERR => append_capped(&mut source_stderr, &data, &mut truncated),
                ChannelMsg::ExitStatus { exit_status } => source_exit = Some(exit_status),
                _ => {}
            }
        }

        if let Some(err) = write_error {
            // The target stopped taking data - it failed, and its own stderr
            // says why far better than the broken write does.
            if let Err(close_err) = source_channel.close().await {
                log::warn!("couldn't close the sending side of a pipe after the target failed: {close_err}");
            }
            let why = match tokio::time::timeout(Duration::from_secs(5), target_done).await {
                Ok(Ok((_, stderr))) if !stderr.is_empty() => stderr,
                _ => err.to_string(),
            };
            return Err(AppError::Connection(format!("the receiving side stopped: {why}")));
        }
        if let Err(err) = target_write.eof().await {
            log::warn!("couldn't signal the end of the stream to the receiving side: {err}");
        }
        let (target_exit, target_stderr) = target_done
            .await
            .map_err(|err| AppError::Internal(format!("the task reading the receiving side failed: {err}")))?;

        let source_stderr = String::from_utf8_lossy(&source_stderr).trim().to_string();
        match (source_exit, target_exit) {
            (Some(0), Some(0)) => Ok(()),
            (source_exit, Some(0)) => Err(AppError::Connection(format!(
                "the sending command failed (exit {}): {source_stderr}",
                source_exit.map_or("unknown".to_string(), |code| code.to_string())
            ))),
            (_, target_exit) => Err(AppError::Connection(format!(
                "the receiving command failed (exit {}): {target_stderr}",
                target_exit.map_or("unknown".to_string(), |code| code.to_string())
            ))),
        }
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
            .open_session_channel()
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
            .open_session_channel()
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
            .open_session_channel()
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
    family: Option<HostKeyFamily>,
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
        seen.family = HostKeyFamily::of(&server_public_key.public_key().algorithm());

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

fn classify_connect_error(err: &russh::Error, seen: &Arc<Mutex<SeenHostKey>>, host: &str, expected: Option<&str>) -> AppError {
    let seen = seen.lock().expect("host key mutex poisoned");
    if seen.mismatched {
        // Its own code rather than a generic connection error: this is the
        // one failure here where the right UI is a warning the user has to
        // read and decide about, not a retry button. The full explanation
        // ("reinstalled, or someone is intercepting") now lives in the
        // frontend's own translated copy, where it can be phrased properly
        // in the user's language instead of assembled in Rust.
        AppError::HostKeyMismatch { host: host.to_string(), server_id: None, expected: expected.map(str::to_string), presented: seen.fingerprint.clone() }
    } else {
        AppError::Connection(format!("SSH connection failed: {err}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_key_family_round_trips_through_its_name() {
        for family in HostKeyFamily::ALL {
            assert_eq!(HostKeyFamily::parse(family.as_str()), Some(family));
        }
        assert_eq!(HostKeyFamily::parse("dsa"), None);
    }

    #[test]
    fn the_recorded_kind_is_asked_for_first_and_nothing_is_dropped() {
        let order = key_preference(Some(HostKeyFamily::Rsa));
        assert_eq!(HostKeyFamily::of(&order[0]), Some(HostKeyFamily::Rsa));
        // Every algorithm russh would offer is still offered, once.
        assert_eq!(order.len(), Preferred::DEFAULT.key.len());
        for algorithm in Preferred::DEFAULT.key.iter() {
            assert_eq!(order.iter().filter(|offered| *offered == algorithm).count(), 1, "{algorithm:?}");
        }
        // Without a recorded kind the order is russh's own.
        assert_eq!(key_preference(None).as_ref(), Preferred::DEFAULT.key.as_ref());
    }

    #[test]
    fn every_kind_asks_only_for_its_own_algorithms() {
        for family in HostKeyFamily::ALL {
            assert!(family.algorithms().iter().all(|algorithm| HostKeyFamily::of(algorithm) == Some(family)));
        }
    }
}
