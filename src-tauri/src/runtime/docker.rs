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
//! **Deliberately out of scope for this phase, not forgotten**:
//! - **Port publishing** (`-p host:container`) - `RuntimeContext` doesn't
//!   carry `ApplicationPort` rows (only `environment`, added in Phase 2 for
//!   the same kind of reason), and the architecture doc's own roadmap
//!   assigns full port CRUD/collision-handling to Phase 10. Containers
//!   created here publish nothing; reaching one from outside the host needs
//!   that later work.
//! - **Volumes / bind mounts** - same reasoning as ports: nothing here
//!   mounts `working_directory` or anything else into the container. This
//!   also shapes `start()`'s own behavior below (recreate-avoidance): with
//!   no volume, a container's writable layer is the *only* place its own
//!   state (a world save, a database's files, ...) lives.
//!
//! **Unlike `runtime::systemd`/`runtime::remote_process`, `start()` does
//! NOT unconditionally recreate.** Those runtimes persist only *config*
//! (a unit file, a shell command) that's always safe to regenerate. A
//! Docker container's writable layer can hold real application state -
//! recreating it on every start would destroy that. So `start()` only runs
//! `docker create` when no container with this application's name exists
//! yet; picking up an edited image/command requires the container to be
//! removed first, which isn't exposed by the `ApplicationRuntime` trait
//! (the same "no `destroy()`" gap noted in the other two SSH runtimes).

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{ApplicationStatus, EnvironmentVariable};
use crate::ssh::docker::validate_container_ref;
use crate::ssh::SshSession;

use super::{ApplicationConsole, ApplicationRuntime, HealthStatus, LogProvider, ResourceUsage, RuntimeContext};

/// What `runtime_config` deserializes into for `RuntimeType::Docker`.
/// `command`, if given, overrides the image's own `ENTRYPOINT`/`CMD` - a
/// container created without one just runs the image as authored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerConfig {
    pub image: String,
    #[serde(default)]
    pub command: Vec<String>,
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

async fn create_container(connection: &SshSession, ctx: &RuntimeContext<'_>, config: &DockerConfig, name: &str) -> AppResult<()> {
    validate_container_ref(name)?;
    reject_newlines(&config.image, "the image")?;
    for arg in &config.command {
        reject_newlines(arg, "a command argument")?;
    }
    validate_environment(ctx.environment)?;

    let mut command = format!("docker create --name {} ", shell_quote(name));
    for env in ctx.environment {
        command.push_str(&format!("-e {}={} ", env.key, shell_quote(&env.value)));
    }
    command.push_str(&shell_quote(&config.image));
    for arg in &config.command {
        command.push(' ');
        command.push_str(&shell_quote(arg));
    }

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

    async fn health_check(&self, ctx: &RuntimeContext<'_>) -> AppResult<HealthStatus> {
        match self.status(ctx).await? {
            ApplicationStatus::Running => Ok(HealthStatus::Healthy),
            ApplicationStatus::Failed => Ok(HealthStatus::Unhealthy("the container exited with a non-zero status".into())),
            _ => Ok(HealthStatus::Unknown),
        }
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
    use crate::models::{Application, RuntimeType};

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

    #[tokio::test]
    async fn methods_that_need_a_connection_fail_cleanly_without_one() {
        let application = stub_application(Uuid::new_v4());
        let config = serde_json::json!({ "image": "alpine:latest", "command": [] });
        let ctx = RuntimeContext { application: &application, runtime_config: &config, environment: &[], connection: None };
        let runtime = DockerRuntime::new();

        assert!(matches!(runtime.validate(&ctx).await, Err(AppError::Internal(_))));
        assert!(matches!(runtime.start(&ctx).await, Err(AppError::Internal(_))));
        assert!(matches!(runtime.status(&ctx).await, Err(AppError::Internal(_))));
        assert!(matches!(runtime.logs(&ctx).await, Err(AppError::Internal(_))));
    }
}
