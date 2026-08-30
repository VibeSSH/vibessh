//! The runtime abstraction Applications is built on - see
//! docs/APPLICATIONS_ARCHITECTURE.md Section 5 for the full design. Mirrors
//! `transport::ServerConnection`'s own shape deliberately: one trait,
//! `Box<dyn ApplicationRuntime>` held by commands/services, exactly one
//! place (`runtime_for`, added once real implementations exist in later
//! phases) that matches on `RuntimeType` to pick which implementation to
//! construct. No implementation lands in this phase - this is the
//! interface Phase 2 (LocalProcessRuntime) onward builds against.

pub mod local_process;
pub mod systemd;

use std::sync::Arc;

use crate::errors::AppResult;
use crate::models::{Application, ApplicationStatus, EnvironmentVariable, RuntimeType};
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
    /// `None` for `RuntimeType::LocalProcess`; `Some` (from
    /// `SshSessionManager`, same cache every other remote feature already
    /// shares) for every Remote runtime type.
    pub connection: Option<Arc<SshSession>>,
}

#[derive(Debug, Clone, Copy)]
pub struct ResourceUsage {
    pub cpu_percent: Option<f32>,
    pub ram_bytes: Option<u64>,
    pub uptime_seconds: Option<u64>,
}

#[derive(Debug, Clone)]
pub enum HealthStatus {
    Healthy,
    Unhealthy(String),
    Unknown,
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
    async fn health_check(&self, ctx: &RuntimeContext<'_>) -> AppResult<HealthStatus>;

    /// `Ok(None)` (not an error) when this runtime/application genuinely
    /// has no interactive console - e.g. a systemd unit with no stdin
    /// (docs/APPLICATIONS_ARCHITECTURE.md's Console section) - the UI must
    /// show a clearly-labeled read-only state for that, never silently
    /// swallow input.
    async fn console(&self, ctx: &RuntimeContext<'_>) -> AppResult<Option<Box<dyn ApplicationConsole>>>;
    async fn logs(&self, ctx: &RuntimeContext<'_>) -> AppResult<Box<dyn LogProvider>>;
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

/// A `RuntimeType` this build has no implementation for yet (every type is
/// still in this state until its own phase lands - see
/// docs/APPLICATIONS_ARCHITECTURE.md Section 10) - kept as its own
/// documented case rather than a panic, since `runtime_for` will need to
/// return *something* the moment `RuntimeType` has more variants than
/// implementations, which is true for the whole of Phase 1.
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
        async fn health_check(&self, _ctx: &RuntimeContext<'_>) -> AppResult<HealthStatus> {
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
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn stub_runtime_is_object_safe_and_callable() {
        let runtime: Box<dyn ApplicationRuntime> = Box::new(StubRuntime);
        let application = stub_application();
        let config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &config, environment: &[], connection: None };

        runtime.start(&ctx).await.unwrap();
        assert_eq!(runtime.status(&ctx).await.unwrap(), ApplicationStatus::Unknown);
        assert!(runtime.console(&ctx).await.unwrap().is_none());
    }
}
