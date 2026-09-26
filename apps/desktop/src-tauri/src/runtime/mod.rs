//! The runtime abstraction Applications is built on - see
//! docs/architecture/APPLICATIONS_ARCHITECTURE.md Section 5 for the full design. One
//! trait, `Box<dyn ApplicationRuntime>` held by commands and services, and
//! exactly one place (`runtime_for`) that matches on `RuntimeType` to pick
//! which implementation to construct.
//!
//! Unlike the `ServerConnection` trait this was originally modelled on -
//! since deleted, see `crate::transport` for why - this one earns its keep:
//! it has four real implementations (Docker, systemd, local process, remote
//! process) and every call site genuinely goes through it.

pub mod docker;
pub mod docker_command;
pub mod local_docker_console;
pub mod member;
pub mod health_check;
pub mod local_process;
pub mod remote_process;
pub mod systemd;

use std::sync::Arc;

use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{Application, ApplicationPort, ApplicationStatus, EnvironmentVariable, RuntimeType};
use crate::ssh::SshSession;

/// Everything a runtime call needs, resolved once per command rather than
/// every trait method re-deriving it - mirrors how actions_commands.rs
/// already resolves `(repo, sessions, server_id)` once per command today.
pub struct RuntimeContext<'a> {
    pub application: &'a Application,
    pub runtime_config: &'a serde_json::Value,
    /// From the `application_environment` table (`Application` itself
    /// deliberately excludes it, see models::application's own doc comment)
    /// - added once `LocalProcessRuntime` (Phase 2) needed it to actually
    /// launch a process; every other trait method is free to ignore it.
    pub environment: &'a [EnvironmentVariable],
    /// From the `application_ports` table - added once `DockerRuntime`
    /// needed real port publishing (`docker create -p`), the same reason
    /// `environment` was added above. Every other runtime is free to
    /// ignore it; Local/Remote process/systemd have no equivalent
    /// "publish a port" step of their own (a process already binds
    /// wherever its own config tells it to).
    pub ports: &'a [ApplicationPort],
    /// The other Applications this one is allowed to reach over the Node's
    /// internal Docker networking, from `ApplicationDetail::links`. Added
    /// once reachability between Applications became default-deny
    /// (`AUDIT_REPORT.md` S-018): `DockerRuntime` needs it to decide which
    /// private networks this container belongs on, and - just as
    /// importantly - which ones it must be disconnected from. Every other
    /// runtime ignores it; nothing but Docker has a network to place a
    /// workload on in the first place.
    pub links: &'a [Uuid],
    /// `None` for `RuntimeType::LocalProcess`; `Some` (from
    /// `SshSessionManager`, same cache every other remote feature already
    /// shares) for every Remote runtime type.
    pub connection: Option<Arc<SshSession>>,
}

/// `Serialize` so a Tauri command can return this directly to the frontend
/// (e.g. `application_resource_usage`) - added once a command actually
/// needed to, not part of the trait's own Phase 0 shape.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceUsage {
    pub cpu_percent: Option<f32>,
    pub ram_bytes: Option<u64>,
    pub uptime_seconds: Option<u64>,
}

/// `Serialize` (tagged, not the default enum encoding - `Unhealthy` carries
/// data the other two variants don't) so `application_health_check` can hand
/// this straight to the frontend: `{"status":"healthy"}` /
/// `{"status":"unhealthy","reason":"..."}` / `{"status":"unknown"}`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum HealthStatus {
    Healthy,
    Unhealthy { reason: String },
    Unknown,
}

/// What a health check actually probes, resolved once by the service layer
/// (`application_service::resolve_health_check_spec`) from an Application's
/// stored `health_check_*` columns plus its ports - see that function's own
/// doc comment for why resolution can come back `None` instead of `Unknown`
/// as a spec. Every runtime checks "is the process still running" first
/// regardless of this (see `health_check::default_health_check`); this is
/// only the *additional* probe layered on top for anything beyond `Process`.
#[derive(Debug, Clone)]
pub enum HealthCheckSpec {
    Process,
    /// Plain TCP connect - Local dials `127.0.0.1:port` directly, Remote
    /// dials from the target host itself over SSH (see
    /// `health_check::check_tcp`'s doc comment for why "from the host", not
    /// from the VibeSSH desktop).
    Tcp { port: u16 },
    /// `GET path` on `port`, healthy on any 2xx/3xx response - same
    /// Local-direct/Remote-via-SSH split as `Tcp`.
    Http { port: u16, path: String },
    /// The real Minecraft Server List Ping protocol, always dialed directly
    /// from the VibeSSH desktop (never via SSH) - see
    /// `health_check::check_minecraft_status`'s doc comment for why.
    MinecraftStatus { host: String, port: u16 },
}

#[async_trait::async_trait]
pub trait ApplicationRuntime: Send + Sync {
    /// Checked before create/start - binary exists, directory writable,
    /// Docker actually present on this host, etc. Distinct from
    /// `health_check`, which checks an already-*running* application.
    async fn validate(&self, ctx: &RuntimeContext<'_>) -> AppResult<()>;

    async fn start(&self, ctx: &RuntimeContext<'_>) -> AppResult<()>;
    /// `graceful = false` is this runtime's normal stop signal (SIGTERM /
    /// `systemctl stop` / `docker stop`) - the UI's separate "Kill" action
    /// maps to `kill()` below, never to `stop(ctx, false)`.
    async fn stop(&self, ctx: &RuntimeContext<'_>, graceful: bool) -> AppResult<()>;
    async fn restart(&self, ctx: &RuntimeContext<'_>) -> AppResult<()>;
    async fn kill(&self, ctx: &RuntimeContext<'_>) -> AppResult<()>;

    async fn status(&self, ctx: &RuntimeContext<'_>) -> AppResult<ApplicationStatus>;
    async fn resource_usage(&self, ctx: &RuntimeContext<'_>) -> AppResult<ResourceUsage>;
    /// `spec` is resolved once by the caller (see `HealthCheckSpec`'s own
    /// doc comment), not derived from `ctx` here - keeps each runtime's
    /// implementation to "check status, then delegate to
    /// `health_check::default_health_check`" rather than 4 copies of the
    /// same DB-lookup-and-port-resolution logic.
    async fn health_check(&self, ctx: &RuntimeContext<'_>, spec: &HealthCheckSpec) -> AppResult<HealthStatus>;

    /// `Ok(None)` (not an error) when this runtime/application genuinely
    /// has no interactive console - e.g. a systemd unit with no stdin
    /// (docs/architecture/APPLICATIONS_ARCHITECTURE.md's Console section) - the UI must
    /// show a clearly-labeled read-only state for that, never silently
    /// swallow input.
    async fn console(&self, ctx: &RuntimeContext<'_>) -> AppResult<Option<Box<dyn ApplicationConsole>>>;
    async fn logs(&self, ctx: &RuntimeContext<'_>) -> AppResult<Box<dyn LogProvider>>;

    /// Tears down whatever `start()` created, *without* touching the
    /// Application's own config - only `runtime::docker::DockerRuntime`
    /// overrides this (stop + `docker rm` an existing container). The
    /// default no-op is correct, not a stub, for the other three runtimes:
    /// `start()` on Local/Remote process and systemd already regenerates
    /// their config every time (see each module's own doc comment on why
    /// that's safe there but not for Docker), so there's nothing a separate
    /// "destroy" step would need to clean up first. Etap M1's "Recreate
    /// Container" action is `destroy()` then `start()` -
    /// `services::application_service::recreate_application`.
    /// Re-applies `RuntimeContext::links` - which other Applications this
    /// one is allowed to reach - to whatever the Node is actually running.
    ///
    /// Called when a connection is granted or revoked, so the change lands
    /// on a running workload immediately. Deliberately not folded into
    /// `start`: revoking has to take effect on an already-running container,
    /// which is precisely the case `start` never sees.
    ///
    /// No-op by default. Only `DockerRuntime` places a workload on a network
    /// at all; a systemd unit or a bare process binds wherever its own
    /// config says, and VibeSSH has nothing to interpose there.
    async fn sync_connections(&self, _ctx: &RuntimeContext<'_>) -> AppResult<()> {
        Ok(())
    }

    async fn destroy(&self, _ctx: &RuntimeContext<'_>) -> AppResult<()> {
        Ok(())
    }
}

#[async_trait::async_trait]
pub trait ApplicationConsole: Send + Sync {
    async fn write(&self, input: &str) -> AppResult<()>;
    fn close(&self);
    /// `false` for a read-only fallback - UI disables the input box and
    /// says why, rather than accepting keystrokes that go nowhere.
    fn supports_input(&self) -> bool;
}

#[async_trait::async_trait]
pub trait LogProvider: Send + Sync {
    /// The last `max_lines` lines available right now - what the Logs tab
    /// shows before the user asks to live-follow.
    async fn tail(&self, max_lines: u32) -> AppResult<Vec<String>>;
}

/// The one and only place that matches on `RuntimeType` to pick which
/// `ApplicationRuntime` implementation to construct - every command/service
/// that needs to act on an `Application` goes through this rather than
/// each doing its own `if runtime_type == ...` branching (the whole point
/// of this trait existing, per this module's own top doc comment).
///
/// Takes `local_process_manager` unconditionally even though only the
/// `LocalProcess` arm uses it - the alternative (returning early with a
/// `LocalProcess`-specific constructor signature) would defeat the "one
/// call site returns `Box<dyn ApplicationRuntime>`" point of this function.
pub fn runtime_for(runtime_type: RuntimeType, local_process_manager: Arc<local_process::LocalProcessManager>) -> Box<dyn ApplicationRuntime> {
    match runtime_type {
        RuntimeType::LocalProcess => Box::new(local_process::LocalProcessRuntime::new(local_process_manager)),
        RuntimeType::RemoteProcess => Box::new(remote_process::RemoteProcessRuntime::new()),
        RuntimeType::Systemd => Box::new(systemd::SystemdRuntime::new()),
        RuntimeType::Docker => Box::new(docker::DockerRuntime::new()),
    }
}

/// Shared by `runtime::docker::build_create_command`,
/// `runtime::systemd::resource_limit_lines`, and
/// `services::set_application_resource_limits` - the same "a limit, if
/// given, must be a positive value" rule applies in all three places, so
/// it's written once rather than three times.
pub fn validate_resource_limits(memory_limit_mb: Option<u32>, cpu_limit_cores: Option<f32>) -> AppResult<()> {
    if let Some(mb) = memory_limit_mb {
        if mb == 0 {
            return Err(AppError::InvalidInput("the memory limit must be greater than 0".into()));
        }
    }
    if let Some(cores) = cpu_limit_cores {
        // `!(cores > 0.0)` would also be correct - it rejects NaN, which a
        // bare `cores <= 0.0` does not - but it reads as a typo. This says
        // the same thing outright.
        if !cores.is_finite() || cores <= 0.0 {
            return Err(AppError::InvalidInput("the CPU limit must be greater than 0".into()));
        }
    }
    Ok(())
}

pub fn runtime_type_display_name(runtime_type: RuntimeType) -> &'static str {
    match runtime_type {
        RuntimeType::LocalProcess => "Local Process",
        RuntimeType::RemoteProcess => "Remote Process",
        RuntimeType::Systemd => "systemd",
        RuntimeType::Docker => "Docker",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Only the tests build a full `Application`; the module itself never
    // names this type.
    use crate::models::HealthCheckType;
    use crate::errors::AppError;

    /// Proves the trait is object-safe (`Box<dyn ApplicationRuntime>`
    /// compiles) and usable from async code - same purpose as
    /// `transport::tests::StubConnection`, before any real implementation
    /// exists.
    struct StubRuntime;

    #[async_trait::async_trait]
    impl ApplicationRuntime for StubRuntime {
        async fn validate(&self, _ctx: &RuntimeContext<'_>) -> AppResult<()> {
            Ok(())
        }
        async fn start(&self, _ctx: &RuntimeContext<'_>) -> AppResult<()> {
            Ok(())
        }
        async fn stop(&self, _ctx: &RuntimeContext<'_>, _graceful: bool) -> AppResult<()> {
            Ok(())
        }
        async fn restart(&self, _ctx: &RuntimeContext<'_>) -> AppResult<()> {
            Ok(())
        }
        async fn kill(&self, _ctx: &RuntimeContext<'_>) -> AppResult<()> {
            Ok(())
        }
        async fn status(&self, _ctx: &RuntimeContext<'_>) -> AppResult<ApplicationStatus> {
            Ok(ApplicationStatus::Unknown)
        }
        async fn resource_usage(&self, _ctx: &RuntimeContext<'_>) -> AppResult<ResourceUsage> {
            Err(AppError::Internal("not implemented in stub".into()))
        }
        async fn health_check(&self, _ctx: &RuntimeContext<'_>, _spec: &HealthCheckSpec) -> AppResult<HealthStatus> {
            Ok(HealthStatus::Unknown)
        }
        async fn console(&self, _ctx: &RuntimeContext<'_>) -> AppResult<Option<Box<dyn ApplicationConsole>>> {
            Ok(None)
        }
        async fn logs(&self, _ctx: &RuntimeContext<'_>) -> AppResult<Box<dyn LogProvider>> {
            struct EmptyLogs;
            #[async_trait::async_trait]
            impl LogProvider for EmptyLogs {
                async fn tail(&self, _max_lines: u32) -> AppResult<Vec<String>> {
                    Ok(vec![])
                }
            }
            Ok(Box::new(EmptyLogs))
        }
    }

    fn stub_application() -> Application {
        Application {
            id: uuid::Uuid::new_v4(),
            server_id: None,
            name: "Stub".to_string(),
            description: None,
            blueprint_id: "generic".to_string(),
            blueprint_version: 1,
            runtime_type: RuntimeType::LocalProcess,
            working_directory: "/tmp".to_string(),
            status: ApplicationStatus::Unknown,
            last_status_check_at: None,
            health_check_type: HealthCheckType::Process,
            health_check_port_id: None,
            health_check_http_path: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn stub_runtime_is_object_safe_and_callable() {
        let runtime: Box<dyn ApplicationRuntime> = Box::new(StubRuntime);
        let application = stub_application();
        let config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &config, environment: &[], ports: &[], links: &[], connection: None };

        runtime.start(&ctx).await.unwrap();
        assert_eq!(runtime.status(&ctx).await.unwrap(), ApplicationStatus::Unknown);
        assert!(runtime.console(&ctx).await.unwrap().is_none());
    }

    #[test]
    fn validate_resource_limits_rejects_zero_memory_and_non_positive_cpu_but_accepts_reasonable_values() {
        assert!(validate_resource_limits(None, None).is_ok());
        assert!(validate_resource_limits(Some(512), Some(1.5)).is_ok());
        assert!(validate_resource_limits(Some(0), None).is_err());
        assert!(validate_resource_limits(None, Some(0.0)).is_err());
        assert!(validate_resource_limits(None, Some(-1.0)).is_err());
    }
}
