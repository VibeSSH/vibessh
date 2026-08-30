//! Orchestrates `ApplicationRepository` + `BlueprintRegistry` + whichever
//! `ApplicationRuntime` a given Application's `runtime_type` resolves to
//! (via `runtime::runtime_for`) - the service layer
//! `commands::application_commands` calls into, same shape as
//! `server_service`/`ssh_service`.

use std::collections::HashMap;
use std::sync::Arc;

use uuid::Uuid;

use crate::blueprints::BlueprintRegistry;
use crate::errors::{AppError, AppResult};
use crate::models::{
    Application, ApplicationDetail, ApplicationStatus, Blueprint, CreateApplicationFromBlueprintInput, CreateApplicationInput,
};
use crate::runtime::local_process::LocalProcessManager;
use crate::runtime::{self, ApplicationRuntime, ResourceUsage, RuntimeContext};
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

pub fn create_application(
    repo: &ApplicationRepository,
    registry: &BlueprintRegistry,
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

    let blueprint_inputs: HashMap<String, serde_json::Value> = match input.blueprint_inputs {
        serde_json::Value::Object(map) => map.into_iter().collect(),
        serde_json::Value::Null => HashMap::new(),
        _ => return Err(AppError::InvalidInput("blueprint inputs must be an object".into())),
    };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::RuntimeType;

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

        let detail = create_application(&app_repo, &registry, sleep_command_input()).unwrap();
        assert_eq!(detail.application.status, ApplicationStatus::Unknown);
        assert_eq!(detail.runtime_config["command"], serde_json::json!(if cfg!(windows) { "cmd" } else { "sh" }));

        let status = start_application(&app_repo, &server_repo, &sessions, &local_process_manager, detail.application.id).await.unwrap();
        assert_eq!(status, ApplicationStatus::Running);

        let refreshed = get_application(&app_repo, detail.application.id).unwrap();
        assert_eq!(refreshed.application.status, ApplicationStatus::Running);

        let status = stop_application(&app_repo, &server_repo, &sessions, &local_process_manager, detail.application.id, true).await.unwrap();
        assert_eq!(status, ApplicationStatus::Stopped);

        delete_application(&app_repo, detail.application.id).unwrap();
        assert!(get_application(&app_repo, detail.application.id).is_err());
    }

    #[test]
    fn create_rejects_an_unknown_blueprint() {
        let (app_repo, _server_repo, _sessions, _local_process_manager, registry) = temp_setup();
        let mut input = sleep_command_input();
        input.blueprint_id = "does-not-exist".to_string();
        assert!(create_application(&app_repo, &registry, input).is_err());
    }

    #[test]
    fn create_rejects_a_runtime_type_the_blueprint_doesnt_support() {
        let (app_repo, _server_repo, _sessions, _local_process_manager, registry) = temp_setup();
        let mut input = sleep_command_input();
        input.runtime_type = RuntimeType::Docker;
        assert!(create_application(&app_repo, &registry, input).is_err());
    }

    #[test]
    fn create_rejects_a_blank_name() {
        let (app_repo, _server_repo, _sessions, _local_process_manager, registry) = temp_setup();
        let mut input = sleep_command_input();
        input.name = "   ".to_string();
        assert!(create_application(&app_repo, &registry, input).is_err());
    }
}
