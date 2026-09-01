//! `SystemdRuntime` - Applications running as a systemd system unit over
//! SSH. Extends `ssh::systemd` (list/start/stop/restart/enable/disable
//! reused verbatim via `SshSession`'s own methods, `validate_unit_name`'s
//! injection-safe validation reused as-is) with unit file create/update,
//! which that module didn't need before Applications existed. See
//! docs/APPLICATIONS_ARCHITECTURE.md Section 5.3 for the design and why
//! this is SSH-only (no Agent path) for now.
//!
//! **Known limitation, stated rather than hidden**: units are written to
//! `/etc/systemd/system/`, which needs the SSH-authenticated user to have
//! write access there (typically root, or passwordless sudo - VibeSSH does
//! not prompt for a sudo password over the exec channel). A non-privileged
//! SSH user will see `start()`/`restart()` fail with the underlying
//! permission error surfaced as-is, not a silent no-op.
//!
//! **Known gap**: the `ApplicationRuntime` trait (Phase 0) has no
//! `destroy()`/`remove()` method, so nothing here deletes the unit file
//! when an Application itself is deleted - that has to be wired into
//! whatever future commands-layer phase handles Application deletion for
//! Remote/Systemd applications, not something a `start`/`stop`/`kill`-only
//! trait can express on its own.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::ApplicationStatus;
use crate::ssh::systemd::validate_unit_name;
use crate::ssh::SshSession;

use super::{
    health_check, validate_resource_limits, ApplicationConsole, ApplicationRuntime, HealthCheckSpec, HealthStatus, LogProvider,
    ResourceUsage, RuntimeContext,
};

/// What `runtime_config` deserializes into for `RuntimeType::Systemd`.
///
/// `memory_limit_mb`/`cpu_limit_cores` are set through
/// `services::set_application_resource_limits`, not the Create Application
/// wizard - see that function's own doc comment. Unlike
/// `runtime::docker::DockerConfig`'s equivalent fields, a change here takes
/// effect on the *next* start/restart automatically: `start()` below
/// rewrites the unit file unconditionally every time, there's no "only
/// applied at creation" gap to work around for a systemd unit the way there
/// is for a Docker container's writable layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemdConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub memory_limit_mb: Option<u32>,
    /// Fractional CPU cores, same unit `DockerConfig::cpu_limit_cores` uses
    /// - converted to systemd's own `CPUQuota=<percent>%` at render time
    /// (1 core = 100%) so the UI can offer one consistent "CPU cores" input
    /// regardless of which runtime type an Application actually uses.
    #[serde(default)]
    pub cpu_limit_cores: Option<f32>,
}

fn parse_config(ctx: &RuntimeContext<'_>) -> AppResult<SystemdConfig> {
    serde_json::from_value(ctx.runtime_config.clone())
        .map_err(|err| AppError::InvalidInput(format!("invalid systemd configuration: {err}")))
}

/// `vibessh-app-<uuid>.service` - unambiguously VibeSSH-owned so this
/// runtime never creates, touches, or removes a unit it didn't create
/// itself (docs/APPLICATIONS_ARCHITECTURE.md Section 5.3). Always accepted
/// by `validate_unit_name`, since a `Uuid`'s `Display` is only hex digits
/// and hyphens - validated anyway before every remote use, as defense in
/// depth against that assumption ever changing.
fn unit_name(application_id: Uuid) -> String {
    format!("vibessh-app-{application_id}.service")
}

const UNIT_PATH_PREFIX: &str = "/etc/systemd/system/";

fn unit_path(unit: &str) -> String {
    format!("{UNIT_PATH_PREFIX}{unit}")
}

fn connection_ref<'a>(ctx: &'a RuntimeContext<'_>) -> AppResult<&'a SshSession> {
    ctx.connection.as_deref().ok_or_else(|| AppError::Internal("SystemdRuntime requires a connection".into()))
}

fn connection_arc(ctx: &RuntimeContext<'_>) -> AppResult<Arc<SshSession>> {
    ctx.connection.clone().ok_or_else(|| AppError::Internal("SystemdRuntime requires a connection".into()))
}

/// A raw newline in a unit-file value could inject an unrelated directive
/// on the next line - systemd's config format is line-oriented. Rejected
/// outright (not escaped), same "reject rather than cleverly encode" stance
/// `ssh::systemd::validate_unit_name` already takes for shell
/// metacharacters.
fn reject_newlines(value: &str, field: &str) -> AppResult<()> {
    if value.contains('\n') || value.contains('\r') {
        return Err(AppError::InvalidInput(format!("{field} can't contain a newline")));
    }
    Ok(())
}

/// systemd expands `%i`/`%n`/etc "specifiers" in every directive value -
/// `%%` is how systemd.unit(5) says to escape a literal `%`.
fn escape_specifiers(value: &str) -> String {
    value.replace('%', "%%")
}

/// Quoting for a word-splitting directive (`ExecStart=`, `Environment=`,
/// per systemd.syntax(5)): specifier-escapes the value, then wraps it in
/// `"..."` with `\` and `"` backslash-escaped inside, so the whole thing is
/// one atomic word regardless of embedded spaces or quote characters.
fn quote_unit_value(value: &str) -> String {
    let specifier_escaped = escape_specifiers(value);
    let mut quoted = String::with_capacity(specifier_escaped.len() + 2);
    quoted.push('"');
    for ch in specifier_escaped.chars() {
        match ch {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            _ => quoted.push(ch),
        }
    }
    quoted.push('"');
    quoted
}

/// POSIX environment variable name rule - also guards against a key
/// containing `=`, which would prematurely terminate systemd's own
/// `Environment=` `KEY=VALUE` parsing.
fn is_valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `MemoryMax=`/`CPUQuota=` lines for `[Service]`, plus the
/// `*Accounting=yes` directives older systemd (pre-231) needs to actually
/// enforce them - modern systemd turns accounting on implicitly once a
/// limit directive is present, but setting it explicitly is harmless and
/// costs nothing. Empty string when neither limit is set, so `start()`'s
/// unit file is byte-for-byte what it always was for an Application with no
/// limits configured.
fn resource_limit_lines(config: &SystemdConfig) -> AppResult<String> {
    validate_resource_limits(config.memory_limit_mb, config.cpu_limit_cores)?;
    let mut lines = String::new();
    if let Some(mb) = config.memory_limit_mb {
        lines.push_str(&format!("MemoryAccounting=yes\nMemoryMax={mb}M\n"));
    }
    if let Some(cores) = config.cpu_limit_cores {
        let percent = (cores * 100.0).round() as u32;
        lines.push_str(&format!("CPUAccounting=yes\nCPUQuota={percent}%\n"));
    }
    Ok(lines)
}

fn render_unit_file(ctx: &RuntimeContext<'_>, config: &SystemdConfig) -> AppResult<String> {
    reject_newlines(&config.command, "the command")?;
    for arg in &config.args {
        reject_newlines(arg, "an argument")?;
    }
    reject_newlines(&ctx.application.working_directory, "the working directory")?;
    reject_newlines(&ctx.application.name, "the application name")?;

    let mut exec_start = quote_unit_value(&config.command);
    for arg in &config.args {
        exec_start.push(' ');
        exec_start.push_str(&quote_unit_value(arg));
    }

    let mut environment_lines = String::new();
    for env in ctx.environment {
        if !is_valid_env_key(&env.key) {
            return Err(AppError::InvalidInput(format!("'{}' isn't a valid environment variable name", env.key)));
        }
        reject_newlines(&env.value, &format!("the '{}' environment variable", env.key))?;
        environment_lines.push_str(&format!("Environment={}\n", quote_unit_value(&format!("{}={}", env.key, env.value))));
    }

    let resource_lines = resource_limit_lines(config)?;

    Ok(format!(
        "[Unit]\nDescription=VibeSSH managed application: {}\nAfter=network.target\n\n\
         [Service]\nType=simple\nWorkingDirectory={}\nExecStart={}\n{}{}Restart=on-failure\nRestartSec=5\n\n\
         [Install]\nWantedBy=multi-user.target\n",
        escape_specifiers(&ctx.application.name),
        escape_specifiers(&ctx.application.working_directory),
        exec_start,
        environment_lines,
        resource_lines,
    ))
}

async fn write_unit(connection: &SshSession, ctx: &RuntimeContext<'_>, config: &SystemdConfig) -> AppResult<String> {
    let unit = unit_name(ctx.application.id);
    validate_unit_name(&unit)?;
    let unit_file = render_unit_file(ctx, config)?;
    connection.write_file(&unit_path(&unit), unit_file.as_bytes()).await?;
    run_daemon_reload(connection).await?;
    Ok(unit)
}

async fn run_daemon_reload(connection: &SshSession) -> AppResult<()> {
    let output = connection.execute_command("systemctl daemon-reload").await?;
    if output.exit_code != 0 {
        let detail = output.stderr.trim();
        let detail = if detail.is_empty() { "systemctl daemon-reload failed".to_string() } else { detail.to_string() };
        return Err(AppError::Connection(format!("couldn't reload systemd: {detail}")));
    }
    Ok(())
}

fn map_is_active_output(output: &str) -> ApplicationStatus {
    match output.trim() {
        "active" => ApplicationStatus::Running,
        "activating" => ApplicationStatus::Starting,
        "deactivating" => ApplicationStatus::Stopping,
        "failed" => ApplicationStatus::Failed,
        // "inactive", "unknown", or anything else (including the unit not
        // existing yet, i.e. never started) - Stopped is the honest
        // default, matching LocalProcessRuntime's own "no other place it
        // could be running" reasoning.
        _ => ApplicationStatus::Stopped,
    }
}

fn parse_main_pid(output: &str) -> Option<u32> {
    let pid = output.trim().parse::<u32>().ok()?;
    if pid == 0 {
        None
    } else {
        Some(pid)
    }
}

/// `ps -o pcpu=,rss= -p <pid>`'s output: two whitespace-separated numbers,
/// RSS in KiB per `ps`'s default units.
fn parse_ps_cpu_and_ram(output: &str) -> (Option<f32>, Option<u64>) {
    let mut fields = output.split_whitespace();
    let cpu_percent = fields.next().and_then(|f| f.parse::<f32>().ok());
    let ram_bytes = fields.next().and_then(|f| f.parse::<u64>().ok()).map(|kb| kb * 1024);
    (cpu_percent, ram_bytes)
}

pub struct SystemdRuntime;

impl SystemdRuntime {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SystemdRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ApplicationRuntime for SystemdRuntime {
    async fn validate(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        connection_ref(ctx)?;
        let config = parse_config(ctx)?;
        if config.command.trim().is_empty() {
            return Err(AppError::InvalidInput("no command configured for this application".into()));
        }
        // Exercises every escaping/validation rule up front, so a bad
        // config is rejected before it ever reaches a remote write.
        render_unit_file(ctx, &config)?;
        Ok(())
    }

    async fn start(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        let connection = connection_ref(ctx)?;
        let config = parse_config(ctx)?;
        let unit = write_unit(connection, ctx, &config).await?;
        // `enable --now`: starts it immediately and persists it across a
        // host reboot - the behavior a user asking VibeSSH to run an
        // Application continuously would expect, not something that quietly
        // needs re-starting after the server reboots.
        connection.enable_service(&unit).await
    }

    /// systemd's own `stop` verb is already a graceful stop by construction
    /// (respects the unit's `ExecStop=`/`TimeoutStopSec=`) - there's no
    /// separate "immediate but not a hard kill" primitive to map the
    /// `graceful` flag onto here, unlike `LocalProcessRuntime` where it
    /// distinguishes waiting for exit vs firing the signal and returning.
    /// The real graceful/immediate split for systemd units is `stop()` vs
    /// `kill()`, not a flag within `stop()`.
    async fn stop(&self, ctx: &RuntimeContext<'_>, _graceful: bool) -> AppResult<()> {
        let connection = connection_ref(ctx)?;
        connection.stop_service(&unit_name(ctx.application.id)).await
    }

    async fn restart(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        let connection = connection_ref(ctx)?;
        let config = parse_config(ctx)?;
        let unit = write_unit(connection, ctx, &config).await?;
        connection.restart_service(&unit).await
    }

    async fn kill(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        let connection = connection_ref(ctx)?;
        connection.kill_service(&unit_name(ctx.application.id)).await
    }

    async fn status(&self, ctx: &RuntimeContext<'_>) -> AppResult<ApplicationStatus> {
        let connection = connection_ref(ctx)?;
        let unit = unit_name(ctx.application.id);
        validate_unit_name(&unit)?;
        let output = connection.execute_command(&format!("systemctl is-active {unit}")).await?;
        Ok(map_is_active_output(&output.stdout))
    }

    async fn resource_usage(&self, ctx: &RuntimeContext<'_>) -> AppResult<ResourceUsage> {
        let empty = ResourceUsage { cpu_percent: None, ram_bytes: None, uptime_seconds: None };
        let connection = connection_ref(ctx)?;
        let unit = unit_name(ctx.application.id);
        validate_unit_name(&unit)?;

        let pid_output = connection.execute_command(&format!("systemctl show {unit} --property=MainPID --value")).await?;
        let Some(pid) = parse_main_pid(&pid_output.stdout) else {
            return Ok(empty);
        };

        let ps_output = connection.execute_command(&format!("ps -o pcpu=,rss= -p {pid}")).await?;
        let (cpu_percent, ram_bytes) = parse_ps_cpu_and_ram(&ps_output.stdout);

        // Not attempting to derive uptime from `ActiveEnterTimestamp` -
        // systemd's timestamp format is locale/timezone-dependent and
        // parsing it unreliably would be worse than honestly reporting
        // "unknown" here.
        Ok(ResourceUsage { cpu_percent, ram_bytes, uptime_seconds: None })
    }

    async fn health_check(&self, ctx: &RuntimeContext<'_>, spec: &HealthCheckSpec) -> AppResult<HealthStatus> {
        let status = self.status(ctx).await?;
        health_check::default_health_check(ctx, spec, status, Some("the systemd unit reported a failed state")).await
    }

    async fn console(&self, _ctx: &RuntimeContext<'_>) -> AppResult<Option<Box<dyn ApplicationConsole>>> {
        // No interactive stdin for a systemd-managed unit in this phase -
        // this is the trait's own documented example of a legitimate
        // `None` (see `ApplicationRuntime::console`'s doc comment).
        Ok(None)
    }

    async fn logs(&self, ctx: &RuntimeContext<'_>) -> AppResult<Box<dyn LogProvider>> {
        let connection = connection_arc(ctx)?;
        Ok(Box::new(SystemdLogs { connection, unit: unit_name(ctx.application.id) }))
    }
}

struct SystemdLogs {
    connection: Arc<SshSession>,
    unit: String,
}

#[async_trait::async_trait]
impl LogProvider for SystemdLogs {
    async fn tail(&self, max_lines: u32) -> AppResult<Vec<String>> {
        validate_unit_name(&self.unit)?;
        let max_lines = max_lines.clamp(1, 5000);
        let output = self
            .connection
            .execute_command(&format!("journalctl -u {} -n {max_lines} --no-pager --output=short-iso", self.unit))
            .await?;
        Ok(output.stdout.lines().map(str::to_string).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Application, EnvironmentVariable, HealthCheckType, RuntimeType};

    fn stub_application(id: Uuid) -> Application {
        Application {
            id,
            server_id: Some(Uuid::new_v4()),
            name: "My App".to_string(),
            description: None,
            blueprint_id: "generic".to_string(),
            blueprint_version: 1,
            runtime_type: RuntimeType::Systemd,
            working_directory: "/srv/my-app".to_string(),
            status: ApplicationStatus::Unknown,
            last_status_check_at: None,
            health_check_type: HealthCheckType::Process,
            health_check_port_id: None,
            health_check_http_path: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn unit_name_is_stable_and_namespaced() {
        let id = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        assert_eq!(unit_name(id), "vibessh-app-11111111-2222-3333-4444-555555555555.service");
        assert!(validate_unit_name(&unit_name(id)).is_ok());
    }

    #[test]
    fn quote_unit_value_escapes_quotes_backslashes_and_specifiers() {
        assert_eq!(quote_unit_value("plain"), "\"plain\"");
        assert_eq!(quote_unit_value("has space"), "\"has space\"");
        assert_eq!(quote_unit_value(r#"has "quotes""#), r#""has \"quotes\"""#);
        assert_eq!(quote_unit_value(r"has\backslash"), r#""has\\backslash""#);
        assert_eq!(quote_unit_value("100%"), "\"100%%\"");
    }

    #[test]
    fn is_valid_env_key_rejects_equals_and_leading_digits() {
        assert!(is_valid_env_key("PORT"));
        assert!(is_valid_env_key("_PRIVATE_VAR"));
        assert!(!is_valid_env_key("PORT=8080"));
        assert!(!is_valid_env_key("8080PORT"));
        assert!(!is_valid_env_key(""));
    }

    #[test]
    fn render_unit_file_produces_a_well_formed_service_section() {
        let application = stub_application(Uuid::new_v4());
        let config = SystemdConfig { command: "/usr/bin/java".into(), args: vec!["-jar".into(), "server.jar".into()], memory_limit_mb: None, cpu_limit_cores: None };
        let environment = vec![EnvironmentVariable { key: "PORT".into(), value: "25565".into(), is_secret: false }];
        let config_value = serde_json::to_value(&config).unwrap();
        let ctx = RuntimeContext { application: &application, runtime_config: &config_value, environment: &environment, ports: &[], links: &[], connection: None };

        let unit_file = render_unit_file(&ctx, &config).unwrap();

        assert!(unit_file.contains("[Unit]\n"));
        assert!(unit_file.contains("[Service]\n"));
        assert!(unit_file.contains("[Install]\n"));
        assert!(unit_file.contains("WorkingDirectory=/srv/my-app\n"));
        assert!(unit_file.contains(r#"ExecStart="/usr/bin/java" "-jar" "server.jar""#));
        assert!(unit_file.contains(r#"Environment="PORT=25565""#));
        assert!(unit_file.contains("WantedBy=multi-user.target"));
    }

    #[test]
    fn render_unit_file_rejects_a_newline_in_the_command() {
        let application = stub_application(Uuid::new_v4());
        let config = SystemdConfig { command: "/bin/sh\nrm -rf /".into(), args: vec![], memory_limit_mb: None, cpu_limit_cores: None };
        let config_value = serde_json::to_value(&config).unwrap();
        let ctx = RuntimeContext { application: &application, runtime_config: &config_value, environment: &[], ports: &[], links: &[], connection: None };

        assert!(render_unit_file(&ctx, &config).is_err());
    }

    #[test]
    fn render_unit_file_rejects_an_invalid_environment_key() {
        let application = stub_application(Uuid::new_v4());
        let config = SystemdConfig { command: "/usr/bin/java".into(), args: vec![], memory_limit_mb: None, cpu_limit_cores: None };
        let environment = vec![EnvironmentVariable { key: "NOT VALID".into(), value: "x".into(), is_secret: false }];
        let config_value = serde_json::to_value(&config).unwrap();
        let ctx = RuntimeContext { application: &application, runtime_config: &config_value, environment: &environment, ports: &[], links: &[], connection: None };

        assert!(render_unit_file(&ctx, &config).is_err());
    }

    #[test]
    fn render_unit_file_adds_memory_and_cpu_directives_only_when_set() {
        let application = stub_application(Uuid::new_v4());
        let without_limits = SystemdConfig { command: "/usr/bin/java".into(), args: vec![], memory_limit_mb: None, cpu_limit_cores: None };
        let config_value = serde_json::to_value(&without_limits).unwrap();
        let ctx = RuntimeContext { application: &application, runtime_config: &config_value, environment: &[], ports: &[], links: &[], connection: None };
        let unit_file = render_unit_file(&ctx, &without_limits).unwrap();
        assert!(!unit_file.contains("MemoryMax"));
        assert!(!unit_file.contains("CPUQuota"));

        let with_limits = SystemdConfig { command: "/usr/bin/java".into(), args: vec![], memory_limit_mb: Some(1024), cpu_limit_cores: Some(1.5) };
        let config_value = serde_json::to_value(&with_limits).unwrap();
        let ctx = RuntimeContext { application: &application, runtime_config: &config_value, environment: &[], ports: &[], links: &[], connection: None };
        let unit_file = render_unit_file(&ctx, &with_limits).unwrap();
        assert!(unit_file.contains("MemoryAccounting=yes\n"));
        assert!(unit_file.contains("MemoryMax=1024M\n"));
        assert!(unit_file.contains("CPUAccounting=yes\n"));
        // 1.5 cores -> 150%, matching CPUQuota's own "percent of one CPU" unit.
        assert!(unit_file.contains("CPUQuota=150%\n"));
    }

    #[test]
    fn render_unit_file_rejects_a_zero_memory_limit_or_non_positive_cpu_limit() {
        let application = stub_application(Uuid::new_v4());

        let zero_memory = SystemdConfig { command: "/usr/bin/java".into(), args: vec![], memory_limit_mb: Some(0), cpu_limit_cores: None };
        let config_value = serde_json::to_value(&zero_memory).unwrap();
        let ctx = RuntimeContext { application: &application, runtime_config: &config_value, environment: &[], ports: &[], links: &[], connection: None };
        assert!(render_unit_file(&ctx, &zero_memory).is_err());

        let negative_cpu = SystemdConfig { command: "/usr/bin/java".into(), args: vec![], memory_limit_mb: None, cpu_limit_cores: Some(-1.0) };
        let config_value = serde_json::to_value(&negative_cpu).unwrap();
        let ctx = RuntimeContext { application: &application, runtime_config: &config_value, environment: &[], ports: &[], links: &[], connection: None };
        assert!(render_unit_file(&ctx, &negative_cpu).is_err());
    }

    #[test]
    fn map_is_active_output_covers_every_systemctl_state() {
        assert_eq!(map_is_active_output("active\n"), ApplicationStatus::Running);
        assert_eq!(map_is_active_output("activating\n"), ApplicationStatus::Starting);
        assert_eq!(map_is_active_output("deactivating\n"), ApplicationStatus::Stopping);
        assert_eq!(map_is_active_output("failed\n"), ApplicationStatus::Failed);
        assert_eq!(map_is_active_output("inactive\n"), ApplicationStatus::Stopped);
        assert_eq!(map_is_active_output("unknown\n"), ApplicationStatus::Stopped);
    }

    #[test]
    fn parse_main_pid_treats_zero_and_garbage_as_not_running() {
        assert_eq!(parse_main_pid("1234\n"), Some(1234));
        assert_eq!(parse_main_pid("0\n"), None);
        assert_eq!(parse_main_pid("\n"), None);
        assert_eq!(parse_main_pid("not-a-pid"), None);
    }

    #[test]
    fn parse_ps_cpu_and_ram_reads_both_fields() {
        assert_eq!(parse_ps_cpu_and_ram(" 12.3  4096\n"), (Some(12.3), Some(4096 * 1024)));
        assert_eq!(parse_ps_cpu_and_ram(""), (None, None));
    }

    #[tokio::test]
    async fn methods_that_need_a_connection_fail_cleanly_without_one() {
        let application = stub_application(Uuid::new_v4());
        let config = serde_json::json!({ "command": "/usr/bin/java", "args": [] });
        let ctx = RuntimeContext { application: &application, runtime_config: &config, environment: &[], ports: &[], links: &[], connection: None };
        let runtime = SystemdRuntime::new();

        assert!(matches!(runtime.validate(&ctx).await, Err(AppError::Internal(_))));
        assert!(matches!(runtime.start(&ctx).await, Err(AppError::Internal(_))));
        assert!(matches!(runtime.status(&ctx).await, Err(AppError::Internal(_))));
        assert!(matches!(runtime.logs(&ctx).await, Err(AppError::Internal(_))));
    }
}
