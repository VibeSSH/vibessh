//! Scheduled power actions - start, stop or restart an Application at set
//! times, the way Pterodactyl's schedules do.
//!
//! **They run on the Node, not in VibeSSH.** Backups only run while the app
//! is open (`services::application_backup_service`), which is fine for "every
//! few hours" and useless for "restart at 4 a.m." on a computer that is off
//! at 4 a.m. So a schedule is written to the Node as a cron file,
//! `/etc/cron.d/vibessh-app-<id>`, one line per enabled schedule, and cron
//! calls a small root-owned script, `schedule-runner`, that does the one
//! thing asked and writes down how it went. The database here stays the
//! source of truth; the cron file is regenerated from it on every change.
//!
//! **Why power actions only.** They need nothing but `docker` on the Node.
//! A console command would need the console pipe VibeSSH opens, and a backup
//! needs this app (and its S3 credentials); both can come later without
//! changing this shape.
//!
//! **Where it crosses a boundary.** The runner runs as root from cron, so
//! nothing that reaches its command line is free text: the cron fields are
//! held to digits and `* / , -`, the ids are UUIDs, the action is one of
//! three words, and the runner re-checks all of it before touching docker.
//! The schedule's name never leaves this database.

use chrono::{DateTime, TimeZone, Utc};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{
    ApplicationSchedule, ApplicationSchedules, ConnectionMode, NodeTimeZone, RuntimeType, ScheduleInput, ScheduleRun,
};
use crate::services::ssh_service::get_or_connect;
use crate::ssh::command::quote as shell_quote;
use crate::ssh::SshSession;
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::application_schedule_repository::ApplicationScheduleRepository;
use crate::storage::server_repository::ServerRepository;

const RUNNER_PATH: &str = "/usr/local/lib/vibessh/schedule-runner";

/// `cron.d` ignores files whose names contain a dot, so the id goes in bare.
fn cron_file_path(application_id: Uuid) -> String {
    format!("/etc/cron.d/vibessh-app-{application_id}")
}

/// Root-only, and outside `/run` on purpose: the last result of a schedule
/// that ran at 4 a.m. has to survive until somebody opens the tab, reboots
/// included.
fn state_dir(application_id: Uuid) -> String {
    format!("/var/lib/vibessh/schedules/{application_id}")
}

/// The script cron calls. Checks its own arguments again even though every
/// caller validated them - it runs as root, and a cron file is a text file
/// that can be edited by hand.
///
/// `docker stop`/`restart` get two minutes rather than Docker's default ten
/// seconds: a Minecraft server saving a large world can need more than ten,
/// and past the timeout Docker kills it mid-save. A server that exits sooner
/// is not held up - it is a ceiling, not a wait.
///
/// After a start or restart the console is re-attached, the same way
/// VibeSSH does after its own restart - otherwise the console tab could no
/// longer send commands until the next restart from the panel. Only onto a
/// pipe VibeSSH already made: one created here would be root's, and the
/// panel, which opens it as the connecting admin, could never open it again.
const RUNNER_SCRIPT: &str = r#"#!/bin/sh
# Installed by VibeSSH, and replaced by it when it changes - edits here do not last.
# Runs one scheduled power action for one Application, and reports past runs.
#
#   schedule-runner run <application-id> <schedule-id> <start|stop|restart>
#   schedule-runner status <application-id>
set -u
PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
STATE_ROOT=/var/lib/vibessh/schedules
CONSOLE_DIR=/run/vibessh/console
GRACE=120

is_id() {
    case "$1" in
        ''|*[!0-9a-f-]*) return 1 ;;
    esac
    [ "${#1}" -eq 36 ]
}

is_number() {
    case "$1" in
        ''|*[!0-9]*) return 1 ;;
    esac
}

# Only what an Application directory is allowed to look like - it reaches here
# from a cron line, so it is checked again rather than trusted.
is_directory() {
    case "$1" in
        /*) ;;
        *) return 1 ;;
    esac
    case "$1" in
        *[!A-Za-z0-9/._-]*|*..*) return 1 ;;
    esac
}

case "${1:-}" in
    diskcheck)
        # Every five minutes from cron: how much the directory holds, and a
        # graceful stop when it holds more than its limit - the same
        # enforcement Pterodactyl's Wings does.
        app="${2:-}"
        limit="${3:-}"
        dir="${4:-}"
        is_id "$app" && is_number "$limit" && is_directory "$dir" || { echo "schedule-runner: invalid disk check" >&2; exit 64; }
        [ -d "$dir" ] || exit 0
        used=$(du -sb -- "$dir" 2>/dev/null | cut -f1)
        is_number "$used" || used=$(( $(du -sk -- "$dir" | cut -f1) * 1024 ))
        stopped=0
        name="vibessh-app-$app"
        if [ "$used" -gt "$limit" ] && [ "$(docker inspect -f '{{.State.Running}}' "$name" 2>/dev/null)" = "true" ]; then
            docker stop -t "$GRACE" "$name" >/dev/null 2>&1 && stopped=1
            logger -t vibessh-schedule "$name stopped: $used bytes used, limit $limit" 2>/dev/null || true
        fi
        umask 077
        mkdir -p "$STATE_ROOT/$app"
        printf '%s\t%s\t%s\t%s\n' "$(date +%s)" "$used" "$limit" "$stopped" > "$STATE_ROOT/$app/disk.tmp" \
            && mv -f "$STATE_ROOT/$app/disk.tmp" "$STATE_ROOT/$app/disk"
        exit 0
        ;;
    disk)
        app="${2:-}"
        is_id "$app" || { echo "schedule-runner: invalid application id" >&2; exit 64; }
        [ -f "$STATE_ROOT/$app/disk" ] && head -n 1 "$STATE_ROOT/$app/disk"
        exit 0
        ;;
    status)
        app="${2:-}"
        is_id "$app" || { echo "schedule-runner: invalid application id" >&2; exit 64; }
        [ -d "$STATE_ROOT/$app" ] || exit 0
        for file in "$STATE_ROOT/$app"/*; do
            [ -f "$file" ] || continue
            schedule="${file##*/}"
            is_id "$schedule" || continue
            printf '%s\t' "$schedule"
            head -n 1 "$file"
        done
        exit 0
        ;;
    run) ;;
    *)
        echo "usage: schedule-runner run <application-id> <schedule-id> <start|stop|restart> | status <application-id>" >&2
        exit 64
        ;;
esac

app="${2:-}"
schedule="${3:-}"
action="${4:-}"
is_id "$app" && is_id "$schedule" || { echo "schedule-runner: invalid id" >&2; exit 64; }
name="vibessh-app-$app"

started=$(date +%s)
case "$action" in
    start) output=$(docker start "$name" 2>&1) ;;
    stop) output=$(docker stop -t "$GRACE" "$name" 2>&1) ;;
    restart) output=$(docker restart -t "$GRACE" "$name" 2>&1) ;;
    *) echo "schedule-runner: invalid action" >&2; exit 64 ;;
esac
code=$?

if [ "$code" -eq 0 ] && [ "$action" != stop ]; then
    fifo="$CONSOLE_DIR/$app.stdin"
    if [ -p "$fifo" ]; then
        ( exec 3<>"$fifo"; nohup docker attach --sig-proxy=false "$name" <&3 3<&- >/dev/null 2>&1 & )
    fi
fi

umask 077
mkdir -p "$STATE_ROOT/$app"
message=$(printf '%s' "$output" | tr '\n\t' '  ' | cut -c1-300)
printf '%s\t%s\t%s\t%s\n' "$started" "$action" "$code" "$message" > "$STATE_ROOT/$app/$schedule.tmp" \
    && mv -f "$STATE_ROOT/$app/$schedule.tmp" "$STATE_ROOT/$app/$schedule"
logger -t vibessh-schedule "$name $action exited $code" 2>/dev/null || true
exit "$code"
"#;

/// The ranges of the five cron fields, in order.
const FIELDS: [(&str, u32, u32); 5] = [("minute", 0, 59), ("hour", 0, 23), ("day of month", 1, 31), ("month", 1, 12), ("day of week", 0, 7)];

/// Checks a cron expression and returns it with single spaces.
///
/// Numbers only - no `JAN` or `MON`, no `@daily` - so the only characters
/// that can reach the cron file are digits and `* / , -`. That is the whole
/// of the injection defence for the one field a person types, and it is
/// enough: a line of cron cannot be ended, commented or extended with those.
pub fn validate_cron(expression: &str) -> AppResult<String> {
    let fields: Vec<&str> = expression.split_whitespace().collect();
    if fields.len() != 5 {
        return Err(AppError::InvalidInput(format!(
            "a schedule needs five fields - minute, hour, day of month, month, day of week - and '{expression}' has {}",
            fields.len()
        )));
    }
    for (field, (what, min, max)) in fields.iter().zip(FIELDS) {
        validate_field(field, what, min, max)?;
    }
    Ok(fields.join(" "))
}

fn validate_field(field: &str, what: &str, min: u32, max: u32) -> AppResult<()> {
    let invalid = || AppError::InvalidInput(format!("'{field}' isn't a valid {what} - use numbers from {min} to {max}, '*', ranges like 1-5, lists like 1,15 and steps like */2"));
    for item in field.split(',') {
        let (range, step) = match item.split_once('/') {
            Some((range, step)) => (range, Some(step)),
            None => (item, None),
        };
        if let Some(step) = step {
            let step: u32 = step.parse().map_err(|_| invalid())?;
            if step == 0 || step > max {
                return Err(invalid());
            }
        }
        if range == "*" {
            continue;
        }
        let (from, to) = match range.split_once('-') {
            Some((from, to)) => (from, Some(to)),
            None => (range, None),
        };
        let number = |text: &str| -> AppResult<u32> {
            if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(invalid());
            }
            let value: u32 = text.parse().map_err(|_| invalid())?;
            if value < min || value > max {
                return Err(invalid());
            }
            Ok(value)
        };
        let from = number(from)?;
        if let Some(to) = to {
            if number(to)? < from {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

fn validate_input(input: &ScheduleInput) -> AppResult<ScheduleInput> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(AppError::InvalidInput("give the schedule a name".into()));
    }
    if name.chars().count() > 80 {
        return Err(AppError::InvalidInput("a schedule name can be at most 80 characters".into()));
    }
    Ok(ScheduleInput { name: name.to_string(), cron: validate_cron(&input.cron)?, action: input.action, enabled: input.enabled })
}

/// The cron file for one Application: a line per enabled schedule, nothing
/// for a disabled one. The trailing newline is not optional - cron silently
/// ignores a last line without one.
pub fn build_cron_file(application_id: Uuid, schedules: &[ApplicationSchedule], disk: Option<&DiskCheck>) -> String {
    let mut file = format!(
        "# Managed by VibeSSH for Application {application_id}. Edits here are overwritten.\n\
         SHELL=/bin/sh\n\
         PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\n"
    );
    for schedule in schedules.iter().filter(|schedule| schedule.enabled) {
        file.push_str(&format!("{} root {RUNNER_PATH} run {application_id} {} {}\n", schedule.cron, schedule.id, schedule.action.as_str()));
    }
    if let Some(disk) = disk {
        file.push_str(&format!("{DISK_CHECK_EVERY} root {RUNNER_PATH} diskcheck {application_id} {} {}\n", disk.limit_bytes, disk.directory));
    }
    file
}

/// How often the Node measures a limited Application's directory.
const DISK_CHECK_EVERY: &str = "*/5 * * * *";

/// A disk limit as the Node enforces it: the directory to measure and the
/// most it may hold.
pub struct DiskCheck {
    pub limit_bytes: u64,
    pub directory: String,
}

/// The directory as it may appear on a cron line. Stricter than a working
/// directory needs to be anywhere else, because a cron line has no quoting:
/// letters, digits and `/ . _ -` only, absolute, no `..`. One that does not
/// fit cannot have a disk limit, and says so rather than being written.
fn cron_safe_directory(directory: &str) -> AppResult<String> {
    let fits = directory.starts_with('/')
        && !directory.contains("..")
        && directory.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'));
    if !fits {
        return Err(AppError::InvalidInput(format!(
            "a disk limit needs a working directory made of letters, digits and / . _ - only, and '{directory}' isn't"
        )));
    }
    Ok(directory.to_string())
}

/// The limit stored for an Application, in megabytes, from its runtime config.
fn disk_limit_mb(runtime_config: &serde_json::Value) -> Option<u64> {
    runtime_config.get("diskLimitMb").and_then(serde_json::Value::as_u64).filter(|mb| *mb > 0)
}

fn disk_check_for(app_repo: &ApplicationRepository, application_id: Uuid) -> AppResult<Option<DiskCheck>> {
    let detail = app_repo.get(application_id)?.ok_or_else(|| AppError::NotFound(format!("application {application_id}")))?;
    let Some(limit_mb) = disk_limit_mb(&detail.runtime_config) else {
        return Ok(None);
    };
    Ok(Some(DiskCheck { limit_bytes: limit_mb * 1024 * 1024, directory: cron_safe_directory(&detail.application.working_directory)? }))
}

/// The Node an Application's schedules run on, or why it cannot have any.
async fn node_for(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
) -> AppResult<(std::sync::Arc<SshSession>, Uuid)> {
    let detail = app_repo.get(application_id)?.ok_or_else(|| AppError::NotFound(format!("application {application_id}")))?;
    if detail.application.runtime_type != RuntimeType::Docker {
        return Err(AppError::InvalidInput("schedules are only available for Docker applications".into()));
    }
    let Some(server_id) = detail.application.server_id else {
        return Err(AppError::InvalidInput("schedules run on a Node, and this application runs on this computer".into()));
    };
    let server = server_repo.get(server_id)?.ok_or_else(|| AppError::NotFound(format!("server {server_id}")))?;
    if server.connection_mode == ConnectionMode::Agent {
        return Err(AppError::InvalidInput("schedules aren't available on a Node managed by the Vibe Agent yet".into()));
    }
    Ok((get_or_connect(server_repo, sessions, server_id).await?, server_id))
}

/// Puts the runner in place, when it is missing or out of date - the same
/// compare-then-install `files::sudo_user::ensure_helper_installed` does.
async fn ensure_runner_installed(connection: &SshSession) -> AppResult<()> {
    let deployed = connection.execute_command(&format!("sudo cat {} 2>/dev/null", shell_quote(RUNNER_PATH))).await;
    if matches!(deployed, Ok(ref output) if output.stdout == RUNNER_SCRIPT) {
        return Ok(());
    }
    // Staged in the admin's own home over SFTP, then moved into place as
    // root: nothing passes through a world-writable directory (AGENTS.md 4).
    let staging = format!(".vibessh-schedule-runner-{}", Uuid::new_v4());
    connection.write_file(&staging, RUNNER_SCRIPT.as_bytes()).await?;
    let output = connection
        .execute_command(&format!(
            "sudo install -D -o root -g root -m 0755 {staging} {runner}; rc=$?; rm -f {staging}; exit $rc",
            staging = shell_quote(&staging),
            runner = shell_quote(RUNNER_PATH),
        ))
        .await?;
    if output.exit_code != 0 {
        return Err(AppError::Connection(format!("couldn't install the schedule runner: {}", output.stderr.trim())));
    }
    Ok(())
}

const CRON_PRESENT: &str = "[ -d /etc/cron.d ] && { command -v cron || command -v crond || [ -x /usr/sbin/cron ] || [ -x /usr/sbin/crond ]; } >/dev/null 2>&1";

/// Refuses early on a Node without cron - otherwise the file would be
/// written and nothing would ever read it. Its own error, so the interface
/// can offer `install_cron` rather than only name the problem.
async fn ensure_cron_available(connection: &SshSession, server_id: Uuid) -> AppResult<()> {
    let output = connection.execute_command(CRON_PRESENT).await?;
    if output.exit_code != 0 {
        return Err(AppError::CronMissing { server_id });
    }
    Ok(())
}

/// Installs cron on a Node and starts it - what the "Install cron" button
/// runs. Debian and Ubuntu call the package `cron`, Fedora and RHEL
/// `cronie` (its service `crond`). The service is enabled explicitly
/// because not every image starts a daemon on install; a Node without
/// systemd is left to the package's own start-up.
pub async fn install_cron(server_repo: &ServerRepository, sessions: &SshSessionManager, server_id: Uuid) -> AppResult<()> {
    let connection = get_or_connect(server_repo, sessions, server_id).await?;
    let output = connection
        .execute_command(
            "if command -v apt-get >/dev/null 2>&1; then                  sudo apt-get update -qq && sudo DEBIAN_FRONTEND=noninteractive apt-get install -y cron &&                  { sudo systemctl enable --now cron >/dev/null 2>&1 || true; };              elif command -v dnf >/dev/null 2>&1; then                  sudo dnf install -y cronie && { sudo systemctl enable --now crond >/dev/null 2>&1 || true; };              elif command -v yum >/dev/null 2>&1; then                  sudo yum install -y cronie && { sudo systemctl enable --now crond >/dev/null 2>&1 || true; };              else echo 'no supported package manager (apt, dnf or yum) on this Node' >&2; exit 3; fi",
        )
        .await?;
    if output.exit_code != 0 {
        let detail = output.stderr.trim();
        let detail = if detail.is_empty() { "the install failed" } else { detail };
        return Err(AppError::Connection(format!("couldn't install cron: {detail}")));
    }
    // Checked the same way a schedule checks, so "installed" means what a
    // schedule needs rather than what the package manager said.
    ensure_cron_available(&connection, server_id).await
}

/// Makes the Node's cron file match `schedules`. No enabled schedule means no
/// file, rather than a file with nothing in it.
/// Regenerates the Node's cron file from everything that belongs in it: the
/// enabled schedules and the disk check.
async fn sync_node(
    connection: &SshSession,
    server_id: Uuid,
    app_repo: &ApplicationRepository,
    schedule_repo: &ApplicationScheduleRepository,
    application_id: Uuid,
) -> AppResult<()> {
    let schedules = schedule_repo.list(application_id)?;
    let disk = disk_check_for(app_repo, application_id)?;
    write_to_node(connection, server_id, application_id, &schedules, disk.as_ref()).await
}

async fn write_to_node(
    connection: &SshSession,
    server_id: Uuid,
    application_id: Uuid,
    schedules: &[ApplicationSchedule],
    disk: Option<&DiskCheck>,
) -> AppResult<()> {
    let path = shell_quote(&cron_file_path(application_id));
    if !schedules.iter().any(|schedule| schedule.enabled) && disk.is_none() {
        let output = connection.execute_command(&format!("sudo rm -f {path}")).await?;
        if output.exit_code != 0 {
            return Err(AppError::Connection(format!("couldn't remove the schedule file: {}", output.stderr.trim())));
        }
        return Ok(());
    }
    ensure_cron_available(connection, server_id).await?;
    ensure_runner_installed(connection).await?;
    // Through stdin into a temporary file beside the target, then renamed, so
    // cron never reads a half-written file. `cron.d` wants root:root 0644.
    let temporary = shell_quote(&format!("/etc/cron.d/.vibessh-app-{application_id}.new"));
    let output = connection
        .execute_command_with_input(
            &format!("sudo tee {temporary} >/dev/null && sudo chmod 0644 {temporary} && sudo mv -f {temporary} {path}"),
            build_cron_file(application_id, schedules, disk).as_bytes(),
        )
        .await?;
    if output.exit_code != 0 {
        return Err(AppError::Connection(format!("couldn't write the schedule file: {}", output.stderr.trim())));
    }
    Ok(())
}

/// One line of `schedule-runner status`: schedule, time, action, exit code,
/// message - tab-separated.
fn parse_status_line(line: &str) -> Option<ScheduleRun> {
    let mut parts = line.splitn(5, '\t');
    let schedule_id = Uuid::parse_str(parts.next()?).ok()?;
    let ran_at: DateTime<Utc> = Utc.timestamp_opt(parts.next()?.trim().parse().ok()?, 0).single()?;
    let action = parts.next()?.to_string();
    let exit_code = parts.next()?.trim().parse().ok()?;
    let message = parts.next().unwrap_or("").trim().to_string();
    Some(ScheduleRun { schedule_id, ran_at, action, exit_code, message })
}

async fn read_last_runs(connection: &SshSession, application_id: Uuid) -> AppResult<Vec<ScheduleRun>> {
    let output = connection
        .execute_command(&format!("[ -x {runner} ] || exit 0; sudo {runner} status {application_id}", runner = shell_quote(RUNNER_PATH)))
        .await?;
    if output.exit_code != 0 {
        return Err(AppError::Connection(format!("couldn't read past schedule runs: {}", output.stderr.trim())));
    }
    Ok(output.stdout.lines().filter_map(parse_status_line).collect())
}

/// `+0200` as minutes east of UTC.
fn parse_offset(text: &str) -> Option<i32> {
    let text = text.trim();
    let (sign, digits) = match text.as_bytes().first()? {
        b'+' => (1, &text[1..]),
        b'-' => (-1, &text[1..]),
        _ => return None,
    };
    if digits.len() != 4 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let hours: i32 = digits[..2].parse().ok()?;
    let minutes: i32 = digits[2..].parse().ok()?;
    Some(sign * (hours * 60 + minutes))
}

async fn read_time_zone(connection: &SshSession) -> AppResult<NodeTimeZone> {
    let output = connection
        .execute_command("date +%z; timedatectl show -p Timezone --value 2>/dev/null || cat /etc/timezone 2>/dev/null || true")
        .await?;
    let mut lines = output.stdout.lines();
    let offset_minutes = lines
        .next()
        .and_then(parse_offset)
        .ok_or_else(|| AppError::Connection(format!("the Node's clock gave an unexpected answer: {:?}", output.stdout.trim())))?;
    let name = lines.next().map(str::trim).filter(|name| !name.is_empty()).map(str::to_string);
    Ok(NodeTimeZone { name, offset_minutes })
}

/// The Schedules tab's data. The schedules come from the database and are
/// always returned; what only the Node knows - past runs, its time zone -
/// comes back empty with a reason when the Node cannot be reached.
pub async fn list_schedules(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    schedule_repo: &ApplicationScheduleRepository,
    application_id: Uuid,
) -> AppResult<ApplicationSchedules> {
    let schedules = schedule_repo.list(application_id)?;
    let node = async {
        let (connection, _) = node_for(app_repo, server_repo, sessions, application_id).await?;
        let time_zone = read_time_zone(&connection).await?;
        let last_runs = read_last_runs(&connection, application_id).await?;
        AppResult::Ok((time_zone, last_runs))
    };
    Ok(match node.await {
        Ok((time_zone, last_runs)) => ApplicationSchedules { schedules, last_runs, time_zone: Some(time_zone), node_error: None },
        Err(err) => ApplicationSchedules { schedules, last_runs: Vec::new(), time_zone: None, node_error: serde_json::to_value(&err).ok() },
    })
}

pub async fn create_schedule(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    schedule_repo: &ApplicationScheduleRepository,
    application_id: Uuid,
    input: &ScheduleInput,
) -> AppResult<ApplicationSchedule> {
    let input = validate_input(input)?;
    let (connection, server_id) = node_for(app_repo, server_repo, sessions, application_id).await?;
    let created = schedule_repo.create(application_id, &input)?;
    // The row and the Node have to agree. A schedule the Node never got is
    // one the panel would show as set while nothing happens - so it goes.
    if let Err(err) = sync_node(&connection, server_id, app_repo, schedule_repo, application_id).await {
        if let Err(undo) = schedule_repo.delete(created.id) {
            log::error!("couldn't undo a schedule the Node refused: {undo}");
        }
        return Err(err);
    }
    Ok(created)
}

pub async fn update_schedule(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    schedule_repo: &ApplicationScheduleRepository,
    schedule_id: Uuid,
    input: &ScheduleInput,
) -> AppResult<ApplicationSchedule> {
    let input = validate_input(input)?;
    let before = schedule_repo.get(schedule_id)?.ok_or_else(|| AppError::NotFound(format!("schedule {schedule_id}")))?;
    let (connection, server_id) = node_for(app_repo, server_repo, sessions, before.application_id).await?;
    schedule_repo.update(schedule_id, &input)?;
    if let Err(err) = sync_node(&connection, server_id, app_repo, schedule_repo, before.application_id).await {
        let previous = ScheduleInput { name: before.name.clone(), cron: before.cron.clone(), action: before.action, enabled: before.enabled };
        if let Err(undo) = schedule_repo.update(schedule_id, &previous) {
            log::error!("couldn't undo a schedule change the Node refused: {undo}");
        }
        return Err(err);
    }
    schedule_repo.get(schedule_id)?.ok_or_else(|| AppError::NotFound(format!("schedule {schedule_id}")))
}

pub async fn delete_schedule(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    schedule_repo: &ApplicationScheduleRepository,
    schedule_id: Uuid,
) -> AppResult<()> {
    let Some(schedule) = schedule_repo.get(schedule_id)? else {
        return Ok(());
    };
    let (connection, server_id) = node_for(app_repo, server_repo, sessions, schedule.application_id).await?;
    schedule_repo.delete(schedule_id)?;
    // Deleted here but still in the Node's cron would keep firing with no row
    // left to show it - so a refusal puts the row back.
    if let Err(err) = sync_node(&connection, server_id, app_repo, schedule_repo, schedule.application_id).await {
        if let Err(undo) = schedule_repo.insert(&schedule) {
            log::error!("couldn't restore a schedule the Node wouldn't remove: {undo}");
        }
        return Err(err);
    }
    Ok(())
}

/// Runs a schedule's action now, through the same runner cron uses - so it
/// is also the way to check a schedule works before waiting for it.
pub async fn run_schedule_now(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    schedule_repo: &ApplicationScheduleRepository,
    schedule_id: Uuid,
) -> AppResult<()> {
    let schedule = schedule_repo.get(schedule_id)?.ok_or_else(|| AppError::NotFound(format!("schedule {schedule_id}")))?;
    let (connection, _) = node_for(app_repo, server_repo, sessions, schedule.application_id).await?;
    ensure_runner_installed(&connection).await?;
    let output = connection
        .execute_command(&format!(
            "sudo {} run {} {} {}",
            shell_quote(RUNNER_PATH),
            schedule.application_id,
            schedule.id,
            schedule.action.as_str()
        ))
        .await?;
    if output.exit_code != 0 {
        let detail = output.stdout.trim();
        let detail = if detail.is_empty() { output.stderr.trim() } else { detail };
        return Err(AppError::Connection(format!("the {} didn't succeed: {detail}", schedule.action.as_str())));
    }
    Ok(())
}

/// Removes an Application's cron file and its record of past runs - part of
/// `delete_application`'s teardown (AGENTS.md 5). The rows go with the
/// Application's own row, by cascade.
pub async fn remove_from_node(connection: &SshSession, application_id: Uuid) -> AppResult<()> {
    let output = connection
        .execute_command(&format!("sudo rm -f {} && sudo rm -rf {}", shell_quote(&cron_file_path(application_id)), shell_quote(&state_dir(application_id))))
        .await?;
    if output.exit_code != 0 {
        return Err(AppError::Connection(output.stderr.trim().to_string()));
    }
    Ok(())
}

/// Moves `from`'s schedules to `to` and writes them to `to`'s Node - what a
/// migration does, since the migrated Application is a new row. `from`'s
/// cron file is left for its teardown, which runs right after.
pub async fn move_schedules(
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    app_repo: &ApplicationRepository,
    schedule_repo: &ApplicationScheduleRepository,
    from: Uuid,
    to: Uuid,
) -> AppResult<usize> {
    let moved = schedule_repo.reassign(from, to)?;
    // The disk limit travels in the runtime config, so the target can need a
    // cron file even with no schedule to move.
    if moved > 0 || disk_check_for(app_repo, to)?.is_some() {
        let (connection, server_id) = node_for(app_repo, server_repo, sessions, to).await?;
        sync_node(&connection, server_id, app_repo, schedule_repo, to).await?;
    }
    Ok(moved)
}

/// The largest disk limit accepted: 10 TB, well past any disk a Node has, so
/// a slip of the keyboard is caught rather than stored.
const MAX_DISK_LIMIT_MB: u64 = 10 * 1024 * 1024;

/// Sets or clears an Application's disk limit and writes the Node's check.
///
/// Enforced the way Pterodactyl's Wings enforces it: the Node measures the
/// directory every five minutes and stops the server gracefully once it
/// holds more than the limit, and the panel refuses to start it while it
/// does (`ensure_within_disk_limit`). Not a filesystem quota - that would
/// depend on the Node's filesystem and mount options - so a server can pass
/// the limit for up to five minutes before it is stopped.
pub async fn set_disk_limit(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    schedule_repo: &ApplicationScheduleRepository,
    application_id: Uuid,
    limit_mb: Option<u64>,
) -> AppResult<crate::models::ApplicationDetail> {
    if let Some(mb) = limit_mb {
        if !(100..=MAX_DISK_LIMIT_MB).contains(&mb) {
            return Err(AppError::InvalidInput("a disk limit has to be between 100 MB and 10 TB".into()));
        }
    }
    let (connection, server_id) = node_for(app_repo, server_repo, sessions, application_id).await?;
    let detail = app_repo.get(application_id)?.ok_or_else(|| AppError::NotFound(format!("application {application_id}")))?;
    if limit_mb.is_some() {
        cron_safe_directory(&detail.application.working_directory)?;
    }
    let previous = detail.runtime_config.clone();
    let mut runtime_config = previous.clone();
    let object = runtime_config.as_object_mut().ok_or_else(|| AppError::Internal("runtime_config wasn't a JSON object".into()))?;
    match limit_mb {
        Some(mb) => object.insert("diskLimitMb".to_string(), serde_json::json!(mb)),
        None => object.remove("diskLimitMb"),
    };
    app_repo.update_runtime_config(application_id, &runtime_config)?;
    // Stored but not on the Node would be a limit the panel shows and nothing
    // enforces - so a refusal puts the old config back.
    if let Err(err) = sync_node(&connection, server_id, app_repo, schedule_repo, application_id).await {
        if let Err(undo) = app_repo.update_runtime_config(application_id, &previous) {
            log::error!("couldn't undo a disk limit the Node refused: {undo}");
        }
        return Err(err);
    }
    app_repo.get(application_id)?.ok_or_else(|| AppError::NotFound(format!("application {application_id}")))
}

/// One line of `schedule-runner disk`: when, bytes used, limit, whether that
/// check stopped the server.
fn parse_disk_line(line: &str) -> Option<crate::models::DiskUsage> {
    let mut parts = line.trim().split('\t');
    let checked_at = Utc.timestamp_opt(parts.next()?.parse().ok()?, 0).single()?;
    let used_bytes = parts.next()?.parse().ok()?;
    let limit_bytes = parts.next()?.parse().ok()?;
    let stopped = parts.next()? == "1";
    Some(crate::models::DiskUsage { checked_at, used_bytes, limit_bytes, stopped })
}

/// What the Node's last disk check found, or `None` before the first one (or
/// without a limit). Read from the check's record rather than measured here:
/// `du` over a large world is slow, and the Overview asks often.
pub async fn disk_usage(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
) -> AppResult<Option<crate::models::DiskUsage>> {
    let (connection, _) = node_for(app_repo, server_repo, sessions, application_id).await?;
    let output = connection
        .execute_command(&format!("[ -x {runner} ] || exit 0; sudo {runner} disk {application_id}", runner = shell_quote(RUNNER_PATH)))
        .await?;
    if output.exit_code != 0 {
        return Err(AppError::Connection(format!("couldn't read the disk check: {}", output.stderr.trim())));
    }
    Ok(output.stdout.lines().next().and_then(parse_disk_line))
}

/// Refuses to start an Application whose directory is over its disk limit -
/// the other half of the enforcement `set_disk_limit` describes. Measured now,
/// not read from the last check: the person may have just deleted files to
/// get under it.
pub async fn ensure_within_disk_limit(connection: &SshSession, runtime_config: &serde_json::Value, working_directory: &str) -> AppResult<()> {
    let Some(limit_mb) = disk_limit_mb(runtime_config) else {
        return Ok(());
    };
    let output = connection
        .execute_command(&format!("sudo du -sb -- {} 2>/dev/null | cut -f1", shell_quote(working_directory)))
        .await?;
    let Ok(used) = output.stdout.trim().parse::<u64>() else {
        // Could not measure: starting is not the place to fail over a figure
        // the periodic check will take anyway.
        log::warn!("couldn't measure '{working_directory}' against its disk limit: {}", output.stderr.trim());
        return Ok(());
    };
    let limit = limit_mb * 1024 * 1024;
    if used > limit {
        return Err(AppError::DiskLimitExceeded { used_mb: used / (1024 * 1024), limit_mb });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ScheduleAction;

    fn schedule(cron: &str, action: ScheduleAction, enabled: bool) -> ApplicationSchedule {
        ApplicationSchedule {
            id: Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap(),
            application_id: Uuid::nil(),
            name: "x".into(),
            cron: cron.into(),
            action,
            enabled,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn a_cron_expression_is_five_fields_of_numbers_and_nothing_else() {
        assert_eq!(validate_cron("0 4 * * *").unwrap(), "0 4 * * *");
        assert_eq!(validate_cron("  30   */6 1,15 1-12 1-5 ").unwrap(), "30 */6 1,15 1-12 1-5");
        assert!(validate_cron("0 4 * *").is_err(), "four fields");
        assert!(validate_cron("0 4 * * * *").is_err(), "six fields");
        assert!(validate_cron("60 4 * * *").is_err(), "minute out of range");
        assert!(validate_cron("0 24 * * *").is_err(), "hour out of range");
        assert!(validate_cron("0 4 0 * *").is_err(), "day of month starts at 1");
        assert!(validate_cron("0 4 * * 8").is_err(), "day of week ends at 7");
        assert!(validate_cron("0 5-4 * * *").is_err(), "backwards range");
        assert!(validate_cron("*/0 * * * *").is_err(), "zero step");
        assert!(validate_cron("0 4 * * MON").is_err(), "names are not accepted");
        assert!(validate_cron("@daily").is_err());
    }

    #[test]
    fn nothing_that_could_end_or_extend_a_cron_line_gets_through() {
        for hostile in ["0 4 * * * ; rm -rf /", "0 4 * * *\n* * * * * root sh", "0 4 * * $(id)", "0 4 * * `id`", "0 4 # * *", "0 4 * * 1 root"] {
            assert!(validate_cron(hostile).is_err(), "{hostile:?}");
        }
    }

    #[test]
    fn the_cron_file_has_a_line_per_enabled_schedule_and_ends_with_a_newline() {
        let application = Uuid::parse_str("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee").unwrap();
        let file = build_cron_file(application, &[schedule("0 4 * * *", ScheduleAction::Restart, true), schedule("0 5 * * *", ScheduleAction::Stop, false)], None);
        assert!(file.ends_with('\n'));
        let jobs: Vec<&str> = file.lines().filter(|line| !line.starts_with('#') && !line.contains('=')).collect();
        assert_eq!(
            jobs,
            ["0 4 * * * root /usr/local/lib/vibessh/schedule-runner run aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee 11111111-2222-3333-4444-555555555555 restart"]
        );
    }

    #[test]
    fn a_disk_limit_adds_a_check_every_five_minutes_even_without_schedules() {
        let application = Uuid::parse_str("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee").unwrap();
        let disk = DiskCheck { limit_bytes: 20 * 1024 * 1024 * 1024, directory: "/home/container/mc".into() };
        let file = build_cron_file(application, &[], Some(&disk));
        assert!(file.ends_with('\n'));
        assert!(file.contains(
            "*/5 * * * * root /usr/local/lib/vibessh/schedule-runner diskcheck aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee 21474836480 /home/container/mc\n"
        ));
    }

    #[test]
    fn only_a_plain_absolute_directory_can_go_on_a_cron_line() {
        assert!(cron_safe_directory("/home/container/royalmc-bedwars").is_ok());
        assert!(cron_safe_directory("/srv/app_1.2").is_ok());
        for bad in ["relative/dir", "/home/my server", "/srv/a;id", "/srv/$(id)", "/srv/../etc", "/srv/a\nb", "/srv/a%b"] {
            assert!(cron_safe_directory(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_disk_record_reads_back() {
        let usage = parse_disk_line("1758859200\t1073741824\t2147483648\t0\n").unwrap();
        assert_eq!((usage.used_bytes, usage.limit_bytes, usage.stopped), (1073741824, 2147483648, false));
        assert!(parse_disk_line("1758859200\t5\t2\t1").unwrap().stopped);
        assert!(parse_disk_line("garbage").is_none());
    }

    #[test]
    fn the_runner_script_is_valid_sh() {
        use std::io::Write;
        let Ok(mut child) = std::process::Command::new("sh").arg("-n").stdin(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn() else {
            return;
        };
        child.stdin.take().unwrap().write_all(RUNNER_SCRIPT.as_bytes()).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    }

    #[test]
    fn the_cron_file_name_has_no_dot_for_cron_d_to_skip() {
        let path = cron_file_path(Uuid::new_v4());
        let name = path.rsplit('/').next().unwrap();
        assert!(!name.contains('.'), "{name}");
    }

    #[test]
    fn a_status_line_reads_back_into_a_run() {
        let run = parse_status_line("11111111-2222-3333-4444-555555555555\t1758859200\trestart\t0\tvibessh-app-x").unwrap();
        assert_eq!(run.exit_code, 0);
        assert_eq!(run.action, "restart");
        assert_eq!(run.ran_at.timestamp(), 1758859200);
        assert_eq!(run.message, "vibessh-app-x");
        assert!(parse_status_line("not-a-uuid\t1\trestart\t0\t").is_none());
        let failed = parse_status_line("11111111-2222-3333-4444-555555555555\t1758859200\tstart\t1\tError: No such container").unwrap();
        assert_eq!(failed.exit_code, 1);
    }

    #[test]
    fn a_utc_offset_reads_as_minutes_east() {
        assert_eq!(parse_offset("+0200"), Some(120));
        assert_eq!(parse_offset("-0530"), Some(-330));
        assert_eq!(parse_offset("+0000\n"), Some(0));
        assert_eq!(parse_offset("CEST"), None);
    }

    #[test]
    fn the_runner_checks_its_own_arguments_and_gives_docker_time_to_save() {
        assert!(RUNNER_SCRIPT.starts_with("#!/bin/sh\n"));
        assert!(RUNNER_SCRIPT.contains("docker stop -t \"$GRACE\""));
        assert!(RUNNER_SCRIPT.contains("docker restart -t \"$GRACE\""));
        assert!(RUNNER_SCRIPT.contains("is_id \"$app\" && is_id \"$schedule\""));
        // Re-attaches only to a pipe that already exists - never makes one.
        assert!(RUNNER_SCRIPT.contains("if [ -p \"$fifo\" ]"));
        assert!(!RUNNER_SCRIPT.contains("mkfifo"));
        assert!(!RUNNER_SCRIPT.contains("/tmp"));
    }
}
