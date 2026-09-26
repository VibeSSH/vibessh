//! Somebody else's Application, run from a team member's own install.
//!
//! A member connects to the Node as their own account (`vibessh-m-...`), whose
//! sudo rules the owner's access sync wrote from what they were given - see
//! `member_sudoers`. Those rules name exact commands: `docker restart
//! vibessh-app-<id>`, not `docker restart` with anything after it. So this
//! runtime runs exactly those commands and nothing else. `DockerRuntime`'s
//! own `start`, say, also provisions the Application's account, repairs file
//! ownership, joins networks and attaches the console - every one of which
//! the member's account may not do, and none of which is theirs to do.
//!
//! Every command goes through `sudo -n`, so a rule that is not there fails at
//! once instead of waiting for a password nobody will type, and that refusal
//! comes back as `SharedActionNotAllowed` - "ask the owner for this" - rather
//! than as sudo's own words.
//!
//! What a member can never do here - recreate, reinstall, delete, change the
//! configuration - is not in this runtime at all. Those are the owner's.

use std::sync::Arc;

use crate::errors::{AppError, AppResult};
use crate::member_sudoers::CONSOLE_WRITER_PATH;
use crate::models::{ApplicationStatus, RuntimeType, SharedAccess};
use crate::ssh::command::quote as shell_quote;
use crate::ssh::SshSession;
use crate::transport::CommandOutput;

use super::{ApplicationConsole, ApplicationRuntime, HealthCheckSpec, HealthStatus, LogProvider, ResourceUsage, RuntimeContext};

pub struct MemberRuntime {
    access: SharedAccess,
}

impl MemberRuntime {
    pub fn new(access: SharedAccess) -> Self {
        Self { access }
    }

    /// Refuses before anything is sent when this install already knows the
    /// grant does not cover it - the Node would refuse too, one round trip
    /// later.
    fn require(&self, permission: &str) -> AppResult<()> {
        if self.access.allows(permission) {
            Ok(())
        } else {
            Err(AppError::SharedActionNotAllowed { action: permission.to_string() })
        }
    }
}

/// What the rules name for this Application.
enum Target {
    Container(String),
    Unit(String),
}

fn target(ctx: &RuntimeContext<'_>) -> AppResult<Target> {
    let id = ctx.application.id;
    match ctx.application.runtime_type {
        RuntimeType::Docker => Ok(Target::Container(format!("vibessh-app-{id}"))),
        RuntimeType::Systemd => Ok(Target::Unit(format!("vibessh-app-{id}.service"))),
        _ => Err(AppError::InvalidInput("a shared application has to run in Docker or as a systemd service".into())),
    }
}

fn connection<'a>(ctx: &'a RuntimeContext<'_>) -> AppResult<&'a Arc<SshSession>> {
    ctx.connection.as_ref().ok_or_else(|| AppError::Internal("a shared application's runtime requires a connection".into()))
}

/// Whether sudo said no, as opposed to the command itself failing.
///
/// `sudo -n` without a matching rule prints "a password is required"; a rule
/// that exists for other arguments prints "is not allowed to execute". Both
/// mean the same thing here: the grant does not cover it.
fn refused_by_sudo(output: &CommandOutput) -> bool {
    output.exit_code != 0
        && (output.stderr.contains("a password is required")
            || output.stderr.contains("is not allowed to")
            || output.stderr.contains("may not run sudo"))
}

/// Runs one command as the rules allow it: `sudo -n`, every argument quoted.
async fn sudo(connection: &SshSession, arguments: &[&str], action: &str) -> AppResult<CommandOutput> {
    let command = format!("sudo -n {}", arguments.iter().map(|argument| shell_quote(argument)).collect::<Vec<_>>().join(" "));
    let output = connection.execute_command(&command).await?;
    if refused_by_sudo(&output) {
        return Err(AppError::SharedActionNotAllowed { action: action.to_string() });
    }
    Ok(output)
}

fn expect_success(output: CommandOutput, what: &str) -> AppResult<()> {
    if output.exit_code == 0 {
        return Ok(());
    }
    let detail = output.stderr.trim();
    Err(AppError::Connection(if detail.is_empty() { format!("couldn't {what}") } else { format!("couldn't {what}: {detail}") }))
}

/// The state `docker inspect` reports, from its JSON. The rule allows
/// `inspect <name>` exactly, so there is no `--format` to ask for one field.
fn parse_inspect(stdout: &str) -> Option<(String, String, Option<String>)> {
    let value: serde_json::Value = serde_json::from_str(stdout).ok()?;
    let state = value.get(0)?.get("State")?;
    let status = state.get("Status")?.as_str()?.to_string();
    let exit_code = state.get("ExitCode").and_then(serde_json::Value::as_i64).unwrap_or(0).to_string();
    let started_at = state.get("StartedAt").and_then(serde_json::Value::as_str).map(str::to_string);
    Some((status, exit_code, started_at))
}

/// `systemctl status`'s `Active:` line, as a status.
fn parse_unit_status(stdout: &str) -> ApplicationStatus {
    let active = stdout.lines().map(str::trim).find_map(|line| line.strip_prefix("Active:")).unwrap_or("").trim();
    if active.starts_with("active") {
        ApplicationStatus::Running
    } else if active.starts_with("activating") || active.starts_with("reloading") {
        ApplicationStatus::Starting
    } else if active.starts_with("deactivating") {
        ApplicationStatus::Stopping
    } else if active.starts_with("failed") {
        ApplicationStatus::Failed
    } else {
        ApplicationStatus::Stopped
    }
}

#[async_trait::async_trait]
impl ApplicationRuntime for MemberRuntime {
    async fn validate(&self, _ctx: &RuntimeContext<'_>) -> AppResult<()> {
        Ok(())
    }

    async fn start(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        self.require("applications.lifecycle")?;
        let connection = connection(ctx)?;
        let output = match target(ctx)? {
            Target::Container(name) => sudo(connection, &["docker", "start", &name], "applications.lifecycle").await?,
            Target::Unit(unit) => sudo(connection, &["systemctl", "start", &unit], "applications.lifecycle").await?,
        };
        expect_success(output, "start the application")
    }

    async fn stop(&self, ctx: &RuntimeContext<'_>, _graceful: bool) -> AppResult<()> {
        self.require("applications.lifecycle")?;
        let connection = connection(ctx)?;
        let output = match target(ctx)? {
            Target::Container(name) => sudo(connection, &["docker", "stop", &name], "applications.lifecycle").await?,
            Target::Unit(unit) => sudo(connection, &["systemctl", "stop", &unit], "applications.lifecycle").await?,
        };
        expect_success(output, "stop the application")
    }

    async fn restart(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        self.require("applications.lifecycle")?;
        let connection = connection(ctx)?;
        let output = match target(ctx)? {
            Target::Container(name) => sudo(connection, &["docker", "restart", &name], "applications.lifecycle").await?,
            Target::Unit(unit) => sudo(connection, &["systemctl", "restart", &unit], "applications.lifecycle").await?,
        };
        expect_success(output, "restart the application")
    }

    async fn kill(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        self.require("applications.lifecycle")?;
        let connection = connection(ctx)?;
        match target(ctx)? {
            Target::Container(name) => expect_success(sudo(connection, &["docker", "kill", &name], "applications.lifecycle").await?, "kill the application"),
            // No rule names `systemctl kill`: stopping a unit is its forceful
            // end already, once its own timeout runs out.
            Target::Unit(_) => Err(AppError::SharedActionNotAllowed { action: "applications.lifecycle".into() }),
        }
    }

    async fn status(&self, ctx: &RuntimeContext<'_>) -> AppResult<ApplicationStatus> {
        let connection = connection(ctx)?;
        match target(ctx)? {
            Target::Container(name) => {
                let output = sudo(connection, &["docker", "inspect", &name], "applications.view").await?;
                if output.exit_code != 0 {
                    // No such container: removed by its owner, or never made.
                    return Ok(ApplicationStatus::Stopped);
                }
                Ok(parse_inspect(&output.stdout)
                    .map(|(status, exit_code, _)| super::docker::map_container_status(&status, &exit_code))
                    .unwrap_or(ApplicationStatus::Unknown))
            }
            Target::Unit(unit) => {
                // `systemctl status` exits 3 for a stopped unit, which is an
                // answer rather than a failure - only sudo's refusal is one.
                let output = sudo(connection, &["systemctl", "status", "--no-pager", &unit], "applications.view").await?;
                Ok(parse_unit_status(&output.stdout))
            }
        }
    }

    /// Uptime only. CPU and memory come from `docker stats`, which takes
    /// several containers and so has no rule a member could be given for
    /// one of them.
    async fn resource_usage(&self, ctx: &RuntimeContext<'_>) -> AppResult<ResourceUsage> {
        let empty = ResourceUsage { cpu_percent: None, ram_bytes: None, uptime_seconds: None };
        let Target::Container(name) = target(ctx)? else { return Ok(empty) };
        let output = sudo(connection(ctx)?, &["docker", "inspect", &name], "applications.view").await?;
        let Some((status, _, Some(started_at))) = parse_inspect(&output.stdout) else { return Ok(empty) };
        if status != "running" {
            return Ok(empty);
        }
        let uptime_seconds = chrono::DateTime::parse_from_rfc3339(&started_at)
            .ok()
            .and_then(|started| u64::try_from((chrono::Utc::now() - started.with_timezone(&chrono::Utc)).num_seconds()).ok());
        Ok(ResourceUsage { uptime_seconds, ..empty })
    }

    async fn health_check(&self, ctx: &RuntimeContext<'_>, _spec: &HealthCheckSpec) -> AppResult<HealthStatus> {
        Ok(match self.status(ctx).await? {
            ApplicationStatus::Running => HealthStatus::Unknown,
            _ => HealthStatus::Unhealthy { reason: "the application is not running".into() },
        })
    }

    async fn console(&self, ctx: &RuntimeContext<'_>) -> AppResult<Option<Box<dyn ApplicationConsole>>> {
        let Target::Container(_) = target(ctx)? else { return Ok(None) };
        Ok(Some(Box::new(MemberConsole {
            connection: connection(ctx)?.clone(),
            application_id: ctx.application.id,
            can_write: self.access.allows("applications.console"),
        })))
    }

    async fn logs(&self, ctx: &RuntimeContext<'_>) -> AppResult<Box<dyn LogProvider>> {
        match target(ctx)? {
            Target::Container(name) => Ok(Box::new(MemberLogs { connection: connection(ctx)?.clone(), name })),
            // `journalctl -u <unit>` takes several units too, so it has no
            // rule for one of them either.
            Target::Unit(_) => Err(AppError::SharedActionNotAllowed { action: "applications.view".into() }),
        }
    }

    async fn destroy(&self, _ctx: &RuntimeContext<'_>) -> AppResult<()> {
        Err(AppError::SharedActionNotAllowed { action: "applications.config".into() })
    }
}

struct MemberLogs {
    connection: Arc<SshSession>,
    name: String,
}

#[async_trait::async_trait]
impl LogProvider for MemberLogs {
    async fn tail(&self, max_lines: u32) -> AppResult<Vec<String>> {
        // Both streams: a server's errors are on stderr. The redirect is the
        // shell's, so sudo still sees exactly `docker logs --tail N
        // --timestamps <name>`, which `logs * <name>` allows.
        let command = format!(
            "sudo -n docker logs --tail {max_lines} --timestamps {name} 2>&1",
            name = shell_quote(&self.name),
        );
        let output = self.connection.execute_command(&command).await?;
        if refused_by_sudo(&output) || output.stdout.contains("a password is required") {
            return Err(AppError::SharedActionNotAllowed { action: "applications.view".into() });
        }
        if output.exit_code != 0 {
            let detail = output.stdout.trim();
            return Err(AppError::Connection(if detail.is_empty() { "docker logs failed".to_string() } else { detail.to_string() }));
        }
        Ok(output.stdout.lines().map(str::to_string).collect())
    }
}

struct MemberConsole {
    connection: Arc<SshSession>,
    application_id: uuid::Uuid,
    can_write: bool,
}

#[async_trait::async_trait]
impl ApplicationConsole for MemberConsole {
    /// Through the console writer, with the line on stdin: nothing typed
    /// into a console is ever part of a command.
    async fn write(&self, input: &str) -> AppResult<()> {
        if !self.can_write {
            return Err(AppError::SharedActionNotAllowed { action: "applications.console".into() });
        }
        let command = format!("sudo -n {} {}", shell_quote(CONSOLE_WRITER_PATH), self.application_id);
        let line = format!("{}\n", input.lines().next().unwrap_or(""));
        let output = self.connection.execute_command_with_input(&command, line.as_bytes()).await?;
        if refused_by_sudo(&output) {
            return Err(AppError::SharedActionNotAllowed { action: "applications.console".into() });
        }
        expect_success(output, "write to the console")
    }

    fn close(&self) {}

    fn supports_input(&self) -> bool {
        self.can_write
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(exit_code: i32, stdout: &str, stderr: &str) -> CommandOutput {
        CommandOutput { exit_code, stdout: stdout.to_string(), stderr: stderr.to_string() }
    }

    #[test]
    fn sudos_refusals_are_told_apart_from_a_failing_command() {
        assert!(refused_by_sudo(&output(1, "", "sudo: a password is required")));
        assert!(refused_by_sudo(&output(1, "", "Sorry, user vibessh-m-x is not allowed to execute '/usr/bin/docker rm x' as root on host.")));
        assert!(!refused_by_sudo(&output(1, "", "Error response from daemon: No such container: vibessh-app-x")));
        assert!(!refused_by_sudo(&output(0, "", "")));
    }

    #[test]
    fn a_containers_state_is_read_from_inspects_json() {
        let json = r#"[{"Id":"abc","State":{"Status":"running","ExitCode":0,"StartedAt":"2026-09-26T12:00:00.123456789Z"}}]"#;
        assert_eq!(parse_inspect(json), Some(("running".into(), "0".into(), Some("2026-09-26T12:00:00.123456789Z".into()))));
        let exited = r#"[{"State":{"Status":"exited","ExitCode":137}}]"#;
        assert_eq!(parse_inspect(exited).map(|(s, c, _)| (s, c)), Some(("exited".into(), "137".into())));
        assert_eq!(parse_inspect("[]"), None);
        assert_eq!(parse_inspect("not json"), None);
    }

    #[test]
    fn a_units_state_is_read_from_its_active_line() {
        assert_eq!(parse_unit_status("  Active: active (running) since Sat"), ApplicationStatus::Running);
        assert_eq!(parse_unit_status("  Active: inactive (dead)"), ApplicationStatus::Stopped);
        assert_eq!(parse_unit_status("  Active: failed (Result: exit-code)"), ApplicationStatus::Failed);
        assert_eq!(parse_unit_status("  Active: activating (start)"), ApplicationStatus::Starting);
        assert_eq!(parse_unit_status(""), ApplicationStatus::Stopped);
    }

    fn access(keys: &[&str]) -> SharedAccess {
        SharedAccess { team_id: uuid::Uuid::nil(), permissions: keys.iter().map(|key| (*key).to_string()).collect() }
    }

    /// Refused on this side when the grant does not cover it - no round trip
    /// to the Node to be told the same.
    #[test]
    fn an_action_outside_the_grant_is_refused_before_anything_is_sent() {
        let runtime = MemberRuntime::new(access(&["applications.files.read"]));
        match runtime.require("applications.lifecycle") {
            Err(AppError::SharedActionNotAllowed { action }) => assert_eq!(action, "applications.lifecycle"),
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert!(MemberRuntime::new(access(&["applications.lifecycle"])).require("applications.lifecycle").is_ok());
    }
}
