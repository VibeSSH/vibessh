//! Orchestrates `ApplicationRepository` + `BlueprintRegistry` + whichever
//! `ApplicationRuntime` a given Application's `runtime_type` resolves to
//! (via `runtime::runtime_for`) - the service layer
//! `commands::application_commands` calls into, same shape as
//! `server_service`/`ssh_service`.

use std::collections::HashMap;
use std::sync::Arc;

use uuid::Uuid;

use crate::blueprints::{BlueprintRegistry, ProvisionContext};
use crate::errors::{AppError, AppResult};
use crate::models::{
    Application, ApplicationDetail, ApplicationPort, ApplicationStatus, Blueprint, CreateApplicationFromBlueprintInput,
    CreateApplicationInput, HealthCheckType, PortInput, RuntimeType, SetHealthCheckInput, SetResourceLimitsInput,
};
use crate::runtime::local_process::LocalProcessManager;
use crate::runtime::{self, ApplicationRuntime, HealthCheckSpec, HealthStatus, ResourceUsage, RuntimeContext};
use crate::services::ssh_service::get_or_connect;
use crate::ssh::SshSession;
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::server_repository::ServerRepository;

pub fn list_applications(repo: &ApplicationRepository) -> AppResult<Vec<Application>> {
    repo.list()
}

pub fn get_application(repo: &ApplicationRepository, id: Uuid) -> AppResult<ApplicationDetail> {
    repo.get(id)?.ok_or_else(|| AppError::NotFound(format!("application {id}")))
}

pub fn list_blueprints(registry: &BlueprintRegistry) -> Vec<Blueprint> {
    registry.list().into_iter().cloned().collect()
}

/// **Known, deliberate scope gap**: this validates ports for collisions
/// against this same Application's *other* declared ports only (the
/// repository's own job, see `ApplicationRepository::add_port`'s doc
/// comment) - it does not check whether the port is actually free on the
/// target host, local or remote. That needs a real live probe (a bind
/// attempt locally, an `ss`/`netstat`-style query over SSH remotely) that
/// hasn't been built yet; declaring a port here is documentation of intent
/// today, not a guarantee nothing else on the host is already using it.
pub fn list_application_ports(repo: &ApplicationRepository, application_id: Uuid) -> AppResult<Vec<ApplicationPort>> {
    repo.list_ports(application_id)
}

pub fn add_application_port(repo: &ApplicationRepository, application_id: Uuid, port: &PortInput) -> AppResult<ApplicationPort> {
    repo.add_port(application_id, port)
}

pub fn update_application_port(
    repo: &ApplicationRepository,
    application_id: Uuid,
    port_id: Uuid,
    port: &PortInput,
) -> AppResult<ApplicationPort> {
    repo.update_port(application_id, port_id, port)
}

pub fn remove_application_port(repo: &ApplicationRepository, application_id: Uuid, port_id: Uuid) -> AppResult<()> {
    repo.remove_port(application_id, port_id)
}

/// Creates the working directory (local `create_dir_all`, or `mkdir -p`
/// over the same SSH connection every other Remote feature shares) before
/// the Application row itself is created - so a fresh working directory
/// the user just typed in the wizard (as opposed to one from an existing,
/// already-provisioned setup) doesn't leave `start()` failing on a bare
/// "No such file or directory" the user has to go fix by hand over a
/// separate SSH session. `mkdir -p` (not the single-level SFTP
/// `create_directory` the Files module uses) so a working directory nested
/// under parents that don't exist yet - `/srv/minecraft/my-server` on a
/// freshly provisioned host - still works in one step, and so an already-
/// existing directory is a no-op rather than an error.
pub async fn create_application(
    repo: &ApplicationRepository,
    registry: &BlueprintRegistry,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    input: CreateApplicationFromBlueprintInput,
) -> AppResult<ApplicationDetail> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(AppError::InvalidInput("a name is required".into()));
    }
    let working_directory = input.working_directory.trim();
    if working_directory.is_empty() {
        return Err(AppError::InvalidInput("a working directory is required".into()));
    }

    let handler = registry
        .get(&input.blueprint_id)
        .ok_or_else(|| AppError::InvalidInput(format!("unknown blueprint '{}'", input.blueprint_id)))?;
    if !handler.blueprint().supported_runtime_types.contains(&input.runtime_type) {
        return Err(AppError::InvalidInput(format!("'{}' doesn't support this runtime type", handler.blueprint().name)));
    }

    let mut blueprint_inputs: HashMap<String, serde_json::Value> = match input.blueprint_inputs {
        serde_json::Value::Object(map) => map.into_iter().collect(),
        serde_json::Value::Null => HashMap::new(),
        _ => return Err(AppError::InvalidInput("blueprint inputs must be an object".into())),
    };

    ensure_working_directory_exists(server_repo, sessions, input.server_id, working_directory).await?;

    // Resolved once, reused for provisioning - the same connection
    // `start_application` et al. would independently resolve later via
    // `load_runtime`, just needed here too for a blueprint that has to
    // reach the target host during creation (PaperBlueprint downloading a
    // jar over this same connection rather than through the SSH user's
    // desktop).
    let connection = resolve_connection(server_repo, sessions, input.server_id).await?;
    let provision_context = ProvisionContext { working_directory, connection };
    let discovered = handler.provision(&blueprint_inputs, &provision_context).await?;
    blueprint_inputs.extend(discovered);

    let runtime_config = handler.render_runtime_config(&blueprint_inputs)?;

    let create_input = CreateApplicationInput {
        server_id: input.server_id,
        name: name.to_string(),
        description: input.description,
        blueprint_id: input.blueprint_id,
        blueprint_version: handler.blueprint().blueprint_version,
        runtime_type: input.runtime_type,
        working_directory: working_directory.to_string(),
        environment: input.environment,
        ports: vec![],
        runtime_config,
        metadata: serde_json::json!({}),
    };
    repo.create(&create_input)
}

async fn ensure_working_directory_exists(
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Option<Uuid>,
    working_directory: &str,
) -> AppResult<()> {
    match server_id {
        None => tokio::fs::create_dir_all(working_directory)
            .await
            .map_err(|err| AppError::InvalidInput(format!("couldn't create working directory '{working_directory}': {err}"))),
        Some(server_id) => {
            let connection = get_or_connect(server_repo, sessions, server_id).await?;
            let output = connection.execute_command(&format!("mkdir -p {}", shell_quote(working_directory))).await?;
            if output.exit_code != 0 {
                let detail = output.stderr.trim();
                let detail = if detail.is_empty() { "mkdir failed".to_string() } else { detail.to_string() };
                return Err(AppError::InvalidInput(format!("couldn't create working directory '{working_directory}' on the remote host: {detail}")));
            }
            Ok(())
        }
    }
}

/// POSIX single-quote shell escaping - see `runtime::remote_process`'s copy
/// of the same function for the full reasoning; duplicated rather than
/// shared across the `services`/`runtime` module boundary, same as it's
/// already duplicated between `runtime::remote_process` and
/// `runtime::docker`.
fn shell_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

pub fn delete_application(repo: &ApplicationRepository, id: Uuid) -> AppResult<()> {
    repo.delete(id)
}

/// `None` for a Local application, `Some` (via the same cache-then-connect
/// path every other remote feature already shares) for a Remote one.
async fn resolve_connection(
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Option<Uuid>,
) -> AppResult<Option<Arc<SshSession>>> {
    match server_id {
        None => Ok(None),
        Some(server_id) => Ok(Some(get_or_connect(server_repo, sessions, server_id).await?)),
    }
}

/// The four `*_application` lifecycle functions below all need the exact
/// same three things before they can act - the Application's current
/// detail, its (possibly absent) SSH connection, and the runtime
/// implementation for its `runtime_type`. Factored here so each of them is
/// a couple of lines, not a repeat of this setup.
async fn load_runtime(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
) -> AppResult<(ApplicationDetail, Option<Arc<SshSession>>, Box<dyn ApplicationRuntime>)> {
    let detail = get_application(repo, id)?;
    let connection = resolve_connection(server_repo, sessions, detail.application.server_id).await?;
    let runtime = runtime::runtime_for(detail.application.runtime_type, local_process_manager.clone());
    Ok((detail, connection, runtime))
}

/// Re-reads status straight from the runtime and persists it - the only
/// way `Application::status`/`last_status_check_at` (a cache, never
/// trusted as sole truth - see `models::application`'s own doc comment)
/// ever gets updated.
async fn refresh_and_persist_status(
    repo: &ApplicationRepository,
    runtime: &dyn ApplicationRuntime,
    ctx: &RuntimeContext<'_>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    let status = runtime.status(ctx).await?;
    repo.update_status(id, status)?;
    Ok(status)
}

pub async fn start_application(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
    let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, connection };
    runtime.start(&ctx).await?;
    refresh_and_persist_status(repo, runtime.as_ref(), &ctx, id).await
}

pub async fn stop_application(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
    graceful: bool,
) -> AppResult<ApplicationStatus> {
    let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
    let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, connection };
    runtime.stop(&ctx, graceful).await?;
    refresh_and_persist_status(repo, runtime.as_ref(), &ctx, id).await
}

pub async fn restart_application(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
    let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, connection };
    runtime.restart(&ctx).await?;
    refresh_and_persist_status(repo, runtime.as_ref(), &ctx, id).await
}

pub async fn kill_application(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
    let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, connection };
    runtime.kill(&ctx).await?;
    refresh_and_persist_status(repo, runtime.as_ref(), &ctx, id).await
}

pub async fn refresh_application_status(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
    let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, connection };
    refresh_and_persist_status(repo, runtime.as_ref(), &ctx, id).await
}

pub async fn application_resource_usage(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
) -> AppResult<ResourceUsage> {
    let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
    let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, connection };
    runtime.resource_usage(&ctx).await
}

/// The last `max_lines` lines available right now - a snapshot the Logs tab
/// fetches on open and on manual refresh, same "pull, not push" shape
/// `ContainerLogsPanel`'s existing `get_server_container_logs` already
/// uses. Not live-streamed - see `runtime::mod`'s own `LogProvider` doc
/// comment for why that's a pull-based API in the first place.
pub async fn application_logs(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
    max_lines: u32,
) -> AppResult<Vec<String>> {
    let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
    let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, connection };
    runtime.logs(&ctx).await?.tail(max_lines).await
}

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
    if !matches!(detail.application.runtime_type, RuntimeType::Docker | RuntimeType::Systemd) {
        return Err(AppError::InvalidInput("resource limits are only supported for Docker and systemd applications".into()));
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
    let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
    let Some(spec) = resolve_health_check_spec(server_repo, &detail.application, &detail.ports)? else {
        return Ok(HealthStatus::Unknown);
    };
    let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, connection };
    runtime.health_check(&ctx, &spec).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real `ApplicationRepository` + `ServerRepository` against a fresh
    /// temp SQLite file, a real `LocalProcessManager`, and the real
    /// built-in `BlueprintRegistry` - the same components `lib.rs` wires
    /// together for the actual app, exercised end to end (create -> start
    /// -> status -> stop -> delete) rather than only unit-tested in
    /// isolation. `ServerRepository`/`SshSessionManager` are unused by a
    /// Local application's own lifecycle but still required by every
    /// function's signature, matching production's own shape.
    fn temp_setup() -> (ApplicationRepository, ServerRepository, SshSessionManager, Arc<LocalProcessManager>, BlueprintRegistry) {
        let path = std::env::temp_dir().join(format!("vibessh-app-service-test-{}.sqlite3", Uuid::new_v4()));
        let app_repo = ApplicationRepository::open(&path).unwrap();
        let server_repo = ServerRepository::open(&path).unwrap();
        (app_repo, server_repo, SshSessionManager::new(), Arc::new(LocalProcessManager::new()), BlueprintRegistry::with_builtins())
    }

    fn sleep_command_input() -> CreateApplicationFromBlueprintInput {
        #[cfg(windows)]
        let (command, args) = ("cmd", vec!["/C".to_string(), "echo hello-from-application-service && ping -n 6 127.0.0.1 >NUL".to_string()]);
        #[cfg(not(windows))]
        let (command, args) = ("sh", vec!["-c".to_string(), "echo hello-from-application-service; sleep 5".to_string()]);

        CreateApplicationFromBlueprintInput {
            server_id: None,
            name: "Integration Test App".to_string(),
            description: None,
            blueprint_id: "generic".to_string(),
            runtime_type: RuntimeType::LocalProcess,
            working_directory: std::env::temp_dir().to_string_lossy().into_owned(),
            environment: vec![],
            blueprint_inputs: serde_json::json!({ "command": command, "args": args }),
        }
    }

    #[tokio::test]
    async fn full_lifecycle_create_start_status_stop_delete() {
        let (app_repo, server_repo, sessions, local_process_manager, registry) = temp_setup();

        let detail = create_application(&app_repo, &registry, &server_repo, &sessions, sleep_command_input()).await.unwrap();
        assert_eq!(detail.application.status, ApplicationStatus::Unknown);
        assert_eq!(detail.runtime_config["command"], serde_json::json!(if cfg!(windows) { "cmd" } else { "sh" }));

        let status = start_application(&app_repo, &server_repo, &sessions, &local_process_manager, detail.application.id).await.unwrap();
        assert_eq!(status, ApplicationStatus::Running);

        let refreshed = get_application(&app_repo, detail.application.id).unwrap();
        assert_eq!(refreshed.application.status, ApplicationStatus::Running);

        // The stdout pump runs on its own background task - poll rather
        // than assume it's already flushed by the time start() returned.
        let mut saw_output = false;
        for _ in 0..30 {
            let lines = application_logs(&app_repo, &server_repo, &sessions, &local_process_manager, detail.application.id, 10).await.unwrap();
            if lines.iter().any(|line| line.contains("hello-from-application-service")) {
                saw_output = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(saw_output, "expected application_logs to eventually show the process's stdout");

        let status = stop_application(&app_repo, &server_repo, &sessions, &local_process_manager, detail.application.id, true).await.unwrap();
        assert_eq!(status, ApplicationStatus::Stopped);

        delete_application(&app_repo, detail.application.id).unwrap();
        assert!(get_application(&app_repo, detail.application.id).is_err());
    }

    #[tokio::test]
    async fn create_rejects_an_unknown_blueprint() {
        let (app_repo, server_repo, sessions, _local_process_manager, registry) = temp_setup();
        let mut input = sleep_command_input();
        input.blueprint_id = "does-not-exist".to_string();
        assert!(create_application(&app_repo, &registry, &server_repo, &sessions, input).await.is_err());
    }

    #[tokio::test]
    async fn create_rejects_a_runtime_type_the_blueprint_doesnt_support() {
        let (app_repo, server_repo, sessions, _local_process_manager, registry) = temp_setup();
        let mut input = sleep_command_input();
        input.runtime_type = RuntimeType::Docker;
        assert!(create_application(&app_repo, &registry, &server_repo, &sessions, input).await.is_err());
    }

    #[tokio::test]
    async fn create_rejects_a_blank_name() {
        let (app_repo, server_repo, sessions, _local_process_manager, registry) = temp_setup();
        let mut input = sleep_command_input();
        input.name = "   ".to_string();
        assert!(create_application(&app_repo, &registry, &server_repo, &sessions, input).await.is_err());
    }

    #[tokio::test]
    async fn create_creates_a_missing_local_working_directory() {
        let (app_repo, server_repo, sessions, _local_process_manager, registry) = temp_setup();
        let mut input = sleep_command_input();
        let fresh_dir = std::env::temp_dir().join(format!("vibessh-app-service-workdir-{}", Uuid::new_v4()));
        assert!(!fresh_dir.exists());
        input.working_directory = fresh_dir.to_string_lossy().into_owned();

        let detail = create_application(&app_repo, &registry, &server_repo, &sessions, input).await.unwrap();

        assert!(fresh_dir.is_dir());
        assert_eq!(detail.application.working_directory, fresh_dir.to_string_lossy());
        std::fs::remove_dir_all(&fresh_dir).ok();
    }

    #[tokio::test]
    async fn port_crud_add_update_remove_round_trips_through_the_service_layer() {
        let (app_repo, server_repo, sessions, _local_process_manager, registry) = temp_setup();
        let detail = create_application(&app_repo, &registry, &server_repo, &sessions, sleep_command_input()).await.unwrap();
        let application_id = detail.application.id;

        assert!(list_application_ports(&app_repo, application_id).unwrap().is_empty());

        let input = crate::models::PortInput {
            name: "game".to_string(),
            protocol: crate::models::PortProtocol::Tcp,
            bind_address: "0.0.0.0".to_string(),
            internal_port: 25565,
            external_port: None,
            required: false,
        };
        let added = add_application_port(&app_repo, application_id, &input).unwrap();
        assert_eq!(added.internal_port, 25565);
        assert_eq!(list_application_ports(&app_repo, application_id).unwrap().len(), 1);

        // Adding the exact same internal_port/bind_address/protocol again
        // is a real collision, not a silent duplicate - the service layer
        // must surface the repository's own collision error, not swallow it.
        assert!(add_application_port(&app_repo, application_id, &input).is_err());

        let updated_input = crate::models::PortInput { internal_port: 25566, ..input };
        let updated = update_application_port(&app_repo, application_id, added.id, &updated_input).unwrap();
        assert_eq!(updated.internal_port, 25566);

        remove_application_port(&app_repo, application_id, added.id).unwrap();
        assert!(list_application_ports(&app_repo, application_id).unwrap().is_empty());
    }

    fn create_raw(app_repo: &ApplicationRepository, runtime_type: RuntimeType, runtime_config: serde_json::Value) -> ApplicationDetail {
        // Bypasses `create_application`'s blueprint/runtime-type compatibility
        // check deliberately - Docker isn't one of the built-in blueprints'
        // `supported_runtime_types` yet (a real, separate gap, not this
        // test's concern), so this goes straight through the repository the
        // same way `runtime::docker`'s own unit tests build a stub
        // `Application` rather than going through the service layer.
        app_repo
            .create(&CreateApplicationInput {
                server_id: None,
                name: "Resource Limits Test App".to_string(),
                description: None,
                blueprint_id: "generic".to_string(),
                blueprint_version: 1,
                runtime_type,
                working_directory: std::env::temp_dir().to_string_lossy().into_owned(),
                environment: vec![],
                ports: vec![],
                runtime_config,
                metadata: serde_json::json!({}),
            })
            .unwrap()
    }

    #[test]
    fn set_application_resource_limits_rejects_a_runtime_type_that_cant_enforce_them() {
        let (app_repo, _server_repo, _sessions, _local_process_manager, _registry) = temp_setup();
        let local = create_raw(&app_repo, RuntimeType::LocalProcess, serde_json::json!({ "command": "sh", "args": [] }));

        let result = set_application_resource_limits(&app_repo, local.application.id, SetResourceLimitsInput { memory_limit_mb: Some(512), cpu_limit_cores: None });
        assert!(result.is_err());
    }

    #[test]
    fn set_application_resource_limits_patches_and_clears_the_docker_runtime_config() {
        let (app_repo, _server_repo, _sessions, _local_process_manager, _registry) = temp_setup();
        let docker = create_raw(&app_repo, RuntimeType::Docker, serde_json::json!({ "image": "alpine:latest", "command": [] }));

        let updated = set_application_resource_limits(
            &app_repo,
            docker.application.id,
            SetResourceLimitsInput { memory_limit_mb: Some(512), cpu_limit_cores: Some(1.5) },
        )
        .unwrap();
        assert_eq!(updated.runtime_config["memoryLimitMb"], serde_json::json!(512));
        assert_eq!(updated.runtime_config["cpuLimitCores"], serde_json::json!(1.5));
        // The rest of the config (set at creation, untouched by this call)
        // must survive the patch - this isn't a full runtime_config replace.
        assert_eq!(updated.runtime_config["image"], serde_json::json!("alpine:latest"));

        let cleared =
            set_application_resource_limits(&app_repo, docker.application.id, SetResourceLimitsInput { memory_limit_mb: None, cpu_limit_cores: None }).unwrap();
        assert!(cleared.runtime_config.get("memoryLimitMb").is_none());
        assert!(cleared.runtime_config.get("cpuLimitCores").is_none());
    }

    #[test]
    fn set_application_resource_limits_rejects_a_zero_memory_limit() {
        let (app_repo, _server_repo, _sessions, _local_process_manager, _registry) = temp_setup();
        let systemd = create_raw(&app_repo, RuntimeType::Systemd, serde_json::json!({ "command": "/usr/bin/java", "args": [] }));

        let result = set_application_resource_limits(&app_repo, systemd.application.id, SetResourceLimitsInput { memory_limit_mb: Some(0), cpu_limit_cores: None });
        assert!(result.is_err());
    }
}
