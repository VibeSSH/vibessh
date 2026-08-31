//! `DockerRuntime` - Applications running as a Docker container over SSH.
//! Extends `ssh::docker` (list/start/stop/restart/remove/logs reused
//! verbatim via `SshSession`'s own methods, `validate_container_ref`'s
//! injection-safe validation reused as-is) with `docker create`, which that
//! module didn't need before Applications existed. SSH-only, matching the
//! same confirmed decision `runtime::systemd` documents
//! (docs/APPLICATIONS_ARCHITECTURE.md Section 5.3/11) - Agent-managed
//! Docker stays a later, explicit decision made with a real feature in
//! hand, not granted on spec.
//!
//! **Port publishing** (`-p bind:external:internal/proto`, in
//! `build_create_command`) only happens for a declared `ApplicationPort`
//! whose `external_port` is actually set - one without it is documentation
//! of an internal-only port (e.g. a database another container reaches over
//! the Docker network), not something meant to be reachable from outside
//! the host, so it gets no `-p` flag at all.
//!
//! **Etap M1: `working_directory` is bind-mounted in** (`-v dir:dir` plus
//! `-w dir`, host path = container path, so a jar/config path that's
//! already just a bare filename relative to `working_directory` - the same
//! assumption `runtime::local_process`/`remote_process`/`systemd` already
//! make by launching with that as their own cwd - resolves identically
//! inside the container, no path-translation layer needed anywhere else).
//! This is *why* `start()`'s recreate-avoidance below is safe now instead
//! of destroying state on every recreate: the container's writable layer is
//! no longer the only place a world save/database file/config lives, the
//! bind-mounted host directory is. **Known, deliberate gap**: no `--user`
//! is set, so a container runs as whatever user its image defaults to
//! (usually root) - if that user's uid/gid doesn't match the host
//! directory's owner, writes can fail with a permission error. Solving this
//! generally means `stat`-ing the host directory before every `docker
//! create` and passing `--user uid:gid`, which risks breaking images that
//! deliberately expect to run as a specific baked-in user (several official
//! images do real setup as root before dropping privileges themselves) -
//! not attempted here without wider testing across real blueprint images.
//!
//! **Unlike `runtime::systemd`/`runtime::remote_process`, `start()` does
//! NOT unconditionally recreate.** Those runtimes persist only *config*
//! (a unit file, a shell command) that's always safe to regenerate. Now that
//! a Docker container's state lives in the bind mount rather than its
//! writable layer, recreating the *container* on every start would still be
//! wasteful (and briefly drop it, mid-health-check, for no reason) even
//! though it's no longer destructive - `start()` still only runs `docker
//! create` when no container with this application's name exists yet.
//! Picking up an edited image/command/restart policy needs an explicit
//! recreate - `destroy()` below, plus `start()` - exposed as a "Recreate
//! Container" action
//! (`services::application_service::recreate_application`), not implicit on
//! every start.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{ApplicationStatus, EnvironmentVariable, PortProtocol};
use crate::ssh::docker::validate_container_ref;
use crate::ssh::SshSession;

use super::{
    health_check, validate_resource_limits, ApplicationConsole, ApplicationRuntime, HealthCheckSpec, HealthStatus, LogProvider,
    ResourceUsage, RuntimeContext,
};

/// What `runtime_config` deserializes into for `RuntimeType::Docker`.
/// `command`, if given, overrides the image's own `ENTRYPOINT`/`CMD` - a
/// container created without one just runs the image as authored.
///
/// `memory_limit_mb`/`cpu_limit_cores` are set through
/// `services::set_application_resource_limits`, not the Create Application
/// wizard - see that function's own doc comment. They're only baked in at
/// `docker create` time (see `create_container`), so, same as an edited
/// `image`/`command`, a change here doesn't reach an already-existing
/// container until it's removed and recreated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerConfig {
    pub image: String,
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub memory_limit_mb: Option<u32>,
    #[serde(default)]
    pub cpu_limit_cores: Option<f32>,
    /// `--restart <policy>` - defaults to `unless-stopped` when absent (see
    /// `restart_policy_or_default`), including for every Docker Application
    /// created before this field existed, via `#[serde(default)]`.
    #[serde(default)]
    pub restart_policy: Option<String>,
}

/// `docker create --restart` only accepts a fixed set of values - validated
/// here (not left for the daemon to reject) so a typo surfaces as a clear
/// `AppError::InvalidInput` before ever reaching `execute_command`, matching
/// how `validate_resource_limits` already validates the other two config
/// values before they're used to build a command.
const VALID_RESTART_POLICIES: &[&str] = &["no", "always", "unless-stopped", "on-failure"];

fn restart_policy_or_default(config: &DockerConfig) -> AppResult<&str> {
    let policy = config.restart_policy.as_deref().unwrap_or("unless-stopped");
    if !VALID_RESTART_POLICIES.contains(&policy) {
        return Err(AppError::InvalidInput(format!(
            "'{policy}' isn't a valid restart policy - expected one of {VALID_RESTART_POLICIES:?}"
        )));
    }
    Ok(policy)
}

fn parse_config(ctx: &RuntimeContext<'_>) -> AppResult<DockerConfig> {
    serde_json::from_value(ctx.runtime_config.clone())
        .map_err(|err| AppError::InvalidInput(format!("invalid Docker configuration: {err}")))
}

/// `vibessh-app-<uuid>` - unambiguously VibeSSH-owned, matching
/// `runtime::systemd`'s unit-naming reasoning, so this runtime never
/// touches a container it didn't create.
fn container_name(application_id: Uuid) -> String {
    format!("vibessh-app-{application_id}")
}

fn connection_ref<'a>(ctx: &'a RuntimeContext<'_>) -> AppResult<&'a SshSession> {
    ctx.connection.as_deref().ok_or_else(|| AppError::Internal("DockerRuntime requires a connection".into()))
}

fn connection_arc(ctx: &RuntimeContext<'_>) -> AppResult<Arc<SshSession>> {
    ctx.connection.clone().ok_or_else(|| AppError::Internal("DockerRuntime requires a connection".into()))
}

/// A raw newline in an image ref, command argument, or environment value
/// could inject an extra shell statement into the generated `docker
/// create` command - rejected outright, same stance the other two SSH
/// runtimes take.
fn reject_newlines(value: &str, field: &str) -> AppResult<()> {
    if value.contains('\n') || value.contains('\r') {
        return Err(AppError::InvalidInput(format!("{field} can't contain a newline")));
    }
    Ok(())
}

/// POSIX single-quote shell escaping - see `runtime::remote_process`'s copy
/// of the same function for the full reasoning; duplicated rather than
/// shared, matching how `ssh::docker`/`ssh::systemd` don't share code
/// despite a similar shape either.
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

/// POSIX environment variable name rule - also guards against a key
/// containing `=`, which `docker create -e` would otherwise misparse.
fn is_valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn validate_environment(environment: &[EnvironmentVariable]) -> AppResult<()> {
    for env in environment {
        if !is_valid_env_key(&env.key) {
            return Err(AppError::InvalidInput(format!("'{}' isn't a valid environment variable name", env.key)));
        }
        reject_newlines(&env.value, &format!("the '{}' environment variable", env.key))?;
    }
    Ok(())
}

async fn container_exists(connection: &SshSession, name: &str) -> AppResult<bool> {
    validate_container_ref(name)?;
    let output = connection.execute_command(&format!("docker inspect {name} >/dev/null 2>&1")).await?;
    Ok(output.exit_code == 0)
}

/// Pure command-string construction, separated from `create_container`'s
/// actual SSH exec so the resource-limit flag placement can be unit tested
/// without a live connection - same split `runtime::systemd::render_unit_file`
/// already uses for the same reason.
fn build_create_command(ctx: &RuntimeContext<'_>, config: &DockerConfig, name: &str) -> AppResult<String> {
    validate_container_ref(name)?;
    reject_newlines(&config.image, "the image")?;
    for arg in &config.command {
        reject_newlines(arg, "a command argument")?;
    }
    validate_environment(ctx.environment)?;
    validate_resource_limits(config.memory_limit_mb, config.cpu_limit_cores)?;
    let restart_policy = restart_policy_or_default(config)?;
    reject_newlines(&ctx.application.working_directory, "the working directory")?;
    for port in ctx.ports {
        reject_newlines(&port.bind_address, "a port's bind address")?;
    }

    let working_directory = shell_quote(&ctx.application.working_directory);
    let mut command = format!(
        "docker create --name {} --restart {} -v {working_directory}:{working_directory} -w {working_directory} ",
        shell_quote(name),
        restart_policy,
    );
    if let Some(mb) = config.memory_limit_mb {
        command.push_str(&format!("--memory {mb}m "));
    }
    if let Some(cores) = config.cpu_limit_cores {
        command.push_str(&format!("--cpus {cores} "));
    }
    for port in ctx.ports {
        // `external_port` unset means "declared but not meant to be
        // reachable from outside the container's own network" (e.g. a
        // database another container reaches internally) - see
        // `ApplicationPort::external_port`'s own doc comment. Only publish
        // the ones that actually asked for it.
        let Some(external_port) = port.external_port else { continue };
        let proto = match port.protocol {
            PortProtocol::Tcp => "tcp",
            PortProtocol::Udp => "udp",
        };
        command.push_str(&format!("-p {} ", shell_quote(&format!("{}:{external_port}:{}/{proto}", port.bind_address, port.internal_port))));
    }
    for env in ctx.environment {
        command.push_str(&format!("-e {}={} ", env.key, shell_quote(&env.value)));
    }
    command.push_str(&shell_quote(&config.image));
    for arg in &config.command {
        command.push(' ');
        command.push_str(&shell_quote(arg));
    }
    Ok(command)
}

async fn create_container(connection: &SshSession, ctx: &RuntimeContext<'_>, config: &DockerConfig, name: &str) -> AppResult<()> {
    let command = build_create_command(ctx, config, name)?;
    let output = connection.execute_command(&command).await?;
    if output.exit_code != 0 {
        let detail = output.stderr.trim();
        let detail = if detail.is_empty() { "docker create failed".to_string() } else { detail.to_string() };
        return Err(AppError::Connection(format!("couldn't create the container: {detail}")));
    }
    Ok(())
}

/// `.State.Status` alone can't distinguish a clean stop from a crash - both
/// are `"exited"`. Combined with `.State.ExitCode` (fetched in the same
/// `docker inspect` call), this is a genuinely more reliable Failed signal
/// than `runtime::remote_process` can offer for a plain `nohup`'d process -
/// a real capability difference between runtimes, not something to flatten
/// away (brief's own "nie udawaj identycznych capabilities").
fn map_container_status(status: &str, exit_code: &str) -> ApplicationStatus {
    match status {
        "running" => ApplicationStatus::Running,
        "restarting" => ApplicationStatus::Starting,
        "removing" | "paused" => ApplicationStatus::Stopping,
        "dead" => ApplicationStatus::Failed,
        "exited" if exit_code != "0" => ApplicationStatus::Failed,
        // "exited" with code 0, "created" (made but never started), or
        // anything unrecognized (including the container not existing yet)
        // - Stopped is the honest default.
        _ => ApplicationStatus::Stopped,
    }
}

/// Parses `docker inspect --format '{{.State.Status}}|{{.State.ExitCode}}'`.
fn parse_status_output(output: &str) -> (String, String) {
    let mut fields = output.trim().splitn(2, '|');
    let status = fields.next().unwrap_or("").to_string();
    let exit_code = fields.next().unwrap_or("").trim().to_string();
    (status, exit_code)
}

/// Parses `docker stats --no-stream --format '{{.CPUPerc}}|{{.MemUsage}}'` -
/// e.g. `"1.23%|45.6MiB / 512MiB"`.
fn parse_stats_output(output: &str) -> (Option<f32>, Option<u64>) {
    let mut fields = output.trim().splitn(2, '|');
    let cpu_percent = fields.next().and_then(|f| f.trim().trim_end_matches('%').parse::<f32>().ok());
    let ram_bytes = fields.next().and_then(|mem| mem.split('/').next()).and_then(|used| parse_docker_byte_size(used.trim()));
    (cpu_percent, ram_bytes)
}

/// Docker's `go-units.BytesSize` formatting - a number immediately followed
/// by a unit suffix, no space between them (`"45.6MiB"`). IEC units
/// (`KiB`/`MiB`/`GiB`/`TiB`) are what modern Docker actually emits; decimal
/// units (`kB`/`MB`/`GB`) are accepted too since older/differently
/// configured builds have used them.
fn parse_docker_byte_size(value: &str) -> Option<u64> {
    let split_at = value.find(|c: char| !c.is_ascii_digit() && c != '.')?;
    let (number, unit) = value.split_at(split_at);
    let number: f64 = number.parse().ok()?;
    let multiplier = match unit {
        "B" => 1.0,
        "KiB" => 1024.0,
        "MiB" => 1024.0 * 1024.0,
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        "TiB" => 1024.0_f64.powi(4),
        "kB" => 1000.0,
        "MB" => 1000.0 * 1000.0,
        "GB" => 1000.0 * 1000.0 * 1000.0,
        _ => return None,
    };
    Some((number * multiplier) as u64)
}

/// `.State.StartedAt` is always RFC3339 - unlike `runtime::systemd`'s
/// `ActiveEnterTimestamp` (skipped there for being locale/timezone-
/// dependent), this is a standard, unambiguous format `chrono` parses
/// directly.
fn parse_started_at(value: &str) -> Option<u64> {
    let started_at = DateTime::parse_from_rfc3339(value.trim()).ok()?;
    let elapsed = Utc::now().signed_duration_since(started_at.with_timezone(&Utc));
    u64::try_from(elapsed.num_seconds()).ok()
}

pub struct DockerRuntime;

impl DockerRuntime {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DockerRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ApplicationRuntime for DockerRuntime {
    async fn validate(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        let connection = connection_ref(ctx)?;
        let config = parse_config(ctx)?;
        if config.image.trim().is_empty() {
            return Err(AppError::InvalidInput("no image configured for this application".into()));
        }
        reject_newlines(&config.image, "the image")?;
        for arg in &config.command {
            reject_newlines(arg, "a command argument")?;
        }
        validate_environment(ctx.environment)?;

        let output = connection.execute_command("docker version --format '{{.Server.Version}}' 2>&1").await?;
        if output.exit_code != 0 {
            return Err(AppError::InvalidInput("Docker doesn't seem to be available on this host".into()));
        }
        Ok(())
    }

    async fn start(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        let connection = connection_ref(ctx)?;
        let config = parse_config(ctx)?;
        let name = container_name(ctx.application.id);

        if !container_exists(connection, &name).await? {
            create_container(connection, ctx, &config, &name).await?;
        }
        connection.start_container(&name).await
    }

    /// `docker stop` is already a graceful stop by construction (SIGTERM,
    /// then SIGKILL after its own timeout) - same reasoning
    /// `runtime::systemd::stop` documents for why `graceful` doesn't map to
    /// anything further here; the real graceful/immediate split is
    /// `stop()` vs `kill()`.
    async fn stop(&self, ctx: &RuntimeContext<'_>, _graceful: bool) -> AppResult<()> {
        let connection = connection_ref(ctx)?;
        let name = container_name(ctx.application.id);
        if !container_exists(connection, &name).await? {
            return Err(AppError::InvalidInput("this application isn't running".into()));
        }
        connection.stop_container(&name).await
    }

    /// Restarts the existing container in place - does not recreate it
    /// (see the module doc comment). Falls back to `start()` if nothing has
    /// been created yet, same as the other two SSH runtimes.
    async fn restart(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        let connection = connection_ref(ctx)?;
        let name = container_name(ctx.application.id);
        if container_exists(connection, &name).await? {
            connection.restart_container(&name).await
        } else {
            self.start(ctx).await
        }
    }

    async fn kill(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        let connection = connection_ref(ctx)?;
        let name = container_name(ctx.application.id);
        if !container_exists(connection, &name).await? {
            return Err(AppError::InvalidInput("this application isn't running".into()));
        }
        connection.kill_container(&name).await
    }

    async fn status(&self, ctx: &RuntimeContext<'_>) -> AppResult<ApplicationStatus> {
        let connection = connection_ref(ctx)?;
        let name = container_name(ctx.application.id);
        validate_container_ref(&name)?;
        let output = connection
            .execute_command(&format!("docker inspect --format '{{{{.State.Status}}}}|{{{{.State.ExitCode}}}}' {name} 2>/dev/null"))
            .await?;
        if output.exit_code != 0 {
            // Never created, or removed - Stopped, not an error: the same
            // "no other place it could be running" reasoning the other two
            // SSH runtimes use.
            return Ok(ApplicationStatus::Stopped);
        }
        let (status, exit_code) = parse_status_output(&output.stdout);
        Ok(map_container_status(&status, &exit_code))
    }

    async fn resource_usage(&self, ctx: &RuntimeContext<'_>) -> AppResult<ResourceUsage> {
        let empty = ResourceUsage { cpu_percent: None, ram_bytes: None, uptime_seconds: None };
        let connection = connection_ref(ctx)?;
        let name = container_name(ctx.application.id);
        validate_container_ref(&name)?;

        let stats_output =
            connection.execute_command(&format!("docker stats --no-stream --format '{{{{.CPUPerc}}}}|{{{{.MemUsage}}}}' {name} 2>/dev/null")).await?;
        if stats_output.exit_code != 0 {
            return Ok(empty);
        }
        let (cpu_percent, ram_bytes) = parse_stats_output(&stats_output.stdout);

        let started_output = connection.execute_command(&format!("docker inspect --format '{{{{.State.StartedAt}}}}' {name} 2>/dev/null")).await?;
        let uptime_seconds = parse_started_at(&started_output.stdout);

        Ok(ResourceUsage { cpu_percent, ram_bytes, uptime_seconds })
    }

    async fn health_check(&self, ctx: &RuntimeContext<'_>, spec: &HealthCheckSpec) -> AppResult<HealthStatus> {
        let status = self.status(ctx).await?;
        health_check::default_health_check(ctx, spec, status, Some("the container exited with a non-zero status")).await
    }

    async fn console(&self, _ctx: &RuntimeContext<'_>) -> AppResult<Option<Box<dyn ApplicationConsole>>> {
        // Containers created here don't get `-i` (see the module doc
        // comment on scope) - this is the trait's own documented example
        // of a legitimate `None` ("a Docker container started without
        // -i" - see `ApplicationConsole`'s doc comment).
        Ok(None)
    }

    async fn logs(&self, ctx: &RuntimeContext<'_>) -> AppResult<Box<dyn LogProvider>> {
        let connection = connection_arc(ctx)?;
        Ok(Box::new(DockerLogs { connection, name: container_name(ctx.application.id) }))
    }

    /// `docker rm -f` - stops (if running) and removes in one step, same as
    /// the CLI's own `-f` semantics. A no-op, not an error, when nothing was
    /// ever created (e.g. "Recreate Container" clicked before the first
    /// successful start) - `destroy()` guarantees "no container with this
    /// name exists after this returns Ok", not "a container existed before
    /// this ran".
    async fn destroy(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        let connection = connection_ref(ctx)?;
        let name = container_name(ctx.application.id);
        validate_container_ref(&name)?;
        if !container_exists(connection, &name).await? {
            return Ok(());
        }
        let output = connection.execute_command(&format!("docker rm -f {}", shell_quote(&name))).await?;
        if output.exit_code != 0 {
            let detail = output.stderr.trim();
            let detail = if detail.is_empty() { "docker rm failed".to_string() } else { detail.to_string() };
            return Err(AppError::Connection(format!("couldn't remove the container: {detail}")));
        }
        Ok(())
    }
}

struct DockerLogs {
    connection: Arc<SshSession>,
    name: String,
}

#[async_trait::async_trait]
impl LogProvider for DockerLogs {
    async fn tail(&self, max_lines: u32) -> AppResult<Vec<String>> {
        // Reuses `container_logs` verbatim - it already exists for the
        // Quick Actions Docker tab, and `Applications` needs the exact same
        // thing.
        let logs = self.connection.container_logs(&self.name, max_lines).await?;
        Ok(logs.lines().map(str::to_string).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Application, ApplicationPort, HealthCheckType, RuntimeType};

    fn stub_application(id: Uuid) -> Application {
        Application {
            id,
            server_id: Some(Uuid::new_v4()),
            name: "My App".to_string(),
            description: None,
            blueprint_id: "generic".to_string(),
            blueprint_version: 1,
            runtime_type: RuntimeType::Docker,
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
    fn container_name_is_stable_and_namespaced() {
        let id = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        assert_eq!(container_name(id), "vibessh-app-11111111-2222-3333-4444-555555555555");
        assert!(validate_container_ref(&container_name(id)).is_ok());
    }

    #[test]
    fn is_valid_env_key_rejects_equals_and_leading_digits() {
        assert!(is_valid_env_key("PORT"));
        assert!(!is_valid_env_key("PORT=8080"));
        assert!(!is_valid_env_key("8080PORT"));
    }

    #[test]
    fn map_container_status_distinguishes_a_clean_exit_from_a_crash() {
        assert_eq!(map_container_status("running", "0"), ApplicationStatus::Running);
        assert_eq!(map_container_status("exited", "0"), ApplicationStatus::Stopped);
        assert_eq!(map_container_status("exited", "1"), ApplicationStatus::Failed);
        assert_eq!(map_container_status("dead", "137"), ApplicationStatus::Failed);
        assert_eq!(map_container_status("restarting", "0"), ApplicationStatus::Starting);
        assert_eq!(map_container_status("created", ""), ApplicationStatus::Stopped);
    }

    #[test]
    fn parse_status_output_splits_on_the_pipe() {
        assert_eq!(parse_status_output("exited|137\n"), ("exited".to_string(), "137".to_string()));
        assert_eq!(parse_status_output("running|0"), ("running".to_string(), "0".to_string()));
    }

    #[test]
    fn parse_docker_byte_size_handles_iec_and_decimal_units() {
        assert_eq!(parse_docker_byte_size("45.6MiB"), Some((45.6 * 1024.0 * 1024.0) as u64));
        assert_eq!(parse_docker_byte_size("512B"), Some(512));
        assert_eq!(parse_docker_byte_size("1.5GiB"), Some((1.5 * 1024.0 * 1024.0 * 1024.0) as u64));
        assert_eq!(parse_docker_byte_size("bogus"), None);
    }

    #[test]
    fn parse_stats_output_reads_cpu_and_the_used_side_of_mem_usage() {
        let (cpu, ram) = parse_stats_output("1.23%|45.6MiB / 512MiB\n");
        assert_eq!(cpu, Some(1.23));
        assert_eq!(ram, Some((45.6 * 1024.0 * 1024.0) as u64));
    }

    #[test]
    fn parse_started_at_reads_a_real_rfc3339_timestamp() {
        let five_minutes_ago = Utc::now() - chrono::Duration::minutes(5);
        let formatted = five_minutes_ago.to_rfc3339();
        let uptime = parse_started_at(&formatted).unwrap();
        assert!((295..=310).contains(&uptime), "expected roughly 300s, got {uptime}");
        assert_eq!(parse_started_at("not-a-timestamp"), None);
    }

    #[test]
    fn reject_newlines_rejects_embedded_newlines_and_accepts_plain_text() {
        assert!(reject_newlines("alpine:latest", "the image").is_ok());
        assert!(reject_newlines("alpine\nrm -rf /", "the image").is_err());
    }

    #[test]
    fn validate_environment_rejects_a_bad_key_but_accepts_a_good_one() {
        assert!(validate_environment(&[EnvironmentVariable { key: "PORT".into(), value: "25565".into() }]).is_ok());
        assert!(validate_environment(&[EnvironmentVariable { key: "NOT VALID".into(), value: "x".into() }]).is_err());
    }

    #[test]
    fn build_create_command_includes_memory_and_cpu_flags_before_the_image_when_set() {
        let application = stub_application(Uuid::new_v4());
        let config = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: Some(512), cpu_limit_cores: Some(1.5), restart_policy: None };
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], connection: None };

        let command = build_create_command(&ctx, &config, "vibessh-app-test").unwrap();
        assert!(command.contains("--memory 512m"), "{command}");
        assert!(command.contains("--cpus 1.5"), "{command}");
        assert!(command.find("--memory").unwrap() < command.find("alpine:latest").unwrap());
    }

    #[test]
    fn build_create_command_bind_mounts_and_sets_the_workdir_to_the_working_directory() {
        let application = stub_application(Uuid::new_v4());
        let config = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: None };
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], connection: None };

        let command = build_create_command(&ctx, &config, "vibessh-app-test").unwrap();
        assert!(command.contains("-v '/srv/my-app':'/srv/my-app'"), "{command}");
        assert!(command.contains("-w '/srv/my-app'"), "{command}");
        assert!(command.find("-v").unwrap() < command.find("alpine:latest").unwrap());
    }

    #[test]
    fn build_create_command_defaults_restart_policy_to_unless_stopped() {
        let application = stub_application(Uuid::new_v4());
        let config = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: None };
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], connection: None };

        let command = build_create_command(&ctx, &config, "vibessh-app-test").unwrap();
        assert!(command.contains("--restart unless-stopped"), "{command}");
    }

    #[test]
    fn build_create_command_honors_an_explicit_restart_policy_and_rejects_an_invalid_one() {
        let application = stub_application(Uuid::new_v4());
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], connection: None };

        let always = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: Some("always".into()) };
        let command = build_create_command(&ctx, &always, "vibessh-app-test").unwrap();
        assert!(command.contains("--restart always"), "{command}");

        let bogus = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: Some("whenever".into()) };
        assert!(build_create_command(&ctx, &bogus, "vibessh-app-test").is_err());
    }

    fn stub_port(protocol: PortProtocol, bind_address: &str, internal_port: u16, external_port: Option<u16>) -> ApplicationPort {
        ApplicationPort {
            id: Uuid::new_v4(),
            application_id: Uuid::new_v4(),
            name: "game".to_string(),
            protocol,
            bind_address: bind_address.to_string(),
            internal_port,
            external_port,
            visibility: crate::models::PortVisibility::Public,
            required: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn build_create_command_publishes_only_ports_with_an_external_port_set() {
        let application = stub_application(Uuid::new_v4());
        let config = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: None };
        let runtime_config = serde_json::json!({});
        let ports = vec![
            stub_port(PortProtocol::Tcp, "0.0.0.0", 25565, Some(25565)),
            stub_port(PortProtocol::Udp, "0.0.0.0", 24454, Some(24454)),
            stub_port(PortProtocol::Tcp, "127.0.0.1", 3306, None),
        ];
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &ports, connection: None };

        let command = build_create_command(&ctx, &config, "vibessh-app-test").unwrap();
        assert!(command.contains("-p '0.0.0.0:25565:25565/tcp'"), "{command}");
        assert!(command.contains("-p '0.0.0.0:24454:24454/udp'"), "{command}");
        // The port with no external_port must not be published at all.
        assert!(!command.contains("3306"), "{command}");
        assert!(command.find("-p").unwrap() < command.find("alpine:latest").unwrap());
    }

    #[test]
    fn build_create_command_publishes_nothing_when_no_ports_are_declared() {
        let application = stub_application(Uuid::new_v4());
        let config = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: None };
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], connection: None };

        let command = build_create_command(&ctx, &config, "vibessh-app-test").unwrap();
        assert!(!command.contains("-p "));
    }

    #[test]
    fn build_create_command_rejects_a_newline_in_a_ports_bind_address() {
        let application = stub_application(Uuid::new_v4());
        let config = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: None };
        let runtime_config = serde_json::json!({});
        let ports = vec![stub_port(PortProtocol::Tcp, "0.0.0.0\nrm -rf /", 25565, Some(25565))];
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &ports, connection: None };

        assert!(build_create_command(&ctx, &config, "vibessh-app-test").is_err());
    }

    #[test]
    fn build_create_command_omits_limit_flags_when_unset() {
        let application = stub_application(Uuid::new_v4());
        let config = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: None };
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], connection: None };

        let command = build_create_command(&ctx, &config, "vibessh-app-test").unwrap();
        assert!(!command.contains("--memory"));
        assert!(!command.contains("--cpus"));
    }

    #[test]
    fn build_create_command_rejects_a_zero_memory_limit_or_non_positive_cpu_limit() {
        let application = stub_application(Uuid::new_v4());
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], connection: None };

        let zero_memory = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: Some(0), cpu_limit_cores: None, restart_policy: None };
        assert!(build_create_command(&ctx, &zero_memory, "vibessh-app-test").is_err());

        let negative_cpu = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: Some(-1.0), restart_policy: None };
        assert!(build_create_command(&ctx, &negative_cpu, "vibessh-app-test").is_err());
    }

    #[tokio::test]
    async fn methods_that_need_a_connection_fail_cleanly_without_one() {
        let application = stub_application(Uuid::new_v4());
        let config = serde_json::json!({ "image": "alpine:latest", "command": [] });
        let ctx = RuntimeContext { application: &application, runtime_config: &config, environment: &[], ports: &[], connection: None };
        let runtime = DockerRuntime::new();

        assert!(matches!(runtime.validate(&ctx).await, Err(AppError::Internal(_))));
        assert!(matches!(runtime.start(&ctx).await, Err(AppError::Internal(_))));
        assert!(matches!(runtime.status(&ctx).await, Err(AppError::Internal(_))));
        assert!(matches!(runtime.logs(&ctx).await, Err(AppError::Internal(_))));
        assert!(matches!(runtime.destroy(&ctx).await, Err(AppError::Internal(_))));
    }
}
