//! Resolves a `Server`/`ServerInput` plus its keyring secret into the
//! `SshCredentials` the `ssh` module needs, and owns the two real entry
//! points: testing a connection before a server is saved, and running a
//! command against one that already is (via the cached `SshSessionManager`).

use std::path::Path;
use std::sync::Arc;

use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::files::{self, sftp::SftpApplicationFileProvider, ApplicationFileProvider};
use crate::models::{AuthenticationType, PortForwardKind, PortForwardStatus, Server, ServerInput, StartPortForwardInput};
use crate::ssh::{self, PortForwardHandle, SshAuth, SshCredentials, SshSession, TerminalHandle};
use crate::state::SshSessionManager;
use crate::storage::credentials::{self, SecretKind};
use crate::storage::server_repository::ServerRepository;
use crate::transport::{CommandOutput, ContainerSummary, ProcessSummary, RemoteFileEntry, ServerMetrics, ServiceSummary};

/// Connects with whatever's in `input` directly - no server id, no keyring,
/// no persisted host key, since nothing has been saved yet to persist
/// against. Used by the "Test connection" button before Save.
pub async fn test_connection(input: &ServerInput) -> AppResult<()> {
    let credentials = credentials_from_input(input)?;
    let outcome = ssh::connect(&credentials, None).await?;
    outcome.session.close().await;
    Ok(())
}

/// Runs a command against a saved server, reusing a cached connection when
/// there is one. A cached connection that turns out to be dead (reboot,
/// idle timeout, network blip) is dropped and retried once with a fresh one
/// before this gives up and returns the original error.
pub async fn execute_command(
    repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    command: &str,
) -> AppResult<CommandOutput> {
    retry_on_connection_failure(sessions, Some(server_id), || async {
        get_or_connect(repo, sessions, server_id).await?.execute_command(command).await
    })
    .await
}

/// Runs `attempt` once, retrying it exactly once - with the cached session
/// for `server_id` dropped first - if it fails. The general form of
/// `execute_command`'s own dead-cached-session recovery above (idle
/// timeout, network blip, Node reboot), for callers that need more than one
/// command against the connection, or hand it to something else
/// (`application_service`'s `ApplicationRuntime` calls, `database_service`'s
/// `run_mysql`) rather than running one command here directly. `attempt` is
/// expected to re-resolve its own connection on each call (a cheap local
/// read either way, and simpler than threading a `RuntimeContext`-shaped
/// borrow through a generic retry wrapper) rather than this handing back an
/// already-built connection. `server_id: None` (a Local application, or
/// nothing to reconnect) just runs `attempt` once - there's no cached
/// session to go stale.
pub(crate) async fn retry_on_connection_failure<T, F, Fut>(sessions: &SshSessionManager, server_id: Option<Uuid>, mut attempt: F) -> AppResult<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = AppResult<T>>,
{
    match attempt().await {
        Ok(value) => Ok(value),
        Err(first_err) => {
            // Only a transport failure is worth reconnecting for. This used
            // to retry on *any* error, so a plain
            // `InvalidInput("port 25565 is already published")` tore down a
            // perfectly healthy SSH session, opened a new one, and re-ran
            // the whole operation - side effects included. A rejected input
            // did the work twice; a failed `docker pull` pulled twice. The
            // name said "connection failure" and the code said "anything".
            if !is_transport_failure(&first_err) {
                return Err(first_err);
            }
            let Some(server_id) = server_id else {
                return Err(first_err);
            };
            sessions.remove(server_id).await;
            // Surface the *second* error, not the first. A reconnect that
            // fails for a new reason ("SSH authentication was rejected") is
            // far more actionable than repeating the stale transport error
            // that triggered the retry - which is what `map_err(|_| first_err)`
            // used to do.
            attempt().await
        }
    }
}

/// `true` for the errors a fresh connection could plausibly fix.
///
/// `Connection` is the transport itself. `Internal` is included because
/// several call sites report a missing or unusable session that way rather
/// than as `Connection`, and reconnecting is the right response to it.
/// Everything else - `InvalidInput`, `NotFound`, `Storage`, `Unauthorized` -
/// describes a request that fails identically on a new connection.
fn is_transport_failure(error: &AppError) -> bool {
    matches!(error, AppError::Connection(_) | AppError::Internal(_))
}

/// Opens an interactive shell against a saved server, reusing a cached
/// connection when there is one. Doesn't retry a dead cached connection
/// itself - `on_output`/`on_closed` are one-shot (`on_closed` is literally
/// `FnOnce`), so a retry that calls this twice needs a fresh pair each time,
/// which only the caller can cheaply reconstruct (it's usually just an
/// `AppHandle` clone and an event-name String). See
/// `commands::terminal_commands::open_terminal`, which is what actually
/// does the "dead session, drop and retry once" recovery here - the same
/// policy `execute_command`/every other SSH-touching feature already
/// applies, just at the layer above instead of inside this function.
pub async fn open_terminal(
    repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    cols: u32,
    rows: u32,
    on_output: impl FnMut(String) + Send + 'static,
    on_closed: impl FnOnce(Option<String>) + Send + 'static,
) -> AppResult<TerminalHandle> {
    let session = get_or_connect(repo, sessions, server_id).await?;
    session.open_terminal(cols, rows, on_output, on_closed).await
}

pub async fn list_directory(
    repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    path: &str,
) -> AppResult<Vec<RemoteFileEntry>> {
    let session = get_or_connect(repo, sessions, server_id).await?;
    session.list_directory(path).await
}

pub async fn read_file(
    repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    path: &str,
) -> AppResult<Vec<u8>> {
    let session = get_or_connect(repo, sessions, server_id).await?;
    session.read_file(path).await
}

/// One window of a file on a Node, for looking at one too large to load
/// whole. The same shape - and the same caveat - as the Application-files
/// version: this is a slice, and writing a partial buffer back would truncate
/// the file.
pub async fn read_file_window(
    repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    path: &str,
    offset: u64,
    len: usize,
) -> AppResult<crate::services::FileWindow> {
    let session = get_or_connect(repo, sessions, server_id).await?;
    let total_size = session.symlink_metadata(path).await?.size;
    let bytes = session.read_file_range(path, offset, len).await?;
    Ok(crate::services::FileWindow { next_offset: offset + bytes.len() as u64, bytes, total_size })
}

pub async fn write_file(
    repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    path: &str,
    contents: &[u8],
) -> AppResult<()> {
    let session = get_or_connect(repo, sessions, server_id).await?;
    session.write_file(path, contents).await
}

pub async fn create_directory(
    repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    path: &str,
) -> AppResult<()> {
    let session = get_or_connect(repo, sessions, server_id).await?;
    session.create_directory(path).await
}

pub async fn download_file(
    repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    remote_path: &str,
    local_path: &Path,
) -> AppResult<()> {
    let session = get_or_connect(repo, sessions, server_id).await?;
    session.download_file(remote_path, local_path).await
}

pub async fn upload_file(
    repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    local_path: &Path,
    remote_path: &str,
) -> AppResult<()> {
    let session = get_or_connect(repo, sessions, server_id).await?;
    session.upload_file(local_path, remote_path).await
}

/// Uploads a whole local directory into `remote_path`, keeping its shape.
///
/// Shares the local half of the work with the Application-files upload -
/// `crate::files::plan_directory_upload` decides what to send, and only the
/// remote calls differ between the two. See that function for why symlinks
/// are skipped rather than followed.
///
/// An existing directory is not an error: dropping a folder onto one that is
/// already there merges into it, the way copying a directory does everywhere
/// else.
pub async fn upload_directory(
    repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    local_path: &Path,
    remote_path: &str,
) -> AppResult<()> {
    let name = local_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| AppError::InvalidInput(format!("{} has no name to copy", local_path.display())))?;
    let remote_root = crate::files::join_remote(remote_path, &name);

    let (directories, files) = crate::files::plan_directory_upload(local_path, &remote_root).await?;
    let session = get_or_connect(repo, sessions, server_id).await?;

    // Parents before children - the walk is breadth-first, so its order is
    // already right.
    for directory in &directories {
        // `symlink_metadata` rather than a stat that follows: this only needs
        // to know whether something is already there under that name.
        if session.create_directory(directory).await.is_err() && session.symlink_metadata(directory).await.is_err() {
            return Err(AppError::Connection(format!("couldn't create {directory}")));
        }
    }

    for file in &files {
        session.upload_file(&file.local, &file.remote).await?;
    }
    Ok(())
}

/// An `ApplicationFileProvider` rooted at "/" - not jailed to anything,
/// deliberately: the plain server-wide Files browser already has full SSH
/// access to this host (the same trust boundary Terminal and every other
/// Node Files command already operate inside), so there's no sandbox to
/// enforce here the way there is for one Application's own working
/// directory. `is_within_root`'s own root-trimming makes any absolute path
/// satisfy a "/" root trivially, so this reuses `SftpApplicationFileProvider`
/// (recursive delete/copy, rename, chmod, archive extraction - all already
/// written and tested for the Application Files case) instead of
/// duplicating that logic for a "no jail" variant.
async fn plain_file_provider(repo: &ServerRepository, sessions: &SshSessionManager, server_id: Uuid) -> AppResult<SftpApplicationFileProvider> {
    let session = connect_with_live_sftp(repo, sessions, server_id).await?;
    Ok(SftpApplicationFileProvider::new(session, "/".to_string()))
}

/// See `application_files_service::connect_with_live_sftp`'s own doc
/// comment for the full reasoning - `SshSession` caches its SFTP subsystem
/// channel for the session's whole lifetime, so a cached session that's
/// still fine for plain command exec can have a long-dead SFTP channel that
/// nothing ever resets on its own.
async fn connect_with_live_sftp(repo: &ServerRepository, sessions: &SshSessionManager, server_id: Uuid) -> AppResult<Arc<SshSession>> {
    let connection = get_or_connect(repo, sessions, server_id).await?;
    if connection.canonicalize_path(".").await.is_ok() {
        return Ok(connection);
    }
    sessions.remove(server_id).await;
    get_or_connect(repo, sessions, server_id).await
}

pub async fn rename_path(repo: &ServerRepository, sessions: &SshSessionManager, server_id: Uuid, from: &str, to: &str) -> AppResult<()> {
    plain_file_provider(repo, sessions, server_id).await?.rename(from, to).await
}

/// Recursive for a directory - see `ApplicationFileProvider::delete`'s own
/// doc comment; the same primitive as the Application Files "Delete"
/// action, just against the whole filesystem instead of one working
/// directory.
pub async fn delete_path(repo: &ServerRepository, sessions: &SshSessionManager, server_id: Uuid, path: &str) -> AppResult<()> {
    plain_file_provider(repo, sessions, server_id).await?.delete(path).await
}

pub async fn set_permissions(repo: &ServerRepository, sessions: &SshSessionManager, server_id: Uuid, path: &str, mode: u32) -> AppResult<()> {
    if mode > 0o7777 {
        return Err(AppError::InvalidInput("not a valid POSIX permission value".into()));
    }
    plain_file_provider(repo, sessions, server_id).await?.set_permissions(path, mode).await
}

/// Extracts an already-uploaded `.zip` at `archive_path` into `destination`
/// - reuses the exact Zip-Slip-guarded `files::archive::extract_zip` the
/// Application Files "Extract" action already uses, just against the
/// unjailed provider above.
pub async fn extract_archive(
    repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    archive_path: &str,
    destination: &str,
) -> AppResult<u32> {
    let provider = plain_file_provider(repo, sessions, server_id).await?;
    let bytes = provider.read_file(archive_path).await?;
    files::archive::extract_zip(&provider, &bytes, destination).await
}

/// Compresses `paths` into a new `.zip` written to `destination_path` - see
/// `files::archive::create_zip`'s own doc comment for the entry-naming rule.
pub async fn compress_paths(
    repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    paths: &[String],
    destination_path: &str,
) -> AppResult<()> {
    let provider = plain_file_provider(repo, sessions, server_id).await?;
    files::archive::create_zip(&provider, paths, destination_path).await
}

/// A fresh sample each call - the CPU%/network-rate delta math lives on
/// `SshSession` itself (see `ssh/monitor.rs`), keyed off the cached
/// connection so repeated polling compares against the *previous* poll
/// rather than resetting to a meaningless first-sample 0 every time.
pub async fn get_metrics(repo: &ServerRepository, sessions: &SshSessionManager, server_id: Uuid) -> AppResult<ServerMetrics> {
    let session = get_or_connect(repo, sessions, server_id).await?;
    session.get_metrics().await
}

pub async fn list_processes(
    repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
) -> AppResult<Vec<ProcessSummary>> {
    let session = get_or_connect(repo, sessions, server_id).await?;
    session.list_processes().await
}

pub async fn list_services(repo: &ServerRepository, sessions: &SshSessionManager, server_id: Uuid) -> AppResult<Vec<ServiceSummary>> {
    let session = get_or_connect(repo, sessions, server_id).await?;
    session.list_services().await
}

pub async fn restart_service(
    repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    service_name: &str,
) -> AppResult<()> {
    let session = get_or_connect(repo, sessions, server_id).await?;
    session.restart_service(service_name).await
}

pub async fn start_service(
    repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    service_name: &str,
) -> AppResult<()> {
    let session = get_or_connect(repo, sessions, server_id).await?;
    session.start_service(service_name).await
}

pub async fn stop_service(
    repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    service_name: &str,
) -> AppResult<()> {
    let session = get_or_connect(repo, sessions, server_id).await?;
    session.stop_service(service_name).await
}

pub async fn enable_service(
    repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    service_name: &str,
) -> AppResult<()> {
    let session = get_or_connect(repo, sessions, server_id).await?;
    session.enable_service(service_name).await
}

pub async fn disable_service(
    repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    service_name: &str,
) -> AppResult<()> {
    let session = get_or_connect(repo, sessions, server_id).await?;
    session.disable_service(service_name).await
}

pub async fn list_containers(
    repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
) -> AppResult<Vec<ContainerSummary>> {
    let session = get_or_connect(repo, sessions, server_id).await?;
    session.list_containers().await
}

pub async fn restart_container(
    repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    container: &str,
) -> AppResult<()> {
    let session = get_or_connect(repo, sessions, server_id).await?;
    session.restart_container(container).await
}

pub async fn start_container(
    repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    container: &str,
) -> AppResult<()> {
    let session = get_or_connect(repo, sessions, server_id).await?;
    session.start_container(container).await
}

pub async fn stop_container(
    repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    container: &str,
) -> AppResult<()> {
    let session = get_or_connect(repo, sessions, server_id).await?;
    session.stop_container(container).await
}

pub async fn remove_container(
    repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    container: &str,
) -> AppResult<()> {
    let session = get_or_connect(repo, sessions, server_id).await?;
    session.remove_container(container).await
}

pub async fn container_logs(
    repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Uuid,
    container: &str,
    tail: u32,
) -> AppResult<String> {
    let session = get_or_connect(repo, sessions, server_id).await?;
    session.container_logs(container, tail).await
}

/// Starts a Local/Remote/Dynamic SSH tunnel against a saved server, reusing
/// a cached connection when there is one. No dead-connection retry here -
/// unlike a single command or one terminal session, a forward is meant to
/// keep running unattended, so `commands::port_forward_commands` is what
/// actually inserts the result into `state::PortForwardManager` once this
/// returns; this only resolves the connection and starts the tunnel itself.
pub async fn start_port_forward(repo: &ServerRepository, sessions: &SshSessionManager, input: &StartPortForwardInput) -> AppResult<(PortForwardStatus, PortForwardHandle)> {
    let session = get_or_connect(repo, sessions, input.server_id).await?;

    let handle = match input.kind {
        PortForwardKind::Local => {
            SshSession::start_local_forward(session, &input.bind_address, input.bind_port, require_target_host(input)?, require_target_port(input)?).await?
        }
        PortForwardKind::Remote => {
            SshSession::start_remote_forward(session, &input.bind_address, input.bind_port, require_target_host(input)?, require_target_port(input)?).await?
        }
        PortForwardKind::Dynamic => SshSession::start_dynamic_forward(session, &input.bind_address, input.bind_port).await?,
    };

    let status = PortForwardStatus {
        id: Uuid::new_v4(),
        server_id: input.server_id,
        kind: input.kind,
        bind_address: input.bind_address.clone(),
        bind_port: handle.actual_port,
        target_host: input.target_host.clone(),
        target_port: input.target_port,
    };
    Ok((status, handle))
}

fn require_target_host(input: &StartPortForwardInput) -> AppResult<String> {
    input
        .target_host
        .as_deref()
        .map(str::trim)
        .filter(|host| !host.is_empty())
        .map(str::to_string)
        .ok_or_else(|| AppError::InvalidInput("a target host is required".into()))
}

fn require_target_port(input: &StartPortForwardInput) -> AppResult<u16> {
    input.target_port.filter(|&port| port != 0).ok_or_else(|| AppError::InvalidInput("a target port is required".into()))
}

/// `pub(super)` (not just private) so `application_service`'s
/// runtime-connection resolution can reuse this exact same
/// cache-then-connect-then-cache logic rather than duplicating it - see
/// that module's own use of it.
pub(super) async fn get_or_connect(repo: &ServerRepository, sessions: &SshSessionManager, server_id: Uuid) -> AppResult<Arc<SshSession>> {
    if let Some(session) = sessions.get(server_id).await {
        return Ok(session);
    }

    // Serialize connecting to *this* Node. Without this, two callers that
    // both miss the cache above - the metrics poller and a user action, say -
    // each open a full SSH connection, and the loser of the `insert` race
    // leaks: never closed, never removed. See `SshSessionManager::connect_locks`.
    let connect_lock = sessions.connect_lock(server_id).await;
    let _guard = connect_lock.lock().await;

    // Re-check now that the lock is held: whoever held it before us has very
    // likely just connected, and connecting again is exactly the duplicate
    // this lock exists to prevent.
    if let Some(session) = sessions.get(server_id).await {
        return Ok(session);
    }

    let server = repo.get(server_id)?.ok_or_else(|| AppError::NotFound(format!("server {server_id}")))?;
    let credentials = credentials_from_server(&server)?;
    let known_fingerprint = repo.get_known_host_fingerprint(server_id)?;

    let outcome = ssh::connect(&credentials, known_fingerprint.clone()).await?;
    if known_fingerprint.is_none() {
        repo.set_known_host_fingerprint(server_id, &outcome.host_key_fingerprint)?;
    }

    let session = Arc::new(outcome.session);
    sessions.insert(server_id, session.clone()).await;
    Ok(session)
}

fn credentials_from_input(input: &ServerInput) -> AppResult<SshCredentials> {
    let auth = match input.authentication_type {
        AuthenticationType::Password => SshAuth::Password(
            non_blank(&input.password)
                .ok_or_else(|| AppError::InvalidInput("a password is required for password authentication".into()))?
                .to_string(),
        ),
        AuthenticationType::PrivateKey => SshAuth::PrivateKey {
            path: non_blank(&input.private_key_path)
                .ok_or_else(|| AppError::InvalidInput("a private key path is required for key authentication".into()))?
                .to_string(),
            passphrase: non_blank(&input.key_passphrase).map(str::to_string),
        },
    };

    Ok(SshCredentials {
        host: input.host.clone(),
        port: input.ssh_port,
        username: input.username.clone(),
        auth,
    })
}

fn credentials_from_server(server: &Server) -> AppResult<SshCredentials> {
    let auth = match server.authentication_type {
        AuthenticationType::Password => {
            let password = credentials::load_secret(server.id, SecretKind::SshPassword)?
                .ok_or_else(|| AppError::Storage("no password is stored for this server".into()))?;
            SshAuth::Password(password)
        }
        AuthenticationType::PrivateKey => {
            let path = server
                .private_key_path
                .clone()
                .ok_or_else(|| AppError::Storage("this server has no private key path recorded".into()))?;
            // Optional: most keys have none, and a keyring that cannot be
            // reached must not block one that never needed a passphrase.
            let passphrase = credentials::load_optional_secret(server.id, SecretKind::SshKeyPassphrase)?;
            SshAuth::PrivateKey { path, passphrase }
        }
    };

    Ok(SshCredentials {
        host: server.host.clone(),
        port: server.ssh_port,
        username: server.username.clone(),
        auth,
    })
}

fn non_blank(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// The regression test for the retry finding. Retrying on *any* error
    /// meant a rejected input re-ran the whole operation, side effects and
    /// all, and tore down a healthy SSH session on the way.
    #[tokio::test]
    async fn a_rejected_input_is_not_retried() {
        let sessions = SshSessionManager::new();
        let attempts = AtomicUsize::new(0);

        let result: AppResult<()> = retry_on_connection_failure(&sessions, Some(Uuid::new_v4()), || async {
            attempts.fetch_add(1, Ordering::SeqCst);
            Err(AppError::InvalidInput("port 25565 is already published".into()))
        })
        .await;

        assert!(matches!(result, Err(AppError::InvalidInput(_))));
        assert_eq!(attempts.load(Ordering::SeqCst), 1, "an invalid input must not be attempted twice");
    }

    #[tokio::test]
    async fn a_not_found_or_storage_error_is_not_retried() {
        let sessions = SshSessionManager::new();
        for error in [AppError::NotFound("server".into()), AppError::Storage("database is locked".into())] {
            let attempts = AtomicUsize::new(0);
            let message = error.to_string();
            let _: AppResult<()> = retry_on_connection_failure(&sessions, Some(Uuid::new_v4()), || async {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err(AppError::Storage(message.clone()))
            })
            .await;
            assert_eq!(attempts.load(Ordering::SeqCst), 1, "{message} must not be retried");
        }
    }

    /// A transport failure is exactly what the retry exists for, and a
    /// second attempt that succeeds must return that success.
    #[tokio::test]
    async fn a_transport_failure_is_retried_once_and_can_succeed() {
        let sessions = SshSessionManager::new();
        let attempts = AtomicUsize::new(0);

        let result = retry_on_connection_failure(&sessions, Some(Uuid::new_v4()), || async {
            if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(AppError::Connection("connection reset".into()))
            } else {
                Ok(42)
            }
        })
        .await;

        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    /// When both attempts fail, the *second* error is what the caller sees.
    /// The previous `map_err(|_| first_err)` reported the stale transport
    /// error even when the reconnect had failed for a new, far more
    /// actionable reason.
    #[tokio::test]
    async fn the_second_error_wins_when_both_attempts_fail() {
        let sessions = SshSessionManager::new();
        let attempts = AtomicUsize::new(0);

        let result: AppResult<()> = retry_on_connection_failure(&sessions, Some(Uuid::new_v4()), || async {
            if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(AppError::Connection("connection reset".into()))
            } else {
                Err(AppError::InvalidInput("SSH authentication was rejected".into()))
            }
        })
        .await;

        let message = result.unwrap_err().to_string();
        assert!(message.contains("authentication was rejected"), "{message}");
    }

    /// Without a server id there is no session to drop and nothing to
    /// reconnect, so retrying would just repeat the same failure.
    #[tokio::test]
    async fn a_local_operation_with_no_server_is_never_retried() {
        let sessions = SshSessionManager::new();
        let attempts = AtomicUsize::new(0);

        let _: AppResult<()> = retry_on_connection_failure(&sessions, None, || async {
            attempts.fetch_add(1, Ordering::SeqCst);
            Err(AppError::Connection("connection reset".into()))
        })
        .await;

        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    /// Two callers racing for the same Node must serialize behind one lock,
    /// and two different Nodes must not block each other.
    #[tokio::test]
    async fn connect_locks_are_per_server() {
        let sessions = SshSessionManager::new();
        let server = Uuid::new_v4();

        let first = sessions.connect_lock(server).await;
        let second = sessions.connect_lock(server).await;
        assert!(Arc::ptr_eq(&first, &second), "the same server must share one connect lock");

        let other = sessions.connect_lock(Uuid::new_v4()).await;
        assert!(!Arc::ptr_eq(&first, &other), "different servers must not share a connect lock");

        // The shared lock genuinely excludes: a second acquisition cannot
        // proceed while the first guard is alive.
        let _held = first.lock().await;
        assert!(second.try_lock().is_err());
    }
}
