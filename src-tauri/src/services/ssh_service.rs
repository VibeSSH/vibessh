//! Resolves a `Server`/`ServerInput` plus its keyring secret into the
//! `SshCredentials` the `ssh` module needs, and owns the two real entry
//! points: testing a connection before a server is saved, and running a
//! command against one that already is (via the cached `SshSessionManager`).

use std::sync::Arc;

use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{AuthenticationType, Server, ServerInput};
use crate::ssh::{self, SshAuth, SshCredentials, SshSession, TerminalHandle};
use crate::state::SshSessionManager;
use crate::storage::credentials::{self, SecretKind};
use crate::storage::server_repository::ServerRepository;
use crate::transport::{CommandOutput, ProcessSummary, RemoteFileEntry, ServerMetrics, ServiceSummary};

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
    let session = get_or_connect(repo, sessions, server_id).await?;
    match session.execute_command(command).await {
        Ok(output) => Ok(output),
        Err(first_err) => {
            sessions.remove(server_id).await;
            let session = get_or_connect(repo, sessions, server_id).await?;
            session.execute_command(command).await.map_err(|_| first_err)
        }
    }
}

/// Opens an interactive shell against a saved server, reusing a cached
/// connection when there is one. Unlike `execute_command`, a dead cached
/// connection here isn't retried automatically - opening a terminal is a
/// user-initiated action with its own visible feedback, so surfacing the
/// failure and letting them try again is clearer than silently reconnecting
/// underneath a UI element they just clicked.
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

async fn get_or_connect(repo: &ServerRepository, sessions: &SshSessionManager, server_id: Uuid) -> AppResult<Arc<SshSession>> {
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
            let passphrase = credentials::load_secret(server.id, SecretKind::SshKeyPassphrase)?;
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
