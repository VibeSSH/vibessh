//! Private registry credentials, and the image pull that uses them.
//!
//! Together because the credential exists only for the pull - and because
//! both halves have to keep the password off the command line, which is
//! where S-008 was found and, in `ensure_registry_login`, found a second
//! time after the first fix missed it.//!
//! Split out of a single 2685-line `application_service` (FIX_PLAN E.7).
//! Behaviour is unchanged; only the file boundaries moved.


use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{
    RegistryCredential, RuntimeType, SetRegistryCredentialInput,
};
use crate::runtime::docker_command::{DockerCommandRunner, LocalDocker};
use crate::services::ssh_service::retry_on_connection_failure;
// The one shared implementation - this module used to carry its own
// byte-identical copy, one of six across the codebase.
use crate::ssh::command::quote as shell_quote;
use crate::ssh::SshSession;
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::credentials;
use crate::storage::registry_credential_repository::RegistryCredentialRepository;
use crate::storage::server_repository::ServerRepository;

use super::*;

pub fn list_registry_credentials(repo: &RegistryCredentialRepository) -> AppResult<Vec<RegistryCredential>> {
    repo.list()
}

/// Sets (creating or overwriting) the login for one registry host - keyed
/// by `input.registry`, not a fresh row every call, so re-saving a
/// registry's credential updates the existing keyring entry in place rather
/// than leaking an orphaned one under a discarded id (see
/// `models::RegistryCredential`'s own doc comment on the one-row-per-host
/// shape).
pub fn set_registry_credential(repo: &RegistryCredentialRepository, input: SetRegistryCredentialInput) -> AppResult<RegistryCredential> {
    let registry = input.registry.trim();
    let username = input.username.trim();
    if registry.is_empty() {
        return Err(AppError::InvalidInput("a registry host is required".into()));
    }
    if username.is_empty() {
        return Err(AppError::InvalidInput("a username is required".into()));
    }
    if input.password.is_empty() {
        return Err(AppError::InvalidInput("a password or token is required".into()));
    }

    let credential = match repo.find_by_registry(registry)? {
        Some(existing) => {
            repo.update_username(existing.id, username)?;
            RegistryCredential { username: username.to_string(), ..existing }
        }
        None => repo.create(registry, username)?,
    };
    credentials::store_registry_credential_password(credential.id, &input.password)?;
    Ok(credential)
}

pub fn remove_registry_credential(repo: &RegistryCredentialRepository, id: Uuid) -> AppResult<()> {
    repo.delete(id)?;
    // Best-effort, same reasoning as every other "delete the row, then the
    // secret that went with it" cleanup in this file - the row is already
    // gone either way, and a leftover keyring entry under a dead id is
    // orphaned but harmless, not a correctness problem worth failing this
    // call over.
    if let Err(err) = credentials::delete_registry_credential_password(id) {
        log::warn!("couldn't remove the stored registry password for {id}: {err}");
    }
    Ok(())
}

/// Docker's own reference-parsing rule: the first path segment before a
/// `/` is a registry host only if it looks like one (has a `.` or `:`, or
/// is literally `localhost`) - otherwise the whole reference is an implicit
/// Docker Hub repository (`nginx:latest`, `someuser/someimage:tag`). Not
/// guessing - this is the same heuristic the real `docker` CLI/distribution
/// tooling itself uses to tell `myuser/myimage` (Hub) apart from
/// `ghcr.io/myuser/myimage` (not Hub).
// `pub(super)` purely so the unit tests in `mod.rs` can reach it. The tests
// stayed in one place when this file was split - moving a thousand lines of
// shared setup into eight files would have been a second, larger change
// wearing the same commit.
pub(super) fn registry_host(image: &str) -> &str {
    match image.split_once('/') {
        Some((first, _)) if first.contains('.') || first.contains(':') || first == "localhost" => first,
        _ => "docker.io",
    }
}

/// Best-effort `docker login` before a pull/create that might need one - a
/// no-op (not an error) when no credential is stored for the image's own
/// registry host, so an ordinary public-image pull costs nothing extra and
/// does not even reach the Node.
///
/// **The password reaches the Node through a mode-0600 file, not the
/// command string.** `--password-stdin` was already right about not using
/// `-p`, but the value was still piped in with
/// `printf '%s' '<password>' | docker login`, and that `printf` argument is
/// part of the command line - visible in `ps` to every local account for as
/// long as the login runs. This is the same finding as the MySQL half of
/// AUDIT S-008; the fix there landed first and this half was missed, so the
/// doc comment above it claimed a property the code did not have.
// `pub(super)`: `lifecycle` calls this before a start that may need to pull
// a private image. Not re-exported through the module's glob - nothing
// outside `application_service` has any business logging a Node into a
// registry.
pub(super) async fn ensure_registry_login(connection: &SshSession, registry_repo: &RegistryCredentialRepository, image: &str) -> AppResult<()> {
    let host = registry_host(image);
    let Some(credential) = registry_repo.find_by_registry(host)? else { return Ok(()) };
    let Some(password) = credentials::load_registry_credential_password(credential.id)? else { return Ok(()) };

    // A bare `docker login` (no host argument) targets Docker Hub - passing
    // `docker.io` explicitly as a host argument isn't guaranteed to be
    // treated the same way, so the sentinel gets the no-argument form
    // instead of just interpolating it in unconditionally.
    let target = if host == "docker.io" { String::new() } else { format!(" {}", shell_quote(host)) };

    let password_file = format!(".vibessh-registry-{}", uuid::Uuid::new_v4());
    crate::ssh::write_private_file(connection, &password_file, password.as_bytes()).await?;
    let command = format!(
        "sudo docker login{target} -u {} --password-stdin < {file}; rc=$?; rm -f {file}; exit $rc",
        shell_quote(&credential.username),
        file = shell_quote(&password_file),
    );
    let output = connection.execute_command(&command).await?;
    if output.exit_code != 0 {
        let detail = output.stderr.trim();
        let detail = if detail.is_empty() { "docker login failed".to_string() } else { detail.to_string() };
        return Err(AppError::Connection(format!("couldn't log in to {host}: {detail}")));
    }
    Ok(())
}

/// `docker pull` for a Docker Application's currently-configured image, on
/// the Node it actually runs on - the design doc's "aktualizacja image"
/// requirement. Only re-fetches whatever layers changed upstream since the
/// last pull (meaningful for a floating tag like `:latest`, or a version
/// tag whose upstream image was rebuilt in place) - an already-running
/// container keeps running its existing layers regardless, same as every
/// other `runtime_config`-adjacent change; the caller still needs a
/// Recreate to actually switch a running container onto the freshly
/// pulled layers. Returns Docker's own pull output (image digest, "Status:
/// Downloaded newer image" / "Image is up to date") for the caller to show
/// as proof something real happened, not just a bare success.
pub async fn pull_application_image(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    registry_repo: &RegistryCredentialRepository,
    application_id: Uuid,
) -> AppResult<String> {
    let detail = get_application(repo, application_id)?;
    if detail.application.runtime_type != RuntimeType::Docker {
        return Err(AppError::InvalidInput("pulling an image is only supported for Docker applications".into()));
    }
    let image = detail
        .runtime_config
        .get("image")
        .and_then(|value| value.as_str())
        .filter(|image| !image.trim().is_empty())
        .ok_or_else(|| AppError::InvalidInput("no image configured for this application".into()))?
        .to_string();
    let server_id = detail.application.server_id;

    retry_on_connection_failure(sessions, server_id, || async {
        // No Node is not a fault: `server_id` is `None` exactly when the
        // Application is local, and the daemon on this machine is then the
        // one to pull into.
        let runner: std::sync::Arc<dyn DockerCommandRunner> = match resolve_connection(server_repo, sessions, server_id).await? {
            Some(connection) => {
                ensure_registry_login(&connection, registry_repo, &image).await?;
                connection
            }
            None => {
                local_registry_login(registry_repo, &image).await?;
                std::sync::Arc::new(LocalDocker)
            }
        };
        let output = runner.docker(&["pull", &image]).await?;
        if output.exit_code != 0 {
            let detail = output.stderr.trim();
            let detail = if detail.is_empty() { "docker pull failed".to_string() } else { detail.to_string() };
            return Err(AppError::Connection(format!("couldn't pull '{image}': {detail}")));
        }
        Ok(output.stdout)
    })
    .await
}

/// `docker login` against the daemon on this machine.
///
/// The password goes down the process's own stdin and never touches disk.
/// The remote path above cannot do that - an SSH exec channel here has no
/// stdin to write into - so it stages the password in a private file and
/// removes it afterwards. Where that constraint does not apply, not writing
/// the secret out at all is the better of the two.
async fn local_registry_login(registry_repo: &RegistryCredentialRepository, image: &str) -> AppResult<()> {
    use tokio::io::AsyncWriteExt;

    let host = registry_host(image);
    let Some(credential) = registry_repo.find_by_registry(host)? else { return Ok(()) };
    let Some(password) = credentials::load_registry_credential_password(credential.id)? else { return Ok(()) };

    // A bare `docker login` targets Docker Hub; passing `docker.io` as a host
    // argument is not guaranteed to mean the same thing - same reasoning as
    // `ensure_registry_login`.
    let mut args: Vec<&str> = vec!["login"];
    if host != "docker.io" {
        args.push(host);
    }
    args.extend(["-u", credential.username.as_str(), "--password-stdin"]);

    let mut command = tokio::process::Command::new("docker");
    command.args(&args).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());

    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = command.spawn().map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            crate::runtime::docker_command::docker_missing()
        } else {
            AppError::Internal(format!("couldn't run docker login: {err}"))
        }
    })?;

    {
        let mut stdin = child.stdin.take().ok_or_else(|| AppError::Internal("docker login gave no stdin".into()))?;
        stdin.write_all(password.as_bytes()).await.map_err(|err| AppError::Internal(format!("couldn't send the password to docker: {err}")))?;
        // Dropped here on purpose: `--password-stdin` reads until end of
        // input, so it would wait forever with the pipe still open.
    }

    let output = child.wait_with_output().await.map_err(|err| AppError::Internal(format!("docker login didn't finish: {err}")))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if detail.is_empty() { "docker login failed".to_string() } else { detail };
        return Err(AppError::Connection(format!("couldn't log in to {host}: {detail}")));
    }
    Ok(())
}
