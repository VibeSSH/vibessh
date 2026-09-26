//! Orchestrates `server_repository` (non-secret fields) and `credentials`
//! (password / key passphrase) so callers never have to remember to touch
//! both. Validation lives here too, not in the repository, so the SQLite
//! layer stays a plain CRUD store.

use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{AuthenticationType, NodeCapabilities, Server, ServerInput};
use crate::services::ssh_service::get_or_connect;
use crate::state::SshSessionManager;
use crate::storage::credentials::{self, SecretKind};
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::server_repository::ServerRepository;

pub fn create_server(repo: &ServerRepository, input: ServerInput) -> AppResult<Server> {
    validate_common(&input)?;
    validate_secret_present_for_create(&input)?;
    let server = repo.create(&input)?;
    persist_secrets(server.id, &input)?;
    Ok(server)
}

pub fn update_server(repo: &ServerRepository, id: Uuid, input: ServerInput) -> AppResult<Server> {
    validate_common(&input)?;
    let server = repo.update(id, &input)?;
    // A blank password/passphrase means "leave it as it was" - the frontend
    // never has the existing secret to redisplay, so it can't resend it.
    persist_secrets(id, &input)?;
    Ok(server)
}

/// Sets or clears a Node's icon. See `ServerRepository::set_icon` for why
/// this is separate from `update_server` and where the validation lives.
pub fn set_server_icon(repo: &ServerRepository, id: Uuid, icon: Option<String>) -> AppResult<Server> {
    repo.set_icon(id, icon.as_deref())
}

/// Refused while applications are on the server, naming them - the foreign
/// key in `storage::migrations` refuses it too, but only as a bare id.
pub fn delete_server(repo: &ServerRepository, app_repo: &ApplicationRepository, id: Uuid) -> AppResult<()> {
    let attached: Vec<String> = app_repo
        .list()?
        .into_iter()
        .filter(|application| application.server_id == Some(id))
        .map(|application| application.name)
        .collect();
    if !attached.is_empty() {
        let server = repo.get(id)?.map(|server| server.name).unwrap_or_else(|| id.to_string());
        return Err(AppError::ServerHasApplications { server, applications: attached.join(", ") });
    }
    repo.delete(id)?;
    // Best-effort: the row is already gone, and delete_secret already treats
    // "nothing to delete" as success, so these can't meaningfully fail in a
    // way the caller should roll back for.
    credentials::forget_secret(id, SecretKind::SshPassword);
    // The in-memory one too - it never reached the keyring, so nothing
    // above would have cleared it.
    crate::state::session_passwords::forget(id);
    credentials::forget_secret(id, SecretKind::SshKeyPassphrase);
    Ok(())
}

pub fn get_server(repo: &ServerRepository, id: Uuid) -> AppResult<Server> {
    repo.get(id)?.ok_or_else(|| AppError::NotFound(format!("server {id}")))
}

pub fn list_servers(repo: &ServerRepository) -> AppResult<Vec<Server>> {
    repo.list()
}

pub fn upsert_agent_server(
    repo: &ServerRepository,
    name: &str,
    host: &str,
    agent_id: Uuid,
    docker_capable: Option<bool>,
) -> AppResult<Server> {
    if name.trim().is_empty() {
        return Err(AppError::InvalidInput("server name cannot be empty".into()));
    }
    if host.trim().is_empty() {
        return Err(AppError::InvalidInput("host cannot be empty".into()));
    }
    repo.upsert_agent(name, host, agent_id, docker_capable.map(|docker| NodeCapabilities { docker, ..Default::default() }))
}

/// Converts an already-known SSH-mode Server to Agent mode in place - see
/// `ServerRepository::upgrade_to_agent`'s own doc comment for why this is
/// a distinct operation from `upsert_agent_server` above (that one always
/// either matches an existing Agent by its own agent_id or creates a brand
/// new row; this one always updates one specific, already-known server id).
pub fn upgrade_server_to_agent(repo: &ServerRepository, server_id: Uuid, agent_id: Uuid, docker_capable: Option<bool>) -> AppResult<Server> {
    repo.upgrade_to_agent(server_id, agent_id, docker_capable.map(|docker| NodeCapabilities { docker, ..Default::default() }))
}

/// A real SSH-exec probe for all three requirements the Setup flow cares
/// about (`command -v docker`/`network::wireguard::detect`/
/// `firewall::ufw::UfwProvider::detect`), not a guess from the image/OS
/// name - the same "actually check, don't assume" stance
/// `runtime::docker::DockerRuntime::validate` already takes when a Docker
/// Application is created. SSH-mode only: an Agent-mode Node's Docker
/// capability comes from its own handshake instead (see
/// `upsert_agent_server` above) - there's no persistent Agent connection
/// this could reuse outside the pairing flow yet, and WireGuard/ufw aren't
/// reported by that handshake at all. Run on demand (the Create Application
/// wizard's Node picker calls this per SSH-mode server as it loads, and the
/// Node Setup flow calls it to check/re-check requirements) rather than on
/// a schedule - none of the three are things that get installed or removed
/// from a host often enough to justify a background poller. Persists the
/// result before returning it, so a probe that ran once survives an app
/// restart even if nothing else asks again for a while. Every
/// `install_*` function below ends by calling this rather than constructing
/// its own partial `NodeCapabilities`, so installing one requirement can
/// never clobber another's already-known state.
pub async fn probe_node_capabilities(repo: &ServerRepository, sessions: &SshSessionManager, server_id: Uuid) -> AppResult<NodeCapabilities> {
    let connection = get_or_connect(repo, sessions, server_id).await?;
    let output = connection.execute_command("command -v docker >/dev/null 2>&1 && echo yes || echo no").await?;
    let docker = output.stdout.trim() == "yes";
    let wireguard = crate::network::wireguard::detect(&connection).await?;
    let ufw = crate::firewall::ufw::UfwProvider::detect(&connection).await?;
    let capabilities = NodeCapabilities { docker, wireguard, ufw };
    repo.set_node_capabilities(server_id, capabilities)?;
    Ok(capabilities)
}

/// Runs Docker's own official convenience script
/// (https://get.docker.com/docs/install) over the existing SSH connection -
/// the same install method Docker's own docs recommend, which auto-detects
/// the distro and already shells out to `sudo` itself when this SSH user
/// isn't already root. Downloaded to a temp file and run from there (not
/// piped straight from curl into sh) so a truncated download can't ever
/// execute a half-written script, and the temp file is removed either way.
///
/// **The temp file has an unguessable name in a private directory.** It used
/// to be a fixed `/tmp/vibessh-get-docker.sh`, which any local account could
/// create first - as a symlink, so the download lands somewhere else, or as
/// their own script, swapped in between the download finishing and `sh`
/// starting. Either way something they wrote runs as root. `mktemp -d` gives
/// a 0700 directory with a random name, created atomically, and nothing can
/// be planted inside one that already belongs to us. The `trap` removes it
/// on every exit path, which the old `rm` after a `;` did not manage for a
/// script killed part-way.
///
/// Always re-probes and persists capabilities afterward rather than trusting
/// the script's own exit code alone - the same "actually check" stance
/// `probe_node_capabilities` itself takes, since a script can exit 0 having
/// silently skipped the actual install step on an unrecognized distro.
pub async fn install_docker(repo: &ServerRepository, sessions: &SshSessionManager, server_id: Uuid) -> AppResult<NodeCapabilities> {
    let connection = get_or_connect(repo, sessions, server_id).await?;
    let script = "set -e; d=$(mktemp -d /tmp/vibessh-docker.XXXXXXXXXX); \
        trap 'rm -rf \"$d\"' EXIT; \
        curl -fsSL https://get.docker.com -o \"$d/get-docker.sh\"; \
        sh \"$d/get-docker.sh\"";
    let output = connection.execute_command(script).await?;
    if output.exit_code != 0 {
        let detail = output.stderr.trim();
        let detail = if detail.is_empty() { "the Docker install script failed".to_string() } else { detail.to_string() };
        return Err(AppError::Connection(format!("couldn't install Docker: {detail}")));
    }

    // Best-effort, deliberately not checked for success: VibeSSH's own
    // Docker commands (`runtime::docker`, `ssh::docker`) already run under
    // `sudo` unconditionally so they never depend on this, but a human who
    // SSHes into this Node by hand shouldn't have to know to run `sudo`
    // before every plain `docker ...` either - the whole point of Etap M1's
    // auto-install is that nothing here needs a manual follow-up step. A
    // freshly `usermod`'d group membership only takes effect on a *new*
    // login session, never the one that just ran this, so there's nothing
    // to verify here regardless.
    let _ = connection.execute_command("sudo usermod -aG docker \"$(whoami)\"").await;

    let capabilities = probe_node_capabilities(repo, sessions, server_id).await?;
    if !capabilities.docker {
        return Err(AppError::Connection(
            "the install script finished without reporting an error, but Docker still isn't on this Node's PATH - it may need a manual install for this distro".into(),
        ));
    }
    Ok(capabilities)
}

/// Installs WireGuard if missing (`network::wireguard::install_if_missing`
/// - a no-op if it's already there) and re-probes/persists every
/// capability afterward, same shape as `install_docker`. Doesn't generate a
/// keypair or join the Vibe Network by itself - that's
/// `services::network_service::join_network`'s own job, deliberately kept
/// separate (installing the tool and actually joining a mesh are two
/// different, independently useful actions).
pub async fn install_wireguard(repo: &ServerRepository, sessions: &SshSessionManager, server_id: Uuid) -> AppResult<NodeCapabilities> {
    let connection = get_or_connect(repo, sessions, server_id).await?;
    crate::network::wireguard::install_if_missing(&connection).await?;
    probe_node_capabilities(repo, sessions, server_id).await
}

/// Installs ufw via `apt-get` - Debian/Ubuntu-only, same target-OS
/// assumption `network::wireguard::install_if_missing` and every built-in
/// blueprint already make. Only installs the package; does **not** enable
/// enforcement - see `firewall::mod`'s own doc comment for why turning
/// enforcement on is always a separate, explicit action
/// (`services::firewall_service::enable_node_firewall`), never a side
/// effect of a Setup step.
pub async fn install_ufw(repo: &ServerRepository, sessions: &SshSessionManager, server_id: Uuid) -> AppResult<NodeCapabilities> {
    let connection = get_or_connect(repo, sessions, server_id).await?;
    let output = connection.execute_command("sudo apt-get update -qq && sudo DEBIAN_FRONTEND=noninteractive apt-get install -y ufw").await?;
    if output.exit_code != 0 {
        let detail = output.stderr.trim();
        let detail = if detail.is_empty() { "the ufw install failed".to_string() } else { detail.to_string() };
        return Err(AppError::Connection(format!("couldn't install ufw: {detail}")));
    }

    let capabilities = probe_node_capabilities(repo, sessions, server_id).await?;
    if !capabilities.ufw {
        return Err(AppError::Connection(
            "the install finished without reporting an error, but ufw still isn't on this Node's PATH - it may need a manual install for this distro".into(),
        ));
    }
    Ok(capabilities)
}

/// Records a new SSH host key for a server after the person has compared it
/// with what the Node itself reports - the way out of a host-key mismatch.
///
/// Stores exactly the fingerprint that was shown to them, not whatever the
/// Node presents next: if the key changed yet again in between, the next
/// connection is refused again rather than trusting a key nobody looked at.
/// Nothing else ever replaces a recorded key; that is the point of pinning.
pub fn trust_host_key(repo: &ServerRepository, id: Uuid, fingerprint: &str) -> AppResult<()> {
    let fingerprint = fingerprint.trim();
    let well_formed = fingerprint
        .strip_prefix("SHA256:")
        .is_some_and(|hash| !hash.is_empty() && hash.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=')));
    if !well_formed {
        return Err(AppError::InvalidInput(format!("'{fingerprint}' isn't an SSH key fingerprint")));
    }
    repo.get(id)?.ok_or_else(|| AppError::NotFound(format!("server {id}")))?;
    log::warn!("the SSH host key for server {id} was replaced by the operator with {fingerprint}");
    repo.set_known_host_fingerprint(id, fingerprint)
}

/// Replaces a server's SSH password after the Node rejected the one it had.
///
/// Goes where Edit Server puts a password - the OS credential store - because
/// that is read *first* on connect: holding the new one only in memory would
/// leave the rejected one winning on every attempt. Where there is no store
/// to write to, it falls back to this run's memory, exactly as the
/// missing-password prompt does, and says so in the log rather than failing
/// the one path that gets somebody back in.
pub fn replace_ssh_password(repo: &ServerRepository, id: Uuid, password: &str) -> AppResult<()> {
    if password.is_empty() {
        return Err(AppError::InvalidInput("the password is empty".into()));
    }
    let server = repo.get(id)?.ok_or_else(|| AppError::NotFound(format!("server {id}")))?;
    if server.authentication_type != AuthenticationType::Password {
        return Err(AppError::InvalidInput("this server signs in with a key, not a password".into()));
    }
    match credentials::store_secret(id, SecretKind::SshPassword, password) {
        Ok(()) => {
            // A password remembered for this run would now be the stale one.
            crate::state::session_passwords::forget(id);
            Ok(())
        }
        Err(err) => {
            log::warn!("couldn't store the replacement SSH password for {id}, keeping it for this run only: {err}");
            crate::state::session_passwords::remember(id, password.to_string());
            Ok(())
        }
    }
}

fn persist_secrets(id: Uuid, input: &ServerInput) -> AppResult<()> {
    if let Some(password) = non_blank(&input.password) {
        credentials::store_secret(id, SecretKind::SshPassword, password)?;
    }
    if let Some(passphrase) = non_blank(&input.key_passphrase) {
        credentials::store_secret(id, SecretKind::SshKeyPassphrase, passphrase)?;
    }
    Ok(())
}

fn non_blank(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

fn validate_common(input: &ServerInput) -> AppResult<()> {
    if input.name.trim().is_empty() {
        return Err(AppError::InvalidInput("server name cannot be empty".into()));
    }
    if input.host.trim().is_empty() {
        return Err(AppError::InvalidInput("host cannot be empty".into()));
    }
    if input.username.trim().is_empty() {
        return Err(AppError::InvalidInput("username cannot be empty".into()));
    }
    if input.ssh_port == 0 {
        return Err(AppError::InvalidInput("SSH port must be between 1 and 65535".into()));
    }
    if input.authentication_type == AuthenticationType::PrivateKey
        && non_blank(&input.private_key_path).is_none()
    {
        return Err(AppError::InvalidInput(
            "a private key file path is required for key-based authentication".into(),
        ));
    }
    Ok(())
}

fn validate_secret_present_for_create(input: &ServerInput) -> AppResult<()> {
    if input.authentication_type == AuthenticationType::Password && non_blank(&input.password).is_none() {
        return Err(AppError::InvalidInput(
            "a password is required for password authentication".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_input() -> ServerInput {
        ServerInput {
            name: "Production".to_string(),
            host: "203.0.113.10".to_string(),
            ssh_port: 22,
            username: "root".to_string(),
            authentication_type: AuthenticationType::Password,
            private_key_path: None,
            group_id: None,
            password: Some("hunter2".to_string()),
            key_passphrase: None,
        }
    }

    fn temp_repo() -> ServerRepository {
        let path = std::env::temp_dir().join(format!("vibessh-service-test-{}.sqlite3", Uuid::new_v4()));
        ServerRepository::open(&path).unwrap()
    }

    /// See `storage::credentials::KEYRING_TEST_LOCK` - any test here that
    /// goes through `create_server`/`update_server`/`delete_server` (and so
    /// touches the real OS keyring) takes this first.
    fn keyring_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::storage::credentials::KEYRING_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn create_rejects_an_empty_name() {
        let repo = temp_repo();
        let mut input = valid_input();
        input.name = "   ".to_string();
        let err = create_server(&repo, input).unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }

    #[test]
    fn create_rejects_password_auth_without_a_password() {
        let repo = temp_repo();
        let mut input = valid_input();
        input.password = None;
        let err = create_server(&repo, input).unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }

    #[test]
    fn create_rejects_key_auth_without_a_key_path() {
        let repo = temp_repo();
        let mut input = valid_input();
        input.authentication_type = AuthenticationType::PrivateKey;
        input.password = None;
        let err = create_server(&repo, input).unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }

    #[test]
    fn create_stores_the_password_in_the_keyring_and_delete_removes_it() {
        let _guard = keyring_lock();
        let repo = temp_repo();
        let server = create_server(&repo, valid_input()).unwrap();

        assert_eq!(
            credentials::load_secret(server.id, SecretKind::SshPassword).unwrap(),
            Some("hunter2".to_string())
        );

        let app_repo = crate::storage::application_repository::ApplicationRepository::open(
            &std::env::temp_dir().join(format!("vibessh-server-service-apps-{}.sqlite3", Uuid::new_v4())),
        )
        .unwrap();
        delete_server(&repo, &app_repo, server.id).unwrap();
        assert_eq!(credentials::load_secret(server.id, SecretKind::SshPassword).unwrap(), None);
        assert!(get_server(&repo, server.id).is_err());
    }

    #[test]
    fn update_with_a_blank_password_keeps_the_existing_secret() {
        let _guard = keyring_lock();
        let repo = temp_repo();
        let server = create_server(&repo, valid_input()).unwrap();

        let mut update = valid_input();
        update.name = "Renamed".to_string();
        update.password = None;
        update_server(&repo, server.id, update).unwrap();

        assert_eq!(
            credentials::load_secret(server.id, SecretKind::SshPassword).unwrap(),
            Some("hunter2".to_string())
        );

        credentials::forget_secret(server.id, SecretKind::SshPassword);
    }

    #[test]
    fn upsert_agent_server_rejects_an_empty_name() {
        let repo = temp_repo();
        let err = upsert_agent_server(&repo, "  ", "203.0.113.20", Uuid::new_v4(), None).unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }

    #[test]
    fn upsert_agent_server_persists_a_real_row() {
        let repo = temp_repo();
        let agent_id = Uuid::new_v4();
        let server = upsert_agent_server(&repo, "Prod Agent", "203.0.113.20", agent_id, Some(true)).unwrap();
        assert_eq!(get_server(&repo, server.id).unwrap().agent_id, Some(agent_id));
        assert_eq!(get_server(&repo, server.id).unwrap().node_capabilities, Some(NodeCapabilities { docker: true, ..Default::default() }));
    }

    #[test]
    fn list_returns_created_servers() {
        let _guard = keyring_lock();
        let repo = temp_repo();
        let server = create_server(&repo, valid_input()).unwrap();
        let servers = list_servers(&repo).unwrap();
        assert!(servers.iter().any(|s| s.id == server.id));
        credentials::forget_secret(server.id, SecretKind::SshPassword);
    }
}
