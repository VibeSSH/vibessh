//! Starting, stopping and inspecting a workload that already exists.
//!
//! Everything here goes through `ApplicationRuntime`, so nothing in this
//! module knows whether it is talking to Docker, systemd, a remote process
//! or a local one - which is exactly the seam this split follows.//!
//! Split out of a single 2685-line `application_service` (FIX_PLAN E.7).
//! Behaviour is unchanged; only the file boundaries moved.

use std::sync::Arc;

use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{
    Application, ApplicationPort, ApplicationStatus, HealthCheckType, RuntimeType,
};
use crate::runtime::local_process::LocalProcessManager;
use crate::runtime::{HealthCheckSpec, HealthStatus, ResourceUsage, RuntimeContext};
use crate::services::ssh_service::retry_on_connection_failure;
// The one shared implementation - this module used to carry its own
// byte-identical copy, one of six across the codebase.
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::registry_credential_repository::RegistryCredentialRepository;
use crate::storage::server_repository::ServerRepository;

use super::*;
use super::registry::ensure_registry_login;

pub async fn start_application(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    registry_repo: &RegistryCredentialRepository,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    let server_id = get_application(repo, id)?.application.server_id;
    retry_on_connection_failure(sessions, server_id, || async {
        let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
        // Best-effort login before whatever `start()` does under the hood
        // (a `docker create` that only pulls if the image isn't already
        // cached locally) - a no-op for every image without a stored
        // credential, see `ensure_registry_login`'s own doc comment.
        if let (Some(conn), Some(image)) = (&connection, detail.runtime_config.get("image").and_then(|v| v.as_str())) {
            ensure_registry_login(conn, registry_repo, image).await?;
        }
        let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, ports: &detail.ports, links: &detail.links, connection };
        runtime.start(&ctx).await?;
        refresh_and_persist_status(repo, runtime.as_ref(), &ctx, id).await
    })
    .await
}

pub async fn stop_application(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
    graceful: bool,
) -> AppResult<ApplicationStatus> {
    let server_id = get_application(repo, id)?.application.server_id;
    retry_on_connection_failure(sessions, server_id, || async {
        let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
        let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, ports: &detail.ports, links: &detail.links, connection };
        runtime.stop(&ctx, graceful).await?;
        refresh_and_persist_status(repo, runtime.as_ref(), &ctx, id).await
    })
    .await
}

pub async fn restart_application(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    let server_id = get_application(repo, id)?.application.server_id;
    retry_on_connection_failure(sessions, server_id, || async {
        let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
        let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, ports: &detail.ports, links: &detail.links, connection };
        runtime.restart(&ctx).await?;
        refresh_and_persist_status(repo, runtime.as_ref(), &ctx, id).await
    })
    .await
}

/// "Recreate Container" (Etap M1) - `destroy()` then `start()`, so an edited
/// image/command/resource-limit/restart-policy actually takes effect. Only
/// meaningful for `RuntimeType::Docker`: the other three runtimes already
/// regenerate their config on every `start()` (see each `ApplicationRuntime`
/// impl's own doc comment), so there's nothing a separate recreate step
/// would do beyond what starting already does - rejected outright rather
/// than silently doing nothing, same "don't pretend two runtimes have
/// identical capabilities" stance `set_application_resource_limits` already
/// takes. Safe to call while stopped or running - `destroy()` is a no-op if
/// nothing was ever created, and removes (stopping first) if it was.
pub async fn recreate_application(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    registry_repo: &RegistryCredentialRepository,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    let detail = get_application(repo, id)?;
    if detail.application.runtime_type != RuntimeType::Docker {
        return Err(AppError::InvalidInput("recreating is only meaningful for Docker applications".into()));
    }
    let server_id = detail.application.server_id;
    retry_on_connection_failure(sessions, server_id, || async {
        let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
        let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, ports: &detail.ports, links: &detail.links, connection };
        runtime.destroy(&ctx).await?;
        // Same best-effort login as `start_application` - `recreate` is
        // exactly the path a changed image (via `ApplicationConfigCard`/
        // `DockerImageCard`'s own auto-recreate) goes through, so this is
        // the realistic place a *new*, never-before-pulled private image
        // actually gets requested.
        if let (Some(conn), Some(image)) = (&ctx.connection, ctx.runtime_config.get("image").and_then(|v| v.as_str())) {
            ensure_registry_login(conn, registry_repo, image).await?;
        }
        runtime.start(&ctx).await?;
        refresh_and_persist_status(repo, runtime.as_ref(), &ctx, id).await
    })
    .await
}

pub async fn kill_application(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    let server_id = get_application(repo, id)?.application.server_id;
    retry_on_connection_failure(sessions, server_id, || async {
        let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
        let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, ports: &detail.ports, links: &detail.links, connection };
        runtime.kill(&ctx).await?;
        refresh_and_persist_status(repo, runtime.as_ref(), &ctx, id).await
    })
    .await
}

pub async fn refresh_application_status(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    let server_id = get_application(repo, id)?.application.server_id;
    retry_on_connection_failure(sessions, server_id, || async {
        let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
        let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, ports: &detail.ports, links: &detail.links, connection };
        refresh_and_persist_status(repo, runtime.as_ref(), &ctx, id).await
    })
    .await
}

pub async fn application_resource_usage(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
) -> AppResult<ResourceUsage> {
    let server_id = get_application(repo, id)?.application.server_id;
    retry_on_connection_failure(sessions, server_id, || async {
        let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
        let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, ports: &detail.ports, links: &detail.links, connection };
        runtime.resource_usage(&ctx).await
    })
    .await
}

/// Turns an Application's stored `health_check_*` columns into the
/// `HealthCheckSpec` its runtime actually probes with - `Ok(None)` (not an
/// error) whenever the configuration can't be resolved right now (the
/// referenced port was removed, or the target Server was deleted), matching
/// `Application::health_check_port_id`'s own doc comment: that case reports
/// `HealthStatus::Unknown`, not a failure.
fn resolve_health_check_spec(
    server_repo: &ServerRepository,
    application: &Application,
    ports: &[ApplicationPort],
) -> AppResult<Option<HealthCheckSpec>> {
    let resolve_port = || application.health_check_port_id.and_then(|port_id| ports.iter().find(|p| p.id == port_id)).map(|p| p.internal_port);

    Ok(match application.health_check_type {
        HealthCheckType::Process => Some(HealthCheckSpec::Process),
        HealthCheckType::Tcp => resolve_port().map(|port| HealthCheckSpec::Tcp { port }),
        HealthCheckType::Http => {
            let (Some(port), Some(path)) = (resolve_port(), application.health_check_http_path.clone()) else { return Ok(None) };
            Some(HealthCheckSpec::Http { port, path })
        }
        HealthCheckType::MinecraftStatus => {
            let Some(port) = resolve_port() else { return Ok(None) };
            let host = match application.server_id {
                None => "127.0.0.1".to_string(),
                Some(server_id) => match server_repo.get(server_id)? {
                    Some(server) => server.host,
                    None => return Ok(None),
                },
            };
            Some(HealthCheckSpec::MinecraftStatus { host, port })
        }
    })
}

pub async fn application_health_check(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
) -> AppResult<HealthStatus> {
    let server_id = get_application(repo, id)?.application.server_id;
    retry_on_connection_failure(sessions, server_id, || async {
        let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
        let Some(spec) = resolve_health_check_spec(server_repo, &detail.application, &detail.ports)? else {
            return Ok(HealthStatus::Unknown);
        };
        let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, ports: &detail.ports, links: &detail.links, connection };
        runtime.health_check(&ctx, &spec).await
    })
    .await
}
