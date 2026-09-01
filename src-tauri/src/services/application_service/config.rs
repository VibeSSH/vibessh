//! The settings that are stored rather than acted on: health check,
//! resource limits, image, environment.
//!
//! Each of these writes a row and returns. Making the change take effect on
//! a running workload is the caller's business - the frontend recreates -
//! which is what separates them from `provisioning`.//!
//! Split out of a single 2685-line `application_service` (FIX_PLAN E.7).
//! Behaviour is unchanged; only the file boundaries moved.


use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{
    ApplicationDetail, EnvironmentVariable, HealthCheckType, RuntimeType,
    SetHealthCheckInput, SetResourceLimitsInput,
};
use crate::runtime::{self};
// The one shared implementation - this module used to carry its own
// byte-identical copy, one of six across the codebase.
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::credentials;

use super::*;

/// Validates a health check configuration before it's stored - beyond the
/// repository's own "does `port_id` belong to this application" check
/// (`ApplicationRepository::set_health_check`), `Tcp`/`Http`/
/// `MinecraftStatus` are meaningless without a port, and `Http` additionally
/// needs a path that actually starts a request (`curl`/`reqwest` would
/// otherwise be asked to fetch a URL like `http://host:portfoo`).
pub fn set_application_health_check(
    repo: &ApplicationRepository,
    id: Uuid,
    input: SetHealthCheckInput,
) -> AppResult<ApplicationDetail> {
    if input.health_check_type != HealthCheckType::Process && input.port_id.is_none() {
        return Err(AppError::InvalidInput("this health check type needs a port".into()));
    }
    let http_path = match input.health_check_type {
        HealthCheckType::Http => {
            let path = input.http_path.as_deref().unwrap_or("").trim();
            if !path.starts_with('/') {
                return Err(AppError::InvalidInput("the HTTP health check path must start with '/'".into()));
            }
            Some(path.to_string())
        }
        _ => None,
    };
    repo.set_health_check(id, input.health_check_type, input.port_id, http_path.as_deref())?;
    get_application(repo, id)
}

/// Patches the `memoryLimitMb`/`cpuLimitCores` keys inside an Application's
/// own `runtime_config` - the same keys `runtime::docker::DockerConfig` and
/// `runtime::systemd::SystemdConfig` both read, under one shared name so
/// the frontend can offer a single "CPU cores" input regardless of which of
/// the two runtime types an Application actually uses. Rejected outright
/// for `LocalProcess`/`RemoteProcess`: there's no OS-level mechanism this
/// codebase can enforce a limit through for a bare child process the way
/// Docker/systemd already provide natively, and silently accepting a
/// setting that does nothing would be exactly the "pretend two runtimes
/// have identical capabilities" the architecture doc warns against, not an
/// honest gap.
pub fn set_application_resource_limits(
    repo: &ApplicationRepository,
    id: Uuid,
    input: SetResourceLimitsInput,
) -> AppResult<ApplicationDetail> {
    let detail = get_application(repo, id)?;
    if !matches!(detail.application.runtime_type, RuntimeType::Docker | RuntimeType::Systemd | RuntimeType::RemoteProcess) {
        return Err(AppError::InvalidInput("resource limits are only supported for Docker, systemd, and remote process applications".into()));
    }
    // A bare SSH-launched process has no cgroup of its own to cap memory
    // through the way Docker/systemd do (see `runtime::remote_process`'s own
    // `cpulimit`-based CPU cap for why CPU is still possible there) - reject
    // rather than silently store a limit that will never actually apply.
    if detail.application.runtime_type == RuntimeType::RemoteProcess && input.memory_limit_mb.is_some() {
        return Err(AppError::InvalidInput("a memory limit isn't supported for remote process applications".into()));
    }
    runtime::validate_resource_limits(input.memory_limit_mb, input.cpu_limit_cores)?;

    let mut runtime_config = detail.runtime_config.clone();
    let object = runtime_config.as_object_mut().ok_or_else(|| AppError::Internal("runtime_config wasn't a JSON object".into()))?;
    match input.memory_limit_mb {
        Some(mb) => object.insert("memoryLimitMb".to_string(), serde_json::json!(mb)),
        None => object.remove("memoryLimitMb"),
    };
    match input.cpu_limit_cores {
        Some(cores) => object.insert("cpuLimitCores".to_string(), serde_json::json!(cores)),
        None => object.remove("cpuLimitCores"),
    };

    repo.update_runtime_config(id, &runtime_config)
}

/// Patches only the `image` key inside a Docker Application's own
/// `runtime_config` - same one-key-patch shape `set_application_resource_limits`
/// already established, Docker-only for the same kind of reason: `image` is
/// a Docker-specific concept, the other three runtime types run a bare
/// `command`/`args` instead and have nothing here to change. Like every
/// other `runtime_config` edit in this codebase, this alone doesn't affect
/// an already-created container - the caller still needs a Recreate
/// (`recreate_application`) for a running Application to actually pick up
/// the new image, same "change it, save it, then Recreate" pattern the
/// frontend already applies for resource limits/environment/ports.
pub fn set_application_image(repo: &ApplicationRepository, id: Uuid, image: String) -> AppResult<ApplicationDetail> {
    let detail = get_application(repo, id)?;
    if detail.application.runtime_type != RuntimeType::Docker {
        return Err(AppError::InvalidInput("the image is only configurable for Docker applications".into()));
    }
    let image = image.trim();
    if image.is_empty() {
        return Err(AppError::InvalidInput("an image is required".into()));
    }
    if image.contains(['\n', '\r']) {
        return Err(AppError::InvalidInput("the image can't contain a newline".into()));
    }

    let mut runtime_config = detail.runtime_config.clone();
    let object = runtime_config.as_object_mut().ok_or_else(|| AppError::Internal("runtime_config wasn't a JSON object".into()))?;
    object.insert("image".to_string(), serde_json::json!(image));

    repo.update_runtime_config(id, &runtime_config)
}

/// Replaces an Application's whole environment variable set - same
/// "editable after creation, not just once in the wizard" bar
/// `set_application_resource_limits`/`update_application_config` already
/// meet for their own fields. No runtime-type restriction (unlike resource
/// limits): every runtime already reads `ctx.environment` the same way, so
/// there's no runtime that genuinely can't support this. Key/value
/// validation itself is deliberately left to whichever runtime's own
/// `start`/`create_container` reads this back (`validate_environment` in
/// `runtime::docker`/`runtime::remote_process`) rather than duplicated
/// here - the same single-source-of-truth reasoning
/// `update_application_config` already applies to blueprint field
/// validation.
/// **Secret handling**: the frontend never has a secret row's real value to
/// resend (`ApplicationRepository::get` always redacts it - see
/// `EnvironmentVariable::value`'s own doc comment), so a secret row with an
/// empty value here means "unchanged," not "clear it" - resolved below by
/// keeping the previous real value from the keyring. A key that was secret
/// before and no longer appears (removed, or flipped back to a plain
/// variable) has its keyring entry deleted, so nothing outlives the row
/// that referenced it.
pub fn set_application_environment(repo: &ApplicationRepository, id: Uuid, environment: Vec<EnvironmentVariable>) -> AppResult<ApplicationDetail> {
    let previous = get_application(repo, id)?.environment;

    let mut resolved = Vec::with_capacity(environment.len());
    for mut env in environment {
        if env.is_secret && env.value.is_empty() && previous.iter().any(|p| p.key == env.key && p.is_secret) {
            env.value = credentials::load_environment_secret(id, &env.key)?.unwrap_or_default();
        }
        resolved.push(env);
    }

    for prev in &previous {
        if prev.is_secret && !resolved.iter().any(|env| env.key == prev.key && env.is_secret) {
            if let Err(err) = credentials::delete_environment_secret(id, &prev.key) {
                log::warn!("couldn't remove the stored '{}' secret for application {id}: {err}", prev.key);
            }
        }
    }

    repo.set_environment(id, &resolved)?;
    store_secret_environment_values(id, &resolved)?;
    get_application(repo, id)
}
