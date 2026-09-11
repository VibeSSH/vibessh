//! Orchestrates `ApplicationRepository` + `BlueprintRegistry` + whichever
//! `ApplicationRuntime` a given Application's `runtime_type` resolves to
//! (via `runtime::runtime_for`) - the service layer
//! `commands::application_commands` calls into, same shape as
//! `server_service`/`ssh_service`.
//!
//! Split into submodules by concern (FIX_PLAN E.7). This file keeps the
//! handful of helpers every one of them needs - resolving a connection,
//! loading a runtime, reading an Application - and re-exports the rest, so
//! `commands::application_commands` and `services::mod` see exactly the
//! same surface they did when this was one file.

use std::sync::Arc;

use uuid::Uuid;

use crate::blueprints::BlueprintRegistry;
use crate::errors::{AppError, AppResult};
use crate::models::{
    Application, ApplicationDetail, ApplicationStatus, Blueprint,
};
use crate::runtime::local_process::LocalProcessManager;
use crate::runtime::{self, ApplicationRuntime, RuntimeContext};
use crate::services::ssh_service::get_or_connect;
// The one shared implementation - this module used to carry its own
// byte-identical copy, one of six across the codebase.
use crate::ssh::SshSession;
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::server_repository::ServerRepository;

mod config;
mod lifecycle;
mod links;
mod logs;
mod ports;
mod provisioning;
mod registry;
mod teardown;

pub use config::*;
pub use lifecycle::*;
pub use links::*;
pub use logs::*;
pub use ports::*;
// The glob re-exports above carry only `pub` items, so the crate-visible
// helpers `migration_service` reaches for need naming explicitly.
pub(crate) use provisioning::{ensure_working_directory_exists, resolve_environment_secrets, store_secret_environment_values};
pub use provisioning::*;
pub use registry::*;
pub use teardown::*;

pub fn list_applications(repo: &ApplicationRepository) -> AppResult<Vec<Application>> {
    repo.list()
}

pub fn get_application(repo: &ApplicationRepository, id: Uuid) -> AppResult<ApplicationDetail> {
    repo.get(id)?.ok_or_else(|| AppError::NotFound(format!("application {id}")))
}

pub fn list_blueprints(registry: &BlueprintRegistry) -> Vec<Blueprint> {
    registry.list().into_iter().cloned().collect()
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
    let mut detail = get_application(repo, id)?;
    // Every other reader of `ApplicationDetail` (the Tauri commands that
    // hand it to the frontend) sees a secret row redacted - this is the one
    // path that's actually about to start/inspect the real process, so it's
    // the one place real secret values get resolved back in.
    detail.environment = resolve_environment_secrets(detail.application.id, detail.environment)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    // Imported here rather than at the top of the file: these are used only
    // by the tests, and a `#[cfg(test)]`-only import at module scope reads as
    // dead to every non-test build.
    use crate::models::{
        CreateApplicationFromBlueprintInput, CreateApplicationInput, EnvironmentVariable, PortInput, PortVisibility, RuntimeType,
        SetResourceLimitsInput,
    };
    use crate::storage::firewall_rule_repository::FirewallRuleRepository;
    use crate::storage::log_capture::LogCaptureStore;
    use crate::storage::node_network_repository::NodeNetworkRepository;
    use crate::storage::registry_credential_repository::RegistryCredentialRepository;

    fn lines(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn merge_new_log_lines_with_nothing_captured_yet_treats_the_whole_batch_as_new() {
        let live = lines(&["a", "b"]);
        assert_eq!(merge_new_log_lines(&[], live.clone()), live);
    }

    #[test]
    fn merge_new_log_lines_returns_only_what_comes_after_the_overlap() {
        assert_eq!(merge_new_log_lines(&lines(&["a", "b"]), lines(&["a", "b", "c", "d"])), lines(&["c", "d"]));
    }

    #[test]
    fn merge_new_log_lines_returns_nothing_when_the_live_window_is_entirely_already_captured() {
        assert!(merge_new_log_lines(&lines(&["a", "b"]), lines(&["a", "b"])).is_empty());
    }

    /// The regression test for the log-loss finding. With a single-line
    /// anchor and a *rightmost* match, the previous implementation returned
    /// only `["done"]` here - silently discarding `ok`, which had genuinely
    /// not been captured yet. Application logs repeat constantly ("Can't
    /// keep up!", reconnect notices), so this was routine, not exotic.
    #[test]
    fn merge_new_log_lines_does_not_lose_lines_around_a_repeated_line() {
        let previous = lines(&["start", "retry"]);
        let live = lines(&["start", "retry", "ok", "retry", "done"]);
        assert_eq!(merge_new_log_lines(&previous, live), lines(&["ok", "retry", "done"]));
    }

    /// The longest overlap wins: a shorter suffix match can be coincidence,
    /// the longest is where the two windows genuinely line up.
    #[test]
    fn merge_new_log_lines_prefers_the_longest_overlap() {
        let previous = lines(&["x", "a", "b", "c"]);
        let live = lines(&["a", "b", "c", "d"]);
        assert_eq!(merge_new_log_lines(&previous, live), lines(&["d"]));
    }

    #[test]
    fn merge_new_log_lines_treats_no_overlap_as_a_fresh_container_and_keeps_everything() {
        // Nothing in common - a Recreate gave the container a brand new
        // buffer. Everything live is new, not dropped.
        let live = lines(&["fresh start", "line two"]);
        assert_eq!(merge_new_log_lines(&lines(&["something from the old container"]), live.clone()), live);
    }

    /// The live window can start *before* what was captured (a larger
    /// `--tail` than last time), in which case the overlap is bounded by
    /// the captured side rather than the live one.
    #[test]
    fn merge_new_log_lines_handles_a_live_window_longer_than_the_captured_tail() {
        assert_eq!(merge_new_log_lines(&lines(&["c"]), lines(&["c", "d", "e"])), lines(&["d", "e"]));
    }

    #[test]
    fn registry_host_treats_a_bare_image_as_docker_hub() {
        assert_eq!(registry_host("nginx:latest"), "docker.io");
        assert_eq!(registry_host("alpine"), "docker.io");
    }

    #[test]
    fn registry_host_treats_a_user_org_path_with_no_dot_or_colon_as_docker_hub() {
        assert_eq!(registry_host("someuser/someimage:tag"), "docker.io");
    }

    #[test]
    fn registry_host_recognizes_a_domain_looking_first_segment_as_the_registry() {
        assert_eq!(registry_host("ghcr.io/someuser/someimage:tag"), "ghcr.io");
        assert_eq!(registry_host("my.private.registry/team/app:tag"), "my.private.registry");
    }

    #[test]
    fn registry_host_recognizes_a_localhost_or_port_first_segment_as_the_registry() {
        assert_eq!(registry_host("localhost:5000/app:tag"), "localhost:5000");
        assert_eq!(registry_host("localhost/app:tag"), "localhost");
    }

    #[test]
    fn registry_host_recognizes_an_explicit_docker_io_prefix_too() {
        assert_eq!(registry_host("docker.io/library/nginx:latest"), "docker.io");
    }

    /// A real `ApplicationRepository` + `ServerRepository` against a fresh
    /// temp SQLite file, a real `LocalProcessManager`, and the real
    /// built-in `BlueprintRegistry` - the same components `lib.rs` wires
    /// together for the actual app, exercised end to end (create -> start
    /// -> status -> stop -> delete) rather than only unit-tested in
    /// isolation. `ServerRepository`/`SshSessionManager` are unused by a
    /// Local application's own lifecycle but still required by every
    /// function's signature, matching production's own shape.
    #[allow(clippy::type_complexity)]
    fn temp_setup() -> (
        ApplicationRepository,
        ServerRepository,
        NodeNetworkRepository,
        SshSessionManager,
        Arc<LocalProcessManager>,
        BlueprintRegistry,
        FirewallRuleRepository,
        RegistryCredentialRepository,
        LogCaptureStore,
        crate::storage::database_repository::DatabaseRepository,
        crate::storage::dns_repository::DnsRepository,
    ) {
        let path = std::env::temp_dir().join(format!("vibessh-app-service-test-{}.sqlite3", Uuid::new_v4()));
        let app_repo = ApplicationRepository::open(&path).unwrap();
        let server_repo = ServerRepository::open(&path).unwrap();
        let network_repo = NodeNetworkRepository::open(&path).unwrap();
        let firewall_rule_repo = FirewallRuleRepository::open(&path).unwrap();
        let registry_repo = RegistryCredentialRepository::open(&path).unwrap();
        let log_capture = LogCaptureStore::new(std::env::temp_dir().join(format!("vibessh-app-service-test-logs-{}", Uuid::new_v4()))).unwrap();
        // Same file as every other repository above - `delete_application`'s
        // teardown relies on `ON DELETE CASCADE` reaching rows these two own.
        let db_repo = crate::storage::database_repository::DatabaseRepository::open(&path).unwrap();
        let dns_repo = crate::storage::dns_repository::DnsRepository::open(&path).unwrap();
        (
            app_repo,
            server_repo,
            network_repo,
            SshSessionManager::new(),
            Arc::new(LocalProcessManager::new()),
            BlueprintRegistry::with_builtins(),
            firewall_rule_repo,
            registry_repo,
            log_capture,
            db_repo,
            dns_repo,
        )
    }

    /// `delete_application` with the wiring every test needs and the
    /// options a user-initiated delete uses. Keeps the teardown's twelve
    /// real parameters out of each individual test.
    #[allow(clippy::too_many_arguments)]
    async fn delete_for_test(
        app_repo: &ApplicationRepository,
        server_repo: &ServerRepository,
        db_repo: &crate::storage::database_repository::DatabaseRepository,
        network_repo: &NodeNetworkRepository,
        firewall_rule_repo: &FirewallRuleRepository,
        dns_repo: &crate::storage::dns_repository::DnsRepository,
        sessions: &SshSessionManager,
        local_process_manager: &Arc<LocalProcessManager>,
        log_capture: &LogCaptureStore,
        id: Uuid,
    ) -> AppResult<ApplicationTeardownReport> {
        delete_application(
            app_repo,
            server_repo,
            db_repo,
            network_repo,
            firewall_rule_repo,
            dns_repo,
            sessions,
            local_process_manager,
            log_capture,
            ".vibe",
            id,
            ApplicationDeleteOptions { drop_databases: true, remove_files: false },
        )
        .await
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
            connect_to_application_id: None,
        }
    }

    #[tokio::test]
    async fn full_lifecycle_create_start_status_stop_delete() {
        let (app_repo, server_repo, network_repo, sessions, local_process_manager, registry, firewall_rule_repo, registry_credential_repo, log_capture, db_repo, dns_repo) =
            temp_setup();

        let detail = create_application(&app_repo, &registry, &server_repo, &sessions, std::path::Path::new(""), sleep_command_input()).await.unwrap();
        assert_eq!(detail.application.status, ApplicationStatus::Unknown);
        assert_eq!(detail.runtime_config["command"], serde_json::json!(if cfg!(windows) { "cmd" } else { "sh" }));

        let status = start_application(&app_repo, &server_repo, &sessions, &registry_credential_repo, &local_process_manager, detail.application.id).await.unwrap();
        assert_eq!(status, ApplicationStatus::Running);

        let refreshed = get_application(&app_repo, detail.application.id).unwrap();
        assert_eq!(refreshed.application.status, ApplicationStatus::Running);

        // The stdout pump runs on its own background task - poll rather
        // than assume it's already flushed by the time start() returned.
        let mut saw_output = false;
        for _ in 0..30 {
            let lines = application_logs(&app_repo, &server_repo, &sessions, &local_process_manager, &log_capture, detail.application.id, 10).await.unwrap();
            if lines.iter().any(|line| line.contains("hello-from-application-service")) {
                saw_output = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(saw_output, "expected application_logs to eventually show the process's stdout");

        let status = stop_application(&app_repo, &server_repo, &sessions, &local_process_manager, detail.application.id, true).await.unwrap();
        assert_eq!(status, ApplicationStatus::Stopped);

        let report = delete_for_test(
            &app_repo,
            &server_repo,
            &db_repo,
            &network_repo,
            &firewall_rule_repo,
            &dns_repo,
            &sessions,
            &local_process_manager,
            &log_capture,
            detail.application.id,
        )
        .await
        .unwrap();
        // A Local application has no Node, so there is nothing that could
        // have half-failed - a clean teardown must report no warnings.
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert!(get_application(&app_repo, detail.application.id).is_err());
    }

    #[tokio::test]
    async fn create_rejects_an_unknown_blueprint() {
        let (app_repo, server_repo, _network_repo, sessions, _local_process_manager, registry, ..) = temp_setup();
        let mut input = sleep_command_input();
        input.blueprint_id = "does-not-exist".to_string();
        assert!(create_application(&app_repo, &registry, &server_repo, &sessions, std::path::Path::new(""), input).await.is_err());
    }

    #[tokio::test]
    async fn create_rejects_a_runtime_type_the_blueprint_doesnt_support() {
        let (app_repo, server_repo, _network_repo, sessions, _local_process_manager, registry, ..) = temp_setup();
        let mut input = sleep_command_input();
        input.runtime_type = RuntimeType::Docker;
        assert!(create_application(&app_repo, &registry, &server_repo, &sessions, std::path::Path::new(""), input).await.is_err());
    }

    #[tokio::test]
    async fn create_rejects_a_blank_name() {
        let (app_repo, server_repo, _network_repo, sessions, _local_process_manager, registry, ..) = temp_setup();
        let mut input = sleep_command_input();
        input.name = "   ".to_string();
        assert!(create_application(&app_repo, &registry, &server_repo, &sessions, std::path::Path::new(""), input).await.is_err());
    }

    /// An Application on a Node has `sudo chown -R <its own account>` run
    /// over its `working_directory` on every start
    /// (`runtime::docker::ensure_working_directory_owned_by_dedicated_user`).
    /// Naming a shared system directory there doesn't fail, it hands the
    /// host's filesystem to an unprivileged account with no way back - so
    /// these have to be refused before anything touches the Node at all,
    /// which is also why this test needs no live connection to pass.
    #[tokio::test]
    async fn create_refuses_a_system_directory_for_an_application_on_a_node() {
        let (app_repo, server_repo, _network_repo, sessions, _local_process_manager, registry, ..) = temp_setup();
        let server = server_repo
            .create(&crate::models::ServerInput {
                name: "Node".to_string(),
                host: "203.0.113.10".to_string(),
                ssh_port: 22,
                username: "root".to_string(),
                authentication_type: crate::models::AuthenticationType::Password,
                private_key_path: None,
                group_id: None,
                password: Some("unused - validation rejects before connecting".to_string()),
                key_passphrase: None,
            })
            .unwrap();
        for hostile in ["/", "/etc", "/home", "/usr", "/var", "/root", "/srv", "srv/app", "/srv/../etc"] {
            let mut input = sleep_command_input();
            input.server_id = Some(server.id);
            input.runtime_type = RuntimeType::RemoteProcess;
            input.working_directory = hostile.to_string();
            let result = create_application(&app_repo, &registry, &server_repo, &sessions, std::path::Path::new(""), input).await;
            assert!(result.is_err(), "should have refused {hostile:?}");
        }
    }

    fn port_input(visibility: PortVisibility, bind_address: &str) -> PortInput {
        PortInput {
            name: "game".to_string(),
            protocol: crate::models::PortProtocol::Tcp,
            bind_address: bind_address.to_string(),
            internal_port: 25565,
            external_port: Some(25565),
            visibility,
            required: false,
        }
    }

    /// The regression test for the finding that "Vibe Network only" ports
    /// were publicly reachable. `VibeNetwork` must never resolve to
    /// `0.0.0.0` - a UFW source-CIDR rule cannot restrict a published
    /// Docker port, because Docker's own iptables rules are evaluated
    /// first. Binding the mesh address is what makes the kernel enforce it.
    #[tokio::test]
    async fn a_vibe_network_port_binds_the_mesh_address_never_all_interfaces() {
        let (app_repo, server_repo, network_repo, ..) = temp_setup();
        let _ = &app_repo;
        let server = server_repo
            .create(&crate::models::ServerInput {
                name: "Node".to_string(),
                host: "203.0.113.10".to_string(),
                ssh_port: 22,
                username: "root".to_string(),
                authentication_type: crate::models::AuthenticationType::Password,
                private_key_path: None,
                group_id: None,
                password: Some("unused".to_string()),
                key_passphrase: None,
            })
            .unwrap();
        let member = network_repo.join(server.id, "K4hV1cB0mQ2sT7nZ9xY3lJ6pR8dW5gA0fE1uI2oC3vM=").unwrap();

        let resolved = resolve_bind_address(&network_repo, Some(server.id), &port_input(PortVisibility::VibeNetwork, "")).unwrap();
        assert_eq!(resolved, member.wireguard_ip);
        assert_ne!(resolved, "0.0.0.0");
    }

    /// ...and when the Node has no mesh address to bind, that must be a
    /// loud error rather than a silent fallback to `0.0.0.0`.
    #[tokio::test]
    async fn a_vibe_network_port_is_refused_when_the_node_isnt_on_the_mesh() {
        let (_app_repo, server_repo, network_repo, ..) = temp_setup();
        let server = server_repo
            .create(&crate::models::ServerInput {
                name: "Node".to_string(),
                host: "203.0.113.11".to_string(),
                ssh_port: 22,
                username: "root".to_string(),
                authentication_type: crate::models::AuthenticationType::Password,
                private_key_path: None,
                group_id: None,
                password: Some("unused".to_string()),
                key_passphrase: None,
            })
            .unwrap();

        let result = resolve_bind_address(&network_repo, Some(server.id), &port_input(PortVisibility::VibeNetwork, ""));
        assert!(result.is_err());
        // A local Application has no mesh address at all.
        assert!(resolve_bind_address(&network_repo, None, &port_input(PortVisibility::VibeNetwork, "")).is_err());
    }

    #[tokio::test]
    async fn the_other_visibilities_are_unchanged() {
        let (_app_repo, _server_repo, network_repo, ..) = temp_setup();
        assert_eq!(resolve_bind_address(&network_repo, None, &port_input(PortVisibility::Public, "")).unwrap(), "0.0.0.0");
        assert_eq!(resolve_bind_address(&network_repo, None, &port_input(PortVisibility::Localhost, "")).unwrap(), "127.0.0.1");
        assert_eq!(
            resolve_bind_address(&network_repo, None, &port_input(PortVisibility::Custom, "10.1.2.3")).unwrap(),
            "10.1.2.3"
        );
    }

    /// The mirror of the test above: the same check must not reject a
    /// *local* Application, whose `working_directory` is a native path on
    /// the operator's own machine (`C:\Users\...` on Windows) and which
    /// never goes near `sudo` or a remote host.
    #[tokio::test]
    async fn create_still_accepts_a_native_local_working_directory() {
        let (app_repo, server_repo, _network_repo, sessions, _local_process_manager, registry, ..) = temp_setup();
        let input = sleep_command_input();
        assert!(input.server_id.is_none());
        assert!(create_application(&app_repo, &registry, &server_repo, &sessions, std::path::Path::new(""), input).await.is_ok());
    }

    #[tokio::test]
    async fn create_creates_a_missing_local_working_directory() {
        let (app_repo, server_repo, _network_repo, sessions, _local_process_manager, registry, ..) = temp_setup();
        let mut input = sleep_command_input();
        let fresh_dir = std::env::temp_dir().join(format!("vibessh-app-service-workdir-{}", Uuid::new_v4()));
        assert!(!fresh_dir.exists());
        input.working_directory = fresh_dir.to_string_lossy().into_owned();

        let detail = create_application(&app_repo, &registry, &server_repo, &sessions, std::path::Path::new(""), input).await.unwrap();

        assert!(fresh_dir.is_dir());
        assert_eq!(detail.application.working_directory, fresh_dir.to_string_lossy());
        std::fs::remove_dir_all(&fresh_dir).ok();
    }

    #[tokio::test]
    async fn port_crud_add_update_remove_round_trips_through_the_service_layer() {
        let (app_repo, server_repo, network_repo, sessions, _local_process_manager, registry, firewall_rule_repo, ..) = temp_setup();
        let detail = create_application(&app_repo, &registry, &server_repo, &sessions, std::path::Path::new(""), sleep_command_input()).await.unwrap();
        let application_id = detail.application.id;

        assert!(list_application_ports(&app_repo, application_id).unwrap().is_empty());

        let input = crate::models::PortInput {
            name: "game".to_string(),
            protocol: crate::models::PortProtocol::Tcp,
            bind_address: "0.0.0.0".to_string(),
            internal_port: 25565,
            external_port: None,
            visibility: crate::models::PortVisibility::Public,
            required: false,
        };
        let added = add_application_port(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, application_id, &input).await.unwrap();
        assert_eq!(added.internal_port, 25565);
        assert_eq!(list_application_ports(&app_repo, application_id).unwrap().len(), 1);

        // Adding the exact same internal_port/bind_address/protocol again
        // is a real collision, not a silent duplicate - the service layer
        // must surface the repository's own collision error, not swallow it.
        assert!(add_application_port(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, application_id, &input).await.is_err());

        let updated_input = crate::models::PortInput { internal_port: 25566, ..input };
        let updated = update_application_port(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, application_id, added.id, &updated_input).await.unwrap();
        assert_eq!(updated.internal_port, 25566);

        remove_application_port(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, application_id, added.id).await.unwrap();
        assert!(list_application_ports(&app_repo, application_id).unwrap().is_empty());
    }

    /// The design doc's own "check other Applications, other Exit Ports"
    /// collision requirement, exercised end to end through the service
    /// layer that actually enforces it (`check_external_port_available`) -
    /// the repository-level check this reuses only ever looked at ports on
    /// the *same* Application (see `add_port`'s own doc comment), so a
    /// second, unrelated Application publishing the exact same host port
    /// used to be silently allowed. The Node's host is unreachable
    /// (`203.0.113.10` is a TEST-NET-3 address, RFC 5737) - the live `ss`
    /// probe half of the check is expected to fail to connect and get
    /// skipped, proving the DB-level half alone is what's catching this,
    /// not a lucky live probe result.
    #[tokio::test]
    async fn add_application_port_rejects_an_external_port_already_published_by_a_different_application_on_the_same_node() {
        let (app_repo, server_repo, network_repo, sessions, _local_process_manager, _registry, firewall_rule_repo, ..) = temp_setup();
        let server = server_repo
            .create(&crate::models::ServerInput {
                name: "Collision Test Node".into(),
                host: "203.0.113.10".into(),
                ssh_port: 22,
                username: "root".into(),
                authentication_type: crate::models::AuthenticationType::Password,
                private_key_path: None,
                group_id: None,
                password: Some("x".into()),
                key_passphrase: None,
            })
            .unwrap();

        fn docker_app_input(server_id: Uuid, name: &str) -> CreateApplicationInput {
            CreateApplicationInput {
                server_id: Some(server_id),
                name: name.to_string(),
                description: None,
                blueprint_id: "generic-docker".to_string(),
                blueprint_version: 1,
                runtime_type: RuntimeType::Docker,
                working_directory: "/srv/app".to_string(),
                environment: vec![],
                ports: vec![],
                runtime_config: serde_json::json!({}),
                metadata: serde_json::json!({}),
            }
        }
        let app_a = app_repo.create(&docker_app_input(server.id, "App A")).unwrap();
        let app_b = app_repo.create(&docker_app_input(server.id, "App B")).unwrap();

        let published_by_a = crate::models::PortInput {
            name: "game".to_string(),
            protocol: crate::models::PortProtocol::Tcp,
            bind_address: "0.0.0.0".to_string(),
            internal_port: 25565,
            external_port: Some(25565),
            visibility: crate::models::PortVisibility::Public,
            required: false,
        };
        add_application_port(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, app_a.application.id, &published_by_a).await.unwrap();

        let colliding_from_b = crate::models::PortInput { internal_port: 25566, ..published_by_a.clone() };
        let err = add_application_port(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, app_b.application.id, &colliding_from_b).await.unwrap_err();
        // Its own code, not a generic invalid-input: the UI renders a
        // translated sentence naming the port and what holds it, rather
        // than echoing a Rust string.
        assert!(
            matches!(err, AppError::PortInUse { port: 25565, protocol: "tcp", owner: Some(_) }),
            "{err:?}"
        );
        assert!(list_application_ports(&app_repo, app_b.application.id).unwrap().is_empty(), "the colliding port must never have been saved");

        // A different protocol on the same port number is not a collision.
        let different_protocol = crate::models::PortInput { protocol: crate::models::PortProtocol::Udp, ..colliding_from_b.clone() };
        assert!(add_application_port(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, app_b.application.id, &different_protocol).await.is_ok());

        // Re-saving App A's own port unchanged (e.g. editing its name) must
        // not collide against itself.
        let app_a_port = list_application_ports(&app_repo, app_a.application.id).unwrap().remove(0);
        let renamed = crate::models::PortInput { name: "renamed".to_string(), ..published_by_a };
        assert!(update_application_port(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, app_a.application.id, app_a_port.id, &renamed).await.is_ok());
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

    /// Reproduces a real Application from before Paper/Velocity went
    /// Docker-only (Etap M1): the row still has `runtime_type =
    /// RemoteProcess` and its own already-working `runtime_config`
    /// (untouched by this test, and by `update_application_config` itself -
    /// see that function's own doc comment), but the blueprint that created
    /// it no longer lists RemoteProcess as supported.
    #[tokio::test]
    async fn update_application_config_rejects_an_application_whose_runtime_type_the_blueprint_no_longer_supports() {
        let (app_repo, server_repo, _network_repo, sessions, _local_process_manager, registry, ..) = temp_setup();
        let legacy_velocity = app_repo
            .create(&CreateApplicationInput {
                server_id: None,
                name: "Legacy Velocity".to_string(),
                description: None,
                blueprint_id: "velocity".to_string(),
                blueprint_version: 1,
                runtime_type: RuntimeType::RemoteProcess,
                working_directory: std::env::temp_dir().to_string_lossy().into_owned(),
                environment: vec![],
                ports: vec![],
                runtime_config: serde_json::json!({ "command": "java", "args": ["-jar", "velocity-3.4.0-566.jar"] }),
                metadata: serde_json::json!({}),
            })
            .unwrap();

        let err = update_application_config(&app_repo, &registry, &server_repo, &sessions, std::path::Path::new(""), legacy_velocity.application.id, serde_json::json!({ "javaVersion": "25" }))
            .await
            .unwrap_err();

        assert!(matches!(err, AppError::InvalidInput(_)));
        // The Application's own config must be completely untouched - this
        // rejects before ever calling `render_runtime_config`, not after.
        let reloaded = get_application(&app_repo, legacy_velocity.application.id).unwrap();
        assert_eq!(reloaded.runtime_config, serde_json::json!({ "command": "java", "args": ["-jar", "velocity-3.4.0-566.jar"] }));
    }

    /// The real-infra regression test for the bug this session actually
    /// found: changing Velocity's own version field used to silently keep
    /// running the jar downloaded at creation, because `update_application_config`
    /// never re-ran `provision()` - see that function's own doc comment for
    /// the full explanation. A real network call against papermc.io, same
    /// "skip if unreachable" pattern this crate's other provision tests
    /// already use.
    #[tokio::test]
    async fn update_application_config_re_provisions_so_a_changed_version_downloads_a_different_jar() {
        let (app_repo, server_repo, _network_repo, sessions, _local_process_manager, registry, ..) = temp_setup();
        let working_directory = std::env::temp_dir().join(format!("vibessh-config-reprovision-test-{}", uuid::Uuid::new_v4()));

        let input = CreateApplicationFromBlueprintInput {
            server_id: None,
            name: "Version Change Test".to_string(),
            description: None,
            blueprint_id: "velocity".to_string(),
            runtime_type: RuntimeType::Docker,
            working_directory: working_directory.to_string_lossy().into_owned(),
            environment: vec![],
            blueprint_inputs: serde_json::json!({ "velocityVersion": "3.1.1" }),
            connect_to_application_id: None,
        };

        let created = match create_application(&app_repo, &registry, &server_repo, &sessions, std::path::Path::new(""), input).await {
            Ok(created) => created,
            Err(err) => {
                eprintln!("skipping: papermc.io unreachable from this environment ({err:?})");
                return;
            }
        };
        let jar_from = |config: &serde_json::Value| {
            config["command"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(serde_json::Value::as_str)
                .find(|arg| arg.ends_with(".jar"))
                .unwrap()
                .to_string()
        };
        let first_jar = jar_from(&created.runtime_config);
        assert!(first_jar.contains("3.1.1"), "expected the 3.1.1 jar, got {first_jar}");

        let updated = update_application_config(
            &app_repo,
            &registry,
            &server_repo,
            &sessions,
            std::path::Path::new(""),
            created.application.id,
            serde_json::json!({ "velocityVersion": "3.4.0" }),
        )
        .await
        .unwrap();

        let second_jar = jar_from(&updated.runtime_config).to_string();
        assert!(second_jar.contains("3.4.0"), "expected the 3.4.0 jar after changing the version, got {second_jar}");
        assert_ne!(first_jar, second_jar, "changing the version must actually change the downloaded jar");
        assert!(working_directory.join(&second_jar).is_file(), "the newly downloaded jar should exist in the working directory");

        tokio::fs::remove_dir_all(&working_directory).await.ok();
    }

    #[tokio::test]
    async fn recreate_application_rejects_a_non_docker_runtime_type() {
        let (app_repo, server_repo, _network_repo, sessions, local_process_manager, _registry, _firewall_rule_repo, registry_credential_repo, ..) = temp_setup();
        let local = create_raw(&app_repo, RuntimeType::LocalProcess, serde_json::json!({ "command": "sh", "args": [] }));

        let err = recreate_application(&app_repo, &_registry, &server_repo, &sessions, &registry_credential_repo, &local_process_manager, local.application.id).await.unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }

    #[test]
    fn set_application_resource_limits_rejects_a_runtime_type_that_cant_enforce_them() {
        let (app_repo, _server_repo, _network_repo, _sessions, _local_process_manager, _registry, ..) = temp_setup();
        let local = create_raw(&app_repo, RuntimeType::LocalProcess, serde_json::json!({ "command": "sh", "args": [] }));

        let result = set_application_resource_limits(&app_repo, local.application.id, SetResourceLimitsInput { memory_limit_mb: Some(512), cpu_limit_cores: None });
        assert!(result.is_err());
    }

    #[test]
    fn set_application_resource_limits_patches_and_clears_the_docker_runtime_config() {
        let (app_repo, _server_repo, _network_repo, _sessions, _local_process_manager, _registry, ..) = temp_setup();
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
        let (app_repo, _server_repo, _network_repo, _sessions, _local_process_manager, _registry, ..) = temp_setup();
        let systemd = create_raw(&app_repo, RuntimeType::Systemd, serde_json::json!({ "command": "/usr/bin/java", "args": [] }));

        let result = set_application_resource_limits(&app_repo, systemd.application.id, SetResourceLimitsInput { memory_limit_mb: Some(0), cpu_limit_cores: None });
        assert!(result.is_err());
    }

    #[test]
    fn set_application_image_patches_the_docker_runtime_config_and_leaves_the_rest_untouched() {
        let (app_repo, _server_repo, _network_repo, _sessions, _local_process_manager, _registry, ..) = temp_setup();
        let docker = create_raw(&app_repo, RuntimeType::Docker, serde_json::json!({ "image": "alpine:latest", "command": ["sleep", "999"] }));

        let updated = set_application_image(&app_repo, docker.application.id, "  eclipse-temurin:25-jre-alpine  ".to_string()).unwrap();
        assert_eq!(updated.runtime_config["image"], serde_json::json!("eclipse-temurin:25-jre-alpine"));
        assert_eq!(updated.runtime_config["command"], serde_json::json!(["sleep", "999"]));
    }

    #[test]
    fn set_application_image_rejects_a_non_docker_runtime_type() {
        let (app_repo, _server_repo, _network_repo, _sessions, _local_process_manager, _registry, ..) = temp_setup();
        let local = create_raw(&app_repo, RuntimeType::LocalProcess, serde_json::json!({ "command": "sh", "args": [] }));

        assert!(set_application_image(&app_repo, local.application.id, "alpine:latest".to_string()).is_err());
    }

    #[test]
    fn set_application_image_rejects_a_blank_or_newline_containing_image() {
        let (app_repo, _server_repo, _network_repo, _sessions, _local_process_manager, _registry, ..) = temp_setup();
        let docker = create_raw(&app_repo, RuntimeType::Docker, serde_json::json!({ "image": "alpine:latest", "command": [] }));

        assert!(set_application_image(&app_repo, docker.application.id, "   ".to_string()).is_err());
        assert!(set_application_image(&app_repo, docker.application.id, "alpine:latest\nrm -rf /".to_string()).is_err());
    }

    #[tokio::test]
    async fn pull_application_image_rejects_a_non_docker_runtime_type() {
        let (app_repo, server_repo, _network_repo, sessions, _local_process_manager, _registry, _firewall_rule_repo, registry_credential_repo, ..) = temp_setup();
        let local = create_raw(&app_repo, RuntimeType::LocalProcess, serde_json::json!({ "command": "sh", "args": [] }));

        let err = pull_application_image(&app_repo, &server_repo, &sessions, &registry_credential_repo, local.application.id).await.unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn pull_application_image_rejects_a_docker_application_with_no_image_configured() {
        let (app_repo, server_repo, _network_repo, sessions, _local_process_manager, _registry, _firewall_rule_repo, registry_credential_repo, ..) = temp_setup();
        let docker = create_raw(&app_repo, RuntimeType::Docker, serde_json::json!({ "command": [] }));

        let err = pull_application_image(&app_repo, &server_repo, &sessions, &registry_credential_repo, docker.application.id).await.unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }

    /// End-to-end through the real OS keyring (guarded by the same
    /// process-wide lock every other keyring-touching test in this crate
    /// takes - see `storage::credentials::KEYRING_TEST_LOCK`'s own doc
    /// comment), covering the whole life of a secret environment variable:
    /// never plaintext on a normal read, resolved back only for an actual
    /// runtime, "leave blank to keep" on edit, and cleaned up both when
    /// removed and when the Application itself is deleted.
    // The guard's whole job is to serialize real OS-keyring access across
    // tests, so it must be held for the duration of the awaits it is
    // guarding. Safe here because `#[tokio::test]` runs this future on a
    // current-thread runtime - a `std::sync::MutexGuard` is not `Send`, so
    // the compiler would reject it on a multi-threaded one.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn secret_environment_variables_never_leak_plaintext_and_round_trip_through_the_keyring() {
        let _guard = crate::storage::credentials::KEYRING_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (app_repo, server_repo, network_repo, sessions, local_process_manager, registry, firewall_rule_repo, _registry_credential_repo, log_capture, db_repo, dns_repo) =
            temp_setup();

        let mut input = sleep_command_input();
        input.environment = vec![
            EnvironmentVariable { key: "PLAIN".into(), value: "visible".into(), is_secret: false },
            EnvironmentVariable { key: "DB_PASSWORD".into(), value: "hunter2".into(), is_secret: true },
        ];
        let created = create_application(&app_repo, &registry, &server_repo, &sessions, std::path::Path::new(""), input).await.unwrap();
        let id = created.application.id;
        struct Cleanup(Uuid);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = crate::storage::credentials::delete_environment_secret(self.0, "DB_PASSWORD");
            }
        }
        let _cleanup = Cleanup(id);

        // A plain read - what every Tauri command hands to the frontend -
        // never carries the secret's real value, only that it is one.
        let redacted = get_application(&app_repo, id).unwrap();
        let secret_row = redacted.environment.iter().find(|e| e.key == "DB_PASSWORD").unwrap();
        assert!(secret_row.is_secret);
        assert_eq!(secret_row.value, "");
        assert_eq!(redacted.environment.iter().find(|e| e.key == "PLAIN").unwrap().value, "visible");

        // An actual runtime (about to start the process) resolves the real
        // value back in.
        let (runtime_detail, _connection, _runtime) = load_runtime(&app_repo, &server_repo, &sessions, &local_process_manager, id).await.unwrap();
        assert_eq!(runtime_detail.environment.iter().find(|e| e.key == "DB_PASSWORD").unwrap().value, "hunter2");

        // Editing another field without retyping the secret (the frontend
        // never has the real value to resend) preserves it.
        set_application_environment(
            &app_repo,
            id,
            vec![
                EnvironmentVariable { key: "PLAIN".into(), value: "still-visible".into(), is_secret: false },
                EnvironmentVariable { key: "DB_PASSWORD".into(), value: "".into(), is_secret: true },
            ],
        )
        .unwrap();
        let (runtime_detail, _connection, _runtime) = load_runtime(&app_repo, &server_repo, &sessions, &local_process_manager, id).await.unwrap();
        assert_eq!(runtime_detail.environment.iter().find(|e| e.key == "DB_PASSWORD").unwrap().value, "hunter2");

        // Removing the key deletes its keyring entry rather than leaving it
        // orphaned forever.
        set_application_environment(&app_repo, id, vec![EnvironmentVariable { key: "PLAIN".into(), value: "still-visible".into(), is_secret: false }])
            .unwrap();
        assert_eq!(crate::storage::credentials::load_environment_secret(id, "DB_PASSWORD").unwrap(), None);

        // Deleting the Application cleans up any secret still attached to it.
        set_application_environment(&app_repo, id, vec![EnvironmentVariable { key: "DB_PASSWORD".into(), value: "again".into(), is_secret: true }]).unwrap();
        delete_for_test(&app_repo, &server_repo, &db_repo, &network_repo, &firewall_rule_repo, &dns_repo, &sessions, &local_process_manager, &log_capture, id)
            .await
            .unwrap();
        assert_eq!(crate::storage::credentials::load_environment_secret(id, "DB_PASSWORD").unwrap(), None);
    }
    /// FIX_PLAN C.3, the slice of it that is worth having.
    ///
    /// The plan asked for every service method against eleven outcome states.
    /// Most of that matrix cannot be written honestly without mocks for SSH,
    /// Docker and MySQL that this codebase does not have, and a mock's
    /// verdict is a statement about the mock. What *can* be checked, and is
    /// where the audit actually found bugs, is the family of outcomes around
    /// **partial failure**: an operation that half-succeeded and has to say
    /// so. `delete_application` is the sharpest example - it used to delete a
    /// row and nothing else while reporting success (S-007) - so the matrix
    /// is written against it, plus the migration rollback added in F.4 that
    /// had never actually been run.
    ///
    /// The unreachable Node is `127.0.0.1` on a port nothing listens to.
    /// That fails with a connection refusal immediately, rather than the
    /// multi-second timeout a routable-but-dead address would cost every run.
    mod outcome_states {
        use super::*;

        fn unreachable_node(server_repo: &ServerRepository, name: &str) -> Uuid {
            server_repo
                .create(&crate::models::ServerInput {
                    name: name.into(),
                    // Nothing listens here, so `connect` is refused at once.
                    host: "127.0.0.1".into(),
                    ssh_port: 1,
                    username: "root".into(),
                    authentication_type: crate::models::AuthenticationType::Password,
                    private_key_path: None,
                    group_id: None,
                    password: Some("x".into()),
                    key_passphrase: None,
                })
                .unwrap()
                .id
        }

        fn docker_application(app_repo: &ApplicationRepository, server_id: Option<Uuid>, name: &str) -> Uuid {
            app_repo
                .create(&CreateApplicationInput {
                    server_id,
                    name: name.to_string(),
                    description: None,
                    blueprint_id: "generic-docker".to_string(),
                    blueprint_version: 1,
                    runtime_type: if server_id.is_some() { RuntimeType::Docker } else { RuntimeType::LocalProcess },
                    working_directory: std::env::temp_dir().to_string_lossy().into_owned(),
                    environment: vec![],
                    ports: vec![],
                    runtime_config: serde_json::json!({ "command": "sh", "args": [] }),
                    metadata: serde_json::json!({}),
                })
                .unwrap()
                .application
                .id
        }

        /// NOT FOUND. An id that never existed is a `NotFound`, not a report
        /// full of warnings about an Application nobody asked about.
        #[tokio::test]
        async fn deleting_something_that_does_not_exist_is_not_found() {
            let (app_repo, server_repo, network_repo, sessions, local_process_manager, _registry, firewall_rule_repo, _rc, log_capture, db_repo, dns_repo) =
                temp_setup();
            let err = delete_for_test(
                &app_repo, &server_repo, &db_repo, &network_repo, &firewall_rule_repo, &dns_repo, &sessions, &local_process_manager, &log_capture,
                Uuid::new_v4(),
            )
            .await
            .unwrap_err();
            assert!(matches!(err, AppError::NotFound(_)), "{err:?}");
        }

        /// SUCCESS. A Local Application has no Node, so every Node-side step
        /// is vacuously complete - and the report has to say *complete*
        /// rather than leaving a caller unable to tell "nothing to do" from
        /// "did not try".
        #[tokio::test]
        async fn deleting_a_local_application_reports_a_clean_teardown() {
            let (app_repo, server_repo, network_repo, sessions, local_process_manager, _registry, firewall_rule_repo, _rc, log_capture, db_repo, dns_repo) =
                temp_setup();
            let id = docker_application(&app_repo, None, "Local App");

            let report = delete_for_test(
                &app_repo, &server_repo, &db_repo, &network_repo, &firewall_rule_repo, &dns_repo, &sessions, &local_process_manager, &log_capture, id,
            )
            .await
            .unwrap();

            assert!(report.warnings.is_empty(), "unexpected warnings: {:?}", report.warnings);
            assert!(report.firewall_synced && report.dns_synced, "{report:?}");
            assert!(app_repo.get(id).unwrap().is_none(), "the row should be gone");
        }

        /// CONNECTION LOST, which is the same thing as PARTIAL FAILURE here.
        ///
        /// The row still goes - leaving it would strand the Application in
        /// the UI with no way to retry - but every step that could not run
        /// has to appear in `warnings`, and none of the booleans may claim
        /// something happened. This is exactly the report S-007 did not
        /// produce.
        #[tokio::test]
        async fn deleting_against_an_unreachable_node_reports_every_step_it_could_not_do() {
            let (app_repo, server_repo, network_repo, sessions, local_process_manager, _registry, firewall_rule_repo, _rc, log_capture, db_repo, dns_repo) =
                temp_setup();
            let server_id = unreachable_node(&server_repo, "Dead Node");
            let id = docker_application(&app_repo, Some(server_id), "Stranded App");

            let report = delete_for_test(
                &app_repo, &server_repo, &db_repo, &network_repo, &firewall_rule_repo, &dns_repo, &sessions, &local_process_manager, &log_capture, id,
            )
            .await
            .unwrap();

            assert!(app_repo.get(id).unwrap().is_none(), "the row must still be removed");
            assert!(!report.container_removed, "claimed to have removed a container on an unreachable node");
            assert!(!report.warnings.is_empty(), "an unreachable node must produce warnings");
            // The warnings are shown to a human, so they have to name the
            // thing that failed rather than being an opaque count.
            assert!(
                report.warnings.iter().any(|warning| warning.contains("runtime") || warning.contains("Node") || warning.contains("node")),
                "no warning mentions what could not be reached: {:?}",
                report.warnings
            );
        }

        /// RESTART / retry. Deleting twice is what an operator does when the
        /// first attempt reported warnings. The second must be a clean
        /// `NotFound`, never a panic and never a second partial teardown.
        #[tokio::test]
        async fn deleting_twice_is_not_found_the_second_time() {
            let (app_repo, server_repo, network_repo, sessions, local_process_manager, _registry, firewall_rule_repo, _rc, log_capture, db_repo, dns_repo) =
                temp_setup();
            let id = docker_application(&app_repo, None, "Twice");

            delete_for_test(
                &app_repo, &server_repo, &db_repo, &network_repo, &firewall_rule_repo, &dns_repo, &sessions, &local_process_manager, &log_capture, id,
            )
            .await
            .unwrap();
            let second = delete_for_test(
                &app_repo, &server_repo, &db_repo, &network_repo, &firewall_rule_repo, &dns_repo, &sessions, &local_process_manager, &log_capture, id,
            )
            .await;
            assert!(matches!(second, Err(AppError::NotFound(_))), "{second:?}");
        }

        /// The delete options are a promise about data. `drop_databases:
        /// false` exists so an operator can keep a database that outlives the
        /// Application, and a teardown that dropped it anyway would be
        /// unrecoverable.
        #[tokio::test]
        async fn a_teardown_that_was_told_to_keep_databases_reports_none_dropped() {
            let (app_repo, server_repo, network_repo, sessions, local_process_manager, _registry, firewall_rule_repo, _rc, log_capture, db_repo, dns_repo) =
                temp_setup();
            let id = docker_application(&app_repo, None, "Keeps Its Data");

            let report = delete_application(
                &app_repo,
                &server_repo,
                &db_repo,
                &network_repo,
                &firewall_rule_repo,
                &dns_repo,
                &sessions,
                &local_process_manager,
                &log_capture,
                ".vibe",
                id,
                ApplicationDeleteOptions { drop_databases: false, remove_files: false },
            )
            .await
            .unwrap();

            assert_eq!(report.databases_dropped, 0);
            assert!(!report.working_directory_removed, "files were not asked for and must not be reported as removed");
        }

        /// CONCURRENT. Two deletes of one Application - a double-clicked
        /// confirm button. Exactly one may report a teardown; the other has
        /// to be a `NotFound`, not a second run of the Node-side steps.
        #[tokio::test]
        async fn two_concurrent_deletes_tear_down_once() {
            let (app_repo, server_repo, network_repo, sessions, local_process_manager, _registry, firewall_rule_repo, _rc, log_capture, db_repo, dns_repo) =
                temp_setup();
            let id = docker_application(&app_repo, None, "Double Clicked");

            let first = delete_for_test(
                &app_repo, &server_repo, &db_repo, &network_repo, &firewall_rule_repo, &dns_repo, &sessions, &local_process_manager, &log_capture, id,
            );
            let second = delete_for_test(
                &app_repo, &server_repo, &db_repo, &network_repo, &firewall_rule_repo, &dns_repo, &sessions, &local_process_manager, &log_capture, id,
            );
            let (first, second) = tokio::join!(first, second);

            let succeeded = [first.is_ok(), second.is_ok()].iter().filter(|ok| **ok).count();
            assert_eq!(succeeded, 1, "both deletes reported a teardown for one application");
            assert!(app_repo.get(id).unwrap().is_none());
        }

        /// A migration onto an unreachable Node fails, and leaves the
        /// Applications list exactly as it found it.
        ///
        /// **What this does not cover, stated rather than implied.** The
        /// rollback added in F.4 (`roll_back_target`) runs when provisioning
        /// fails *after* the target row exists. Reaching that needs the
        /// target Node to answer far enough for
        /// `ensure_working_directory_exists` to succeed and then fail later,
        /// which needs a real SSH server - so this test exercises the
        /// earlier path, where the migration is refused before any row is
        /// written. It is still the property an operator cares about (a
        /// failed migration must not leave a phantom Application), but the
        /// compensating delete itself is unverified and belongs in the
        /// integration pass. Naming it here so nobody reads a green test as
        /// coverage it is not.
        #[tokio::test]
        async fn a_migration_onto_an_unreachable_node_leaves_no_application_behind() {
            let (app_repo, server_repo, network_repo, sessions, local_process_manager, _registry, firewall_rule_repo, registry_repo, log_capture, db_repo, dns_repo) =
                temp_setup();
            let source_node = unreachable_node(&server_repo, "Source");
            let target_node = unreachable_node(&server_repo, "Target");
            let id = docker_application(&app_repo, Some(source_node), "Migrant");
            let before = app_repo.list().unwrap().len();

            let locks = crate::state::MigrationLockManager::default();
            let result = crate::services::migration_service::migrate_application(
                &app_repo,
                &server_repo,
                &network_repo,
                &dns_repo,
                &db_repo,
                ".vibe",
                &firewall_rule_repo,
                &registry_repo,
                &log_capture,
                &sessions,
                &locks,
                &local_process_manager,
                id,
                target_node,
            )
            .await;

            assert!(result.is_err(), "a migration onto an unreachable node must not report success");
            assert_eq!(
                app_repo.list().unwrap().len(),
                before,
                "the half-provisioned target application was left behind: {:?}",
                app_repo.list().unwrap().iter().map(|a| a.name.clone()).collect::<Vec<_>>()
            );
        }
    }

}
