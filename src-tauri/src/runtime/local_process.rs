//! `LocalProcessRuntime` - real local process management for Applications
//! with `server_id: None` (see `models::application::ApplicationLocation`).
//! Everything here runs on this machine: no SSH, no Agent, nothing remote -
//! which is what let this be the runtime Phase 2 could implement and test
//! first, without any remote host (docs/APPLICATIONS_ARCHITECTURE.md
//! Section 10).
//!
//! Interpretation of `ApplicationRuntime::stop`'s `graceful` flag, since
//! this is the first concrete implementation of that trait: the stop signal
//! itself (SIGTERM / `GenerateConsoleCtrlEvent`) is the same either way -
//! `graceful` toggles whether the call *waits* for the process to actually
//! exit before returning, versus firing the signal and returning
//! immediately. `kill()` always force-kills and always waits.

use std::collections::{HashMap, VecDeque};
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{ApplicationStatus, EnvironmentVariable};

use super::{health_check, ApplicationConsole, ApplicationRuntime, HealthCheckSpec, HealthStatus, LogProvider, ResourceUsage, RuntimeContext};

/// How many recent output lines (stdout+stderr merged, in arrival order)
/// each running process keeps for `LogProvider::tail` - a fixed bound so a
/// chatty process can't grow this without limit for as long as VibeSSH runs.
const OUTPUT_HISTORY_LINES: usize = 1000;

/// What `runtime_config` deserializes into for `RuntimeType::LocalProcess` -
/// see `Application`'s own doc comment for why the shape is typed here,
/// downstream, rather than in the shared model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalProcessConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

fn parse_config(ctx: &RuntimeContext<'_>) -> AppResult<LocalProcessConfig> {
    serde_json::from_value(ctx.runtime_config.clone())
        .map_err(|err| AppError::InvalidInput(format!("invalid local process configuration: {err}")))
}

struct OutputBuffer {
    lines: VecDeque<String>,
}

impl OutputBuffer {
    fn new() -> Self {
        Self { lines: VecDeque::with_capacity(OUTPUT_HISTORY_LINES) }
    }

    fn push(&mut self, line: String) {
        if self.lines.len() == OUTPUT_HISTORY_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }

    fn tail(&self, max_lines: usize) -> Vec<String> {
        let skip = self.lines.len().saturating_sub(max_lines);
        self.lines.iter().skip(skip).cloned().collect()
    }
}

struct RunningProcess {
    pid: u32,
    started_at: Instant,
    stdin_tx: mpsc::UnboundedSender<Vec<u8>>,
    output: Arc<StdMutex<OutputBuffer>>,
    /// Filled in by the reaper task once the process actually exits - the
    /// only source of truth `status()` reads, since a plain child process
    /// has no independent supervisor to ask instead.
    exit_status: Arc<StdMutex<Option<ExitStatus>>>,
    /// Set before `stop()`/`kill()` send their signal. A process killed via
    /// SIGTERM/CTRL_BREAK almost never exits with a clean success status
    /// (Windows in particular reports a non-zero STATUS_CONTROL_C_EXIT-style
    /// code for a CTRL_BREAK-terminated process) - without this flag,
    /// `status()` would misclassify every requested stop as `Failed`. Only
    /// an exit `status()` sees with this flag still `false` is judged by its
    /// actual exit code.
    stop_requested: Arc<AtomicBool>,
}

struct ProcessSnapshot {
    pid: u32,
    started_at: Instant,
    exit_status: Option<ExitStatus>,
    stop_requested: bool,
}

/// Tracks every application-managed local process by application id, the
/// same "id -> live handle" shape `TerminalSessionManager` already uses for
/// terminals. One instance is Tauri-managed state, shared by every
/// `LocalProcessRuntime` constructed for a command - mirrors
/// `SshSessionManager` being the shared cache behind per-command
/// `SshSession` lookups.
#[derive(Default)]
pub struct LocalProcessManager {
    processes: AsyncMutex<HashMap<Uuid, RunningProcess>>,
}

impl LocalProcessManager {
    pub fn new() -> Self {
        Self::default()
    }

    async fn spawn(
        &self,
        application_id: Uuid,
        config: &LocalProcessConfig,
        working_directory: &str,
        environment: &[EnvironmentVariable],
    ) -> AppResult<()> {
        let mut processes = self.processes.lock().await;
        if let Some(existing) = processes.get(&application_id) {
            let already_exited = existing.exit_status.lock().unwrap().is_some();
            if !already_exited {
                return Err(AppError::InvalidInput("this application is already running".into()));
            }
        }

        let mut command = Command::new(&config.command);
        command
            .args(&config.args)
            .current_dir(working_directory)
            .envs(environment.iter().map(|env| (env.key.clone(), env.value.clone())))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            // The process is what the user asked to keep running, not a
            // detail of this handle - it must not die just because VibeSSH
            // drops or replaces its `Child` internally.
            .kill_on_drop(false);

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            command.creation_flags(CREATE_NEW_PROCESS_GROUP);
        }

        let mut child = command
            .spawn()
            .map_err(|err| AppError::InvalidInput(format!("couldn't start '{}': {err}", config.command)))?;
        let pid = child
            .id()
            .ok_or_else(|| AppError::Internal("process exited before its PID could be read".into()))?;

        let stdin = child.stdin.take().expect("stdin was piped above");
        let stdout = child.stdout.take().expect("stdout was piped above");
        let stderr = child.stderr.take().expect("stderr was piped above");

        let output = Arc::new(StdMutex::new(OutputBuffer::new()));
        let exit_status = Arc::new(StdMutex::new(None));
        let (stdin_tx, mut stdin_rx) = mpsc::unbounded_channel::<Vec<u8>>();

        spawn_output_pump(stdout, output.clone());
        spawn_output_pump(stderr, output.clone());

        // Owns `stdin` and forwards writes to it until either every
        // `stdin_tx` clone is dropped (the process record was removed) or a
        // write fails (the process closed its stdin, e.g. right before
        // exiting).
        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(data) = stdin_rx.recv().await {
                if stdin.write_all(&data).await.is_err() {
                    break;
                }
            }
        });

        // Owns `child` exclusively - the only place `wait()` is called, and
        // the only writer to `exit_status`.
        {
            let exit_status = exit_status.clone();
            tokio::spawn(async move {
                if let Ok(status) = child.wait().await {
                    *exit_status.lock().unwrap() = Some(status);
                }
            });
        }

        processes.insert(
            application_id,
            RunningProcess {
                pid,
                started_at: Instant::now(),
                stdin_tx,
                output,
                exit_status,
                stop_requested: Arc::new(AtomicBool::new(false)),
            },
        );
        Ok(())
    }

    async fn mark_stop_requested(&self, application_id: Uuid) {
        if let Some(process) = self.processes.lock().await.get(&application_id) {
            process.stop_requested.store(true, Ordering::SeqCst);
        }
    }

    async fn write_stdin(&self, application_id: Uuid, data: Vec<u8>) -> AppResult<()> {
        let processes = self.processes.lock().await;
        let process = processes
            .get(&application_id)
            .ok_or_else(|| AppError::InvalidInput("this application isn't running".into()))?;
        process
            .stdin_tx
            .send(data)
            .map_err(|_| AppError::InvalidInput("this application's process already exited".into()))
    }

    async fn tail(&self, application_id: Uuid, max_lines: usize) -> Vec<String> {
        let processes = self.processes.lock().await;
        match processes.get(&application_id) {
            Some(process) => process.output.lock().unwrap().tail(max_lines),
            None => Vec::new(),
        }
    }

    /// `None` when nothing is tracked for this application (never started
    /// this run, or already reaped by `kill`) - callers treat that as
    /// `ApplicationStatus::Stopped`, not `Unknown`: a local process
    /// genuinely has no other place it could be running.
    async fn snapshot(&self, application_id: Uuid) -> Option<ProcessSnapshot> {
        let processes = self.processes.lock().await;
        processes.get(&application_id).map(|process| ProcessSnapshot {
            pid: process.pid,
            started_at: process.started_at,
            exit_status: *process.exit_status.lock().unwrap(),
            stop_requested: process.stop_requested.load(Ordering::SeqCst),
        })
    }

    async fn remove(&self, application_id: Uuid) {
        self.processes.lock().await.remove(&application_id);
    }
}

fn spawn_output_pump(reader: impl tokio::io::AsyncRead + Unpin + Send + 'static, output: Arc<StdMutex<OutputBuffer>>) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            output.lock().unwrap().push(line);
        }
    });
}

/// Waits (bounded by `timeout`) for the application's tracked process to
/// report an exit status - used by `stop(graceful = true)` and by
/// `restart` (via `kill`, which always waits) before starting a new one.
async fn wait_for_exit(manager: &LocalProcessManager, application_id: Uuid, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match manager.snapshot(application_id).await {
            Some(ProcessSnapshot { exit_status: None, .. }) => tokio::time::sleep(Duration::from_millis(50)).await,
            _ => return,
        }
    }
}

#[cfg(unix)]
fn graceful_stop(pid: u32) -> bool {
    signal_process(pid, sysinfo::Signal::Term)
}

#[cfg(windows)]
fn graceful_stop(pid: u32) -> bool {
    use windows_sys::Win32::System::Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT};
    // Safety: GenerateConsoleCtrlEvent has no preconditions beyond a valid
    // process group id - the child was spawned with CREATE_NEW_PROCESS_GROUP
    // (see `LocalProcessManager::spawn`), making `pid` that group's leader
    // and the only process it targets.
    unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) != 0 }
}

#[cfg(unix)]
fn signal_process(pid: u32, signal: sysinfo::Signal) -> bool {
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true, ProcessRefreshKind::nothing());
    match system.process(Pid::from_u32(pid)) {
        Some(process) => process.kill_with(signal).unwrap_or(false),
        None => false,
    }
}

fn force_kill(pid: u32) -> bool {
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true, ProcessRefreshKind::nothing());
    match system.process(Pid::from_u32(pid)) {
        Some(process) => process.kill(),
        None => false,
    }
}

pub struct LocalProcessRuntime {
    manager: Arc<LocalProcessManager>,
}

impl LocalProcessRuntime {
    pub fn new(manager: Arc<LocalProcessManager>) -> Self {
        Self { manager }
    }
}

#[async_trait::async_trait]
impl ApplicationRuntime for LocalProcessRuntime {
    async fn validate(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        let config = parse_config(ctx)?;
        if config.command.trim().is_empty() {
            return Err(AppError::InvalidInput("no command configured for this application".into()));
        }
        match tokio::fs::metadata(&ctx.application.working_directory).await {
            Ok(metadata) if metadata.is_dir() => Ok(()),
            Ok(_) => Err(AppError::InvalidInput(format!("'{}' is not a directory", ctx.application.working_directory))),
            Err(_) => Err(AppError::InvalidInput(format!(
                "working directory '{}' does not exist",
                ctx.application.working_directory
            ))),
        }
    }

    async fn start(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        let config = parse_config(ctx)?;
        self.manager.spawn(ctx.application.id, &config, &ctx.application.working_directory, ctx.environment).await
    }

    async fn stop(&self, ctx: &RuntimeContext<'_>, graceful: bool) -> AppResult<()> {
        let Some(snapshot) = self.manager.snapshot(ctx.application.id).await else {
            return Err(AppError::InvalidInput("this application isn't running".into()));
        };
        if snapshot.exit_status.is_some() {
            self.manager.remove(ctx.application.id).await;
            return Ok(());
        }
        self.manager.mark_stop_requested(ctx.application.id).await;
        if !graceful_stop(snapshot.pid) {
            return Err(AppError::Internal(format!("couldn't signal process {}", snapshot.pid)));
        }
        if graceful {
            wait_for_exit(&self.manager, ctx.application.id, Duration::from_secs(10)).await;
        }
        Ok(())
    }

    async fn restart(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        if self.manager.snapshot(ctx.application.id).await.is_some() {
            self.kill(ctx).await?;
        }
        self.start(ctx).await
    }

    async fn kill(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        let Some(snapshot) = self.manager.snapshot(ctx.application.id).await else {
            return Err(AppError::InvalidInput("this application isn't running".into()));
        };
        if snapshot.exit_status.is_none() {
            self.manager.mark_stop_requested(ctx.application.id).await;
            if !force_kill(snapshot.pid) {
                return Err(AppError::Internal(format!("couldn't kill process {}", snapshot.pid)));
            }
            wait_for_exit(&self.manager, ctx.application.id, Duration::from_secs(5)).await;
        }
        self.manager.remove(ctx.application.id).await;
        Ok(())
    }

    async fn status(&self, ctx: &RuntimeContext<'_>) -> AppResult<ApplicationStatus> {
        match self.manager.snapshot(ctx.application.id).await {
            None => Ok(ApplicationStatus::Stopped),
            Some(ProcessSnapshot { exit_status: None, .. }) => Ok(ApplicationStatus::Running),
            Some(ProcessSnapshot { exit_status: Some(status), stop_requested, .. }) => {
                Ok(if stop_requested || status.success() { ApplicationStatus::Stopped } else { ApplicationStatus::Failed })
            }
        }
    }

    async fn resource_usage(&self, ctx: &RuntimeContext<'_>) -> AppResult<ResourceUsage> {
        let empty = ResourceUsage { cpu_percent: None, ram_bytes: None, uptime_seconds: None };
        let Some(snapshot) = self.manager.snapshot(ctx.application.id).await else {
            return Ok(empty);
        };
        if snapshot.exit_status.is_some() {
            return Ok(empty);
        }

        let sys_pid = Pid::from_u32(snapshot.pid);
        let mut system = System::new();
        system.refresh_processes_specifics(ProcessesToUpdate::Some(&[sys_pid]), true, ProcessRefreshKind::everything());
        // CPU usage needs two samples apart - see sysinfo::MINIMUM_CPU_UPDATE_INTERVAL's own docs.
        tokio::time::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL).await;
        system.refresh_processes_specifics(ProcessesToUpdate::Some(&[sys_pid]), true, ProcessRefreshKind::everything());

        let (cpu_percent, ram_bytes) = match system.process(sys_pid) {
            Some(process) => (Some(process.cpu_usage()), Some(process.memory())),
            None => (None, None),
        };

        Ok(ResourceUsage { cpu_percent, ram_bytes, uptime_seconds: Some(snapshot.started_at.elapsed().as_secs()) })
    }

    async fn health_check(&self, ctx: &RuntimeContext<'_>, spec: &HealthCheckSpec) -> AppResult<HealthStatus> {
        let status = self.status(ctx).await?;
        health_check::default_health_check(ctx, spec, status, Some("process exited with a non-zero status")).await
    }

    async fn console(&self, ctx: &RuntimeContext<'_>) -> AppResult<Option<Box<dyn ApplicationConsole>>> {
        if self.manager.snapshot(ctx.application.id).await.is_none() {
            return Ok(None);
        }
        Ok(Some(Box::new(LocalConsole { manager: self.manager.clone(), application_id: ctx.application.id })))
    }

    async fn logs(&self, ctx: &RuntimeContext<'_>) -> AppResult<Box<dyn LogProvider>> {
        Ok(Box::new(LocalLogs { manager: self.manager.clone(), application_id: ctx.application.id }))
    }
}

struct LocalConsole {
    manager: Arc<LocalProcessManager>,
    application_id: Uuid,
}

#[async_trait::async_trait]
impl ApplicationConsole for LocalConsole {
    async fn write(&self, input: &str) -> AppResult<()> {
        let mut data = input.as_bytes().to_vec();
        data.push(b'\n');
        self.manager.write_stdin(self.application_id, data).await
    }

    fn close(&self) {
        // A local process isn't tied to a console UI's lifecycle - closing
        // the console must not stop the process the user asked to keep
        // running, so there's nothing to do here.
    }

    fn supports_input(&self) -> bool {
        true
    }
}

struct LocalLogs {
    manager: Arc<LocalProcessManager>,
    application_id: Uuid,
}

#[async_trait::async_trait]
impl LogProvider for LocalLogs {
    async fn tail(&self, max_lines: u32) -> AppResult<Vec<String>> {
        Ok(self.manager.tail(self.application_id, max_lines as usize).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Application, HealthCheckType, RuntimeType};

    fn sleep_command() -> LocalProcessConfig {
        #[cfg(windows)]
        {
            LocalProcessConfig {
                command: "cmd".into(),
                args: vec!["/C".into(), "echo hello-from-local-process && ping -n 6 127.0.0.1 >NUL".into()],
            }
        }
        #[cfg(not(windows))]
        {
            LocalProcessConfig { command: "sh".into(), args: vec!["-c".into(), "echo hello-from-local-process; sleep 5".into()] }
        }
    }

    fn instant_exit_command(code: i32) -> LocalProcessConfig {
        #[cfg(windows)]
        {
            LocalProcessConfig { command: "cmd".into(), args: vec!["/C".into(), format!("exit {code}")] }
        }
        #[cfg(not(windows))]
        {
            LocalProcessConfig { command: "sh".into(), args: vec!["-c".into(), format!("exit {code}")] }
        }
    }

    fn working_directory() -> String {
        std::env::temp_dir().to_string_lossy().into_owned()
    }

    fn stub_application(id: Uuid) -> Application {
        Application {
            id,
            server_id: None,
            name: "Test App".to_string(),
            description: None,
            blueprint_id: "generic".to_string(),
            blueprint_version: 1,
            runtime_type: RuntimeType::LocalProcess,
            working_directory: working_directory(),
            status: ApplicationStatus::Unknown,
            last_status_check_at: None,
            health_check_type: HealthCheckType::Process,
            health_check_port_id: None,
            health_check_http_path: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    async fn wait_until_status_settles(runtime: &LocalProcessRuntime, ctx: &RuntimeContext<'_>) -> ApplicationStatus {
        let mut status = ApplicationStatus::Running;
        for _ in 0..50 {
            status = runtime.status(ctx).await.unwrap();
            if status != ApplicationStatus::Running {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        status
    }

    #[tokio::test]
    async fn start_captures_stdout_and_reports_running() {
        let manager = Arc::new(LocalProcessManager::new());
        let runtime = LocalProcessRuntime::new(manager.clone());
        let application_id = Uuid::new_v4();
        let application = stub_application(application_id);
        let config = serde_json::to_value(sleep_command()).unwrap();
        let ctx = RuntimeContext { application: &application, runtime_config: &config, environment: &[], ports: &[], connection: None };

        runtime.start(&ctx).await.unwrap();
        assert_eq!(runtime.status(&ctx).await.unwrap(), ApplicationStatus::Running);

        let mut saw_output = false;
        for _ in 0..30 {
            if manager.tail(application_id, 10).await.iter().any(|line| line.contains("hello-from-local-process")) {
                saw_output = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(saw_output, "expected the process's stdout to show up in tail()");

        runtime.kill(&ctx).await.unwrap();
    }

    #[tokio::test]
    async fn a_process_that_exits_cleanly_is_reported_as_stopped() {
        let manager = Arc::new(LocalProcessManager::new());
        let runtime = LocalProcessRuntime::new(manager.clone());
        let application_id = Uuid::new_v4();
        let application = stub_application(application_id);
        let config = serde_json::to_value(instant_exit_command(0)).unwrap();
        let ctx = RuntimeContext { application: &application, runtime_config: &config, environment: &[], ports: &[], connection: None };

        runtime.start(&ctx).await.unwrap();
        assert_eq!(wait_until_status_settles(&runtime, &ctx).await, ApplicationStatus::Stopped);
    }

    #[tokio::test]
    async fn a_process_that_exits_with_a_non_zero_code_is_reported_as_failed() {
        let manager = Arc::new(LocalProcessManager::new());
        let runtime = LocalProcessRuntime::new(manager.clone());
        let application_id = Uuid::new_v4();
        let application = stub_application(application_id);
        let config = serde_json::to_value(instant_exit_command(3)).unwrap();
        let ctx = RuntimeContext { application: &application, runtime_config: &config, environment: &[], ports: &[], connection: None };

        runtime.start(&ctx).await.unwrap();
        assert_eq!(wait_until_status_settles(&runtime, &ctx).await, ApplicationStatus::Failed);
    }

    #[tokio::test]
    async fn starting_an_already_running_application_is_rejected() {
        let manager = Arc::new(LocalProcessManager::new());
        let runtime = LocalProcessRuntime::new(manager.clone());
        let application_id = Uuid::new_v4();
        let application = stub_application(application_id);
        let config = serde_json::to_value(sleep_command()).unwrap();
        let ctx = RuntimeContext { application: &application, runtime_config: &config, environment: &[], ports: &[], connection: None };

        runtime.start(&ctx).await.unwrap();
        assert!(matches!(runtime.start(&ctx).await, Err(AppError::InvalidInput(_))));

        runtime.kill(&ctx).await.unwrap();
    }

    #[tokio::test]
    async fn stop_then_start_again_is_allowed_once_the_process_has_exited() {
        let manager = Arc::new(LocalProcessManager::new());
        let runtime = LocalProcessRuntime::new(manager.clone());
        let application_id = Uuid::new_v4();
        let application = stub_application(application_id);
        let config = serde_json::to_value(sleep_command()).unwrap();
        let ctx = RuntimeContext { application: &application, runtime_config: &config, environment: &[], ports: &[], connection: None };

        runtime.start(&ctx).await.unwrap();
        runtime.stop(&ctx, true).await.unwrap();
        assert_eq!(runtime.status(&ctx).await.unwrap(), ApplicationStatus::Stopped);

        runtime.start(&ctx).await.unwrap();
        assert_eq!(runtime.status(&ctx).await.unwrap(), ApplicationStatus::Running);
        runtime.kill(&ctx).await.unwrap();
    }

    #[tokio::test]
    async fn restart_replaces_a_running_process_with_a_new_one() {
        let manager = Arc::new(LocalProcessManager::new());
        let runtime = LocalProcessRuntime::new(manager.clone());
        let application_id = Uuid::new_v4();
        let application = stub_application(application_id);
        let config = serde_json::to_value(sleep_command()).unwrap();
        let ctx = RuntimeContext { application: &application, runtime_config: &config, environment: &[], ports: &[], connection: None };

        runtime.start(&ctx).await.unwrap();
        let first_pid = manager.snapshot(application_id).await.unwrap().pid;

        runtime.restart(&ctx).await.unwrap();
        let second_pid = manager.snapshot(application_id).await.unwrap().pid;

        assert_ne!(first_pid, second_pid);
        assert_eq!(runtime.status(&ctx).await.unwrap(), ApplicationStatus::Running);
        runtime.kill(&ctx).await.unwrap();
    }

    #[tokio::test]
    async fn console_is_none_before_start_and_accepts_input_once_running() {
        let manager = Arc::new(LocalProcessManager::new());
        let runtime = LocalProcessRuntime::new(manager.clone());
        let application_id = Uuid::new_v4();
        let application = stub_application(application_id);
        let config = serde_json::to_value(sleep_command()).unwrap();
        let ctx = RuntimeContext { application: &application, runtime_config: &config, environment: &[], ports: &[], connection: None };

        assert!(runtime.console(&ctx).await.unwrap().is_none());

        runtime.start(&ctx).await.unwrap();
        let console = runtime.console(&ctx).await.unwrap().expect("a running local process has a console");
        assert!(console.supports_input());
        console.write("hello").await.unwrap();

        runtime.kill(&ctx).await.unwrap();
    }

    #[tokio::test]
    async fn environment_variables_are_passed_to_the_child_process() {
        let manager = Arc::new(LocalProcessManager::new());
        let runtime = LocalProcessRuntime::new(manager.clone());
        let application_id = Uuid::new_v4();
        let application = stub_application(application_id);

        #[cfg(windows)]
        let config = LocalProcessConfig { command: "cmd".into(), args: vec!["/C".into(), "echo %VIBESSH_TEST_VAR%".into()] };
        #[cfg(not(windows))]
        let config = LocalProcessConfig { command: "sh".into(), args: vec!["-c".into(), "echo $VIBESSH_TEST_VAR".into()] };

        let config_value = serde_json::to_value(config).unwrap();
        let environment = vec![EnvironmentVariable { key: "VIBESSH_TEST_VAR".into(), value: "vibessh-marker-42".into(), is_secret: false }];
        let ctx = RuntimeContext { application: &application, runtime_config: &config_value, environment: &environment, ports: &[], connection: None };

        runtime.start(&ctx).await.unwrap();

        let mut saw_marker = false;
        for _ in 0..30 {
            if manager.tail(application_id, 10).await.iter().any(|line| line.contains("vibessh-marker-42")) {
                saw_marker = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(saw_marker, "expected the child's environment variable to show up in its output");
    }

    #[tokio::test]
    async fn validate_rejects_a_missing_working_directory() {
        let manager = Arc::new(LocalProcessManager::new());
        let runtime = LocalProcessRuntime::new(manager.clone());
        let application_id = Uuid::new_v4();
        let mut application = stub_application(application_id);
        application.working_directory = std::env::temp_dir().join("vibessh-definitely-not-a-real-dir-xyz").to_string_lossy().into_owned();
        let config = serde_json::to_value(sleep_command()).unwrap();
        let ctx = RuntimeContext { application: &application, runtime_config: &config, environment: &[], ports: &[], connection: None };

        assert!(runtime.validate(&ctx).await.is_err());
    }

    #[tokio::test]
    async fn validate_rejects_a_blank_command() {
        let manager = Arc::new(LocalProcessManager::new());
        let runtime = LocalProcessRuntime::new(manager.clone());
        let application_id = Uuid::new_v4();
        let application = stub_application(application_id);
        let config = serde_json::to_value(LocalProcessConfig { command: "  ".into(), args: vec![] }).unwrap();
        let ctx = RuntimeContext { application: &application, runtime_config: &config, environment: &[], ports: &[], connection: None };

        assert!(runtime.validate(&ctx).await.is_err());
    }
}
