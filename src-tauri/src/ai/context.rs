//! Turning "the Application the user was looking at" into the paragraphs a
//! model can reason from.
//!
//! **Three rules shape everything here.**
//!
//! *Collecting never fails.* `build` returns an `AiContextBundle`, not a
//! `Result`. A Node that will not answer is the single most likely state
//! for someone to be asking about, so a builder that gave up when a probe
//! failed would be useless exactly when it is needed. Every optional probe
//! is best-effort and records what it could not get in `notes`, which is
//! then put in front of the model - that is what lets it say "the Node did
//! not answer, so I cannot tell you X" instead of inventing X.
//!
//! *Everything leaves through the sanitizer.* No field is written into the
//! summary directly from a repository. Free text goes through
//! `sanitize_text`, JSON through `sanitize_json`, environment variables
//! through `sanitize_environment`.
//!
//! *What is excluded is excluded on purpose.* `Server::private_key_path`
//! and `agent_certificate_fingerprint` are not here. Neither is a secret in
//! the strict sense - a path is not a key, a fingerprint is public by
//! construction - but neither helps answer a support question either, and
//! the standing rule for this codebase is that an isolation-versus-
//! convenience call goes to isolation. Whether a key file is configured is
//! included, because "no key set" is a real and common cause of a Node that
//! will not connect.

use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;

use crate::errors::AppResult;
use crate::models::{AiContextBundle, AiContextRef, ApplicationStatus};
use crate::runtime::local_process::LocalProcessManager;
use crate::services;
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::firewall_rule_repository::FirewallRuleRepository;
use crate::storage::log_capture::LogCaptureStore;
use crate::storage::node_network_repository::NodeNetworkRepository;
use crate::storage::server_repository::ServerRepository;

use super::sanitizer;

/// How many log lines to attach.
///
/// Enough for a stack trace or a container's startup sequence, small enough
/// that the logs do not crowd out the configuration they need to be read
/// against. The tail, not the head: the failure is at the end.
const LOG_TAIL_LINES: u32 = 40;

/// The longest single log line worth sending. One line can be an entire
/// serialised request; past this it is noise that costs tokens.
const MAX_LOG_LINE_CHARS: usize = 400;

/// How long any single remote probe may take before it is given up on and
/// recorded as a gap.
///
/// This exists because `ssh::client::COMMAND_TIMEOUT` is ten minutes, which
/// is the right budget for a deliberate operation the user is watching - an
/// archive extraction, a package install - and completely wrong for a
/// best-effort probe feeding a chat message. Without a bound of its own, one
/// unresponsive Node held a whole turn for ten minutes while the panel said
/// "Thinking...", which reads as a slow model rather than a stuck probe.
///
/// The value is a judgement about what is worth waiting for: a Node that
/// cannot answer `docker inspect` in twelve seconds is itself the diagnosis,
/// and saying so beats blocking on it. Every expiry becomes a note, so the
/// model is told what is missing rather than left to assume it was fine.
const PROBE_TIMEOUT: Duration = Duration::from_secs(12);

/// A ceiling on the whole collected snapshot, applied last.
///
/// Providers differ wildly in context window and in what they charge for
/// it, and a user with a chatty container should not discover the limit as
/// a provider error. Truncation is announced in `notes` rather than done
/// silently, so the model knows the picture is partial.
const MAX_SUMMARY_CHARS: usize = 12_000;

pub struct AiContextBuilder<'a> {
    pub applications: &'a ApplicationRepository,
    pub servers: &'a ServerRepository,
    pub networks: &'a NodeNetworkRepository,
    pub firewall_rules: &'a FirewallRuleRepository,
    pub ssh_sessions: &'a SshSessionManager,
    pub local_processes: &'a Arc<LocalProcessManager>,
    pub log_capture: &'a LogCaptureStore,
}

/// Accumulates the summary and the notes as collection proceeds.
struct Collected {
    summary: String,
    sources: Vec<String>,
    notes: Vec<String>,
}

impl Collected {
    fn new() -> Self {
        Self { summary: String::new(), sources: Vec::new(), notes: Vec::new() }
    }

    fn line(&mut self, text: impl AsRef<str>) {
        self.summary.push_str(&sanitizer::sanitize_text(text.as_ref()));
        self.summary.push('\n');
    }

    /// A heading, with a blank line before it unless it is the first thing.
    fn heading(&mut self, text: &str) {
        if !self.summary.is_empty() {
            self.summary.push('\n');
        }
        self.summary.push_str(text);
        self.summary.push('\n');
    }

    fn note(&mut self, text: impl Into<String>) {
        self.notes.push(sanitizer::sanitize_text(&text.into()));
    }

    fn finish(mut self) -> AiContextBundle {
        if self.summary.chars().count() > MAX_SUMMARY_CHARS {
            self.summary = self.summary.chars().take(MAX_SUMMARY_CHARS).collect();
            self.notes.push("the collected snapshot was too long and was cut off at the end".to_string());
        }
        AiContextBundle { summary: self.summary.trim_end().to_string(), sources: self.sources, notes: self.notes }
    }
}

/// Bytes as something a person - and a model - reads without arithmetic.
fn human_bytes(bytes: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let bytes = bytes as f64;
    if bytes >= GB {
        format!("{:.1} GB", bytes / GB)
    } else {
        format!("{:.0} MB", bytes / MB)
    }
}

fn percentage(used: u64, total: u64) -> String {
    if total == 0 {
        return "unknown".to_string();
    }
    format!("{:.0}%", (used as f64 / total as f64) * 100.0)
}

fn status_word(status: ApplicationStatus) -> &'static str {
    match status {
        ApplicationStatus::Unknown => "unknown",
        ApplicationStatus::Starting => "starting",
        ApplicationStatus::Running => "running",
        ApplicationStatus::Stopping => "stopping",
        ApplicationStatus::Stopped => "stopped",
        ApplicationStatus::Failed => "failed",
    }
}

/// Runs one best-effort probe under `PROBE_TIMEOUT`.
///
/// Collapses the two failure modes a probe has - it answered with an error,
/// or it did not answer at all - into the same `Result`, because the context
/// builder treats them identically: both become a note, and neither fails
/// the turn. `label` names the probe in that note, so the model is told
/// which specific thing is missing.
async fn probe<T>(label: &str, future: impl std::future::Future<Output = AppResult<T>>) -> Result<T, String> {
    match tokio::time::timeout(PROBE_TIMEOUT, future).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => Err(format!("{label}: {err}")),
        Err(_) => {
            // Logged as well as noted: a probe that times out repeatedly is
            // a real problem with the Node, and the note only reaches the
            // model, not the operator.
            log::warn!("the AI context probe \"{label}\" gave up after {}s", PROBE_TIMEOUT.as_secs());
            Err(format!("{label}: the Node did not answer within {} seconds", PROBE_TIMEOUT.as_secs()))
        }
    }
}

impl AiContextBuilder<'_> {
    pub async fn build(&self, reference: AiContextRef) -> AiContextBundle {
        match reference {
            AiContextRef::Application { id } => self.build_application(id).await,
            AiContextRef::Node { id } => self.build_node(id).await,
        }
    }

    async fn build_application(&self, id: Uuid) -> AiContextBundle {
        let mut out = Collected::new();
        let detail = match services::get_application(self.applications, id) {
            Ok(detail) => detail,
            Err(err) => {
                // The Application is gone, or the database would not open.
                // Either way the honest thing is an empty context that says
                // so, not a fabricated one.
                out.note(format!("this Application could not be read from the local database: {err}"));
                return out.finish();
            }
        };
        out.sources.push("Application".to_string());
        let app = &detail.application;

        out.heading("Application");
        out.line(format!("Name: {}", app.name));
        if let Some(description) = &app.description {
            out.line(format!("Description: {description}"));
        }
        out.line(format!("Blueprint: {} (version {})", app.blueprint_id, app.blueprint_version));
        out.line(format!("Runtime: {:?}", app.runtime_type));
        out.line(format!("Working directory: {}", app.working_directory));
        out.line(format!("Stored status: {}", status_word(app.status)));
        match app.last_status_check_at {
            Some(at) => out.line(format!("Status last checked: {}", at.format("%Y-%m-%d %H:%M UTC"))),
            None => out.line("Status last checked: never"),
        }

        // Where it runs. Named, not just an id - an id means nothing to a
        // model and nothing to the user reading the preview.
        match app.server_id {
            None => out.line("Runs on: this machine (local)"),
            Some(server_id) => match self.servers.get(server_id) {
                Ok(Some(server)) => out.line(format!("Runs on: Node \"{}\" ({}:{})", server.name, server.host, server.ssh_port)),
                Ok(None) => out.note("the Node this Application belongs to is no longer in the local database"),
                Err(err) => out.note(format!("the Node this Application belongs to could not be read: {err}")),
            },
        }

        // A live status probe. This is the single most valuable field for a
        // diagnosis and the most likely to fail, which is exactly why its
        // failure is reported rather than swallowed - a refusal from the
        // Docker daemon *is* the answer surprisingly often.
        match probe(
            "the live status could not be checked",
            services::refresh_application_status(self.applications, self.servers, self.ssh_sessions, self.local_processes, id),
        )
        .await
        {
            Ok(status) => out.line(format!("Live status right now: {}", status_word(status))),
            Err(note) => out.note(note),
        }

        out.line(format!("Health check: {:?}", app.health_check_type));
        if let Some(path) = &app.health_check_http_path {
            out.line(format!("Health check path: {path}"));
        }

        if detail.ports.is_empty() {
            out.line("Ports: none configured");
        } else {
            out.heading("Ports");
            for port in &detail.ports {
                let published = match port.external_port {
                    Some(external) => format!("published on {external}"),
                    None => "not published".to_string(),
                };
                out.line(format!(
                    "- {} {:?} internal {}, {}, visibility {:?}{}",
                    port.name,
                    port.protocol,
                    port.internal_port,
                    published,
                    port.visibility,
                    if port.required { ", required by the blueprint" } else { "" }
                ));
            }
        }

        // Resource limits live inside `runtime_config` rather than in their
        // own columns (see `runtime::docker::DockerConfig`), so they are
        // pulled out and named here - a model should not have to know this
        // app's storage layout to answer "is it being OOM-killed".
        let memory = detail.runtime_config.get("memoryLimitMb").or_else(|| detail.runtime_config.get("memory_limit_mb"));
        let cpu = detail.runtime_config.get("cpuLimitCores").or_else(|| detail.runtime_config.get("cpu_limit_cores"));
        match (memory, cpu) {
            (None, None) => out.line("Resource limits: none set"),
            (memory, cpu) => {
                let memory = memory.and_then(|v| v.as_u64()).map(|mb| format!("{mb} MB")).unwrap_or_else(|| "unset".to_string());
                let cpu = cpu.and_then(|v| v.as_f64()).map(|c| format!("{c} cores")).unwrap_or_else(|| "unset".to_string());
                out.line(format!("Resource limits: memory {memory}, CPU {cpu}"));
            }
        }

        out.line(format!("Connected to {} other Application(s) on the same Node", detail.links.len()));

        out.heading("Environment variables (secret values are shown as ***)");
        if detail.environment.is_empty() {
            out.line("- none");
        } else {
            let rows: Vec<(String, String, bool)> =
                detail.environment.iter().map(|var| (var.key.clone(), var.value.clone(), var.is_secret)).collect();
            for (key, value) in sanitizer::sanitize_environment(&rows) {
                // Already sanitized; `line` would run the pass twice, which
                // is harmless but pointless.
                out.summary.push_str(&format!("- {key}={value}\n"));
            }
        }

        out.heading("Runtime configuration (secrets removed)");
        match serde_json::to_string_pretty(&sanitizer::sanitize_json(&detail.runtime_config)) {
            Ok(text) => out.summary.push_str(&format!("{text}\n")),
            Err(err) => out.note(format!("the runtime configuration could not be rendered: {err}")),
        }

        out.heading(&format!("Recent log lines (up to {LOG_TAIL_LINES}, oldest first)"));
        match probe(
            "the logs could not be read",
            services::application_logs(
                self.applications,
                self.servers,
                self.ssh_sessions,
                self.local_processes,
                self.log_capture,
                id,
                LOG_TAIL_LINES,
            ),
        )
        .await
        {
            Ok(lines) if lines.is_empty() => out.line("- no log output has been captured"),
            Ok(lines) => {
                out.sources.push("Logs".to_string());
                for line in lines {
                    let clipped: String = line.chars().take(MAX_LOG_LINE_CHARS).collect();
                    out.line(clipped);
                }
            }
            Err(note) => out.note(note),
        }

        out.finish()
    }

    async fn build_node(&self, id: Uuid) -> AiContextBundle {
        let mut out = Collected::new();
        let server = match self.servers.get(id) {
            Ok(Some(server)) => server,
            Ok(None) => {
                out.note("this Node is no longer in the local database");
                return out.finish();
            }
            Err(err) => {
                out.note(format!("this Node could not be read from the local database: {err}"));
                return out.finish();
            }
        };
        out.sources.push("Node".to_string());

        out.heading("Node");
        out.line(format!("Name: {}", server.name));
        out.line(format!("Address: {}:{}", server.host, server.ssh_port));
        out.line(format!("Connection mode: {:?}", server.connection_mode));
        out.line(format!("Authentication: {:?}", server.authentication_type));
        // Whether, not where. See this module's own doc comment.
        out.line(format!("Private key file configured: {}", if server.private_key_path.is_some() { "yes" } else { "no" }));
        match server.agent_status {
            Some(status) => out.line(format!("Agent status: {status:?}")),
            None => out.line("Agent status: no Vibe Agent paired"),
        }
        match &server.node_capabilities {
            Some(caps) => out.line(format!(
                "Capabilities at last probe: Docker {}, WireGuard {}, ufw {}",
                if caps.docker { "yes" } else { "no" },
                if caps.wireguard { "yes" } else { "no" },
                if caps.ufw { "yes" } else { "no" }
            )),
            // "Never probed" and "probed, found nothing" are different
            // states and the model must not conflate them - the same
            // distinction `Server::node_capabilities` documents.
            None => out.line("Capabilities: never probed, so Docker/WireGuard/ufw availability is unknown"),
        }

        out.heading("Live resource usage");
        match probe("live CPU/memory/disk could not be read from this Node", services::get_server_metrics(self.servers, self.ssh_sessions, id))
            .await
        {
            Ok(metrics) => {
                out.sources.push("Metrics".to_string());
                out.line(format!("CPU: {:.1}%", metrics.cpu_usage_percent));
                out.line(format!(
                    "Memory: {} of {} ({})",
                    human_bytes(metrics.ram_used_bytes),
                    human_bytes(metrics.ram_total_bytes),
                    percentage(metrics.ram_used_bytes, metrics.ram_total_bytes)
                ));
                out.line(format!(
                    "Disk: {} of {} ({})",
                    human_bytes(metrics.disk_used_bytes),
                    human_bytes(metrics.disk_total_bytes),
                    percentage(metrics.disk_used_bytes, metrics.disk_total_bytes)
                ));
                out.line(format!("Load average (1m): {:.2}", metrics.load_average_1m));
                out.line(format!("Uptime: {} hours", metrics.uptime_seconds / 3600));
            }
            Err(note) => out.note(note),
        }

        out.heading("Vibe Network");
        match self.networks.list() {
            Ok(members) => match members.iter().find(|member| member.server_id == id) {
                Some(member) => out.line(format!("This Node is a member, with mesh address {}", member.wireguard_ip)),
                None => out.line("This Node is not a member of the Vibe Network"),
            },
            Err(err) => out.note(format!("Vibe Network membership could not be read: {err}")),
        }

        out.heading("Firewall");
        match probe(
            "the firewall status could not be read from this Node",
            services::node_firewall_overview(self.applications, self.servers, self.networks, self.firewall_rules, self.ssh_sessions, id),
        )
        .await
        {
            Ok(overview) => {
                out.sources.push("Firewall".to_string());
                match &overview.backend {
                    Some(backend) => out.line(format!("Backend: {backend}, currently {}", if overview.active { "active" } else { "inactive" })),
                    None => out.line("No supported firewall was found on this Node"),
                }
                out.line(format!("Rules VibeSSH wants in place: {}", overview.rules.len()));
            }
            Err(note) => out.note(note),
        }

        out.heading("Applications on this Node");
        match self.applications.list() {
            Ok(applications) => {
                let mine: Vec<_> = applications.iter().filter(|app| app.server_id == Some(id)).collect();
                if mine.is_empty() {
                    out.line("- none");
                } else {
                    for app in mine {
                        out.line(format!("- {} ({}), stored status {}", app.name, app.blueprint_id, status_word(app.status)));
                    }
                }
            }
            Err(err) => out.note(format!("the Applications on this Node could not be listed: {err}")),
        }

        out.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_read_as_something_a_person_would_say() {
        assert_eq!(human_bytes(2 * 1024 * 1024 * 1024), "2.0 GB");
        assert_eq!(human_bytes(512 * 1024 * 1024), "512 MB");
    }

    /// A Node that has never reported its disk size would otherwise divide
    /// by zero and print `NaN%`, which reads to a model as a real reading.
    #[test]
    fn a_zero_total_reports_unknown_rather_than_a_nonsense_percentage() {
        assert_eq!(percentage(0, 0), "unknown");
        assert_eq!(percentage(1, 4), "25%");
    }

    #[test]
    fn the_summary_is_truncated_and_the_truncation_is_announced() {
        let mut collected = Collected::new();
        collected.line("x".repeat(MAX_SUMMARY_CHARS + 500));
        let bundle = collected.finish();
        assert!(bundle.summary.chars().count() <= MAX_SUMMARY_CHARS);
        assert!(bundle.notes.iter().any(|note| note.contains("cut off")));
    }

    /// Every line written through `Collected::line` is redacted on the way
    /// in, so no caller can forget to do it.
    #[test]
    fn lines_are_sanitized_as_they_are_added() {
        let mut collected = Collected::new();
        collected.line("connected to mysql://root:hunter2@db.internal/appdb");
        collected.note("failed: DB_PASSWORD=hunter2");
        let bundle = collected.finish();
        assert!(!bundle.summary.contains("hunter2"));
        assert!(!bundle.notes[0].contains("hunter2"));
    }
}
