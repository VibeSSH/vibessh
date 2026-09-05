use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::blueprints::BlueprintRegistry;
use crate::errors::{AppError, AppResult};
use crate::models::{
    Application, ApplicationDetail, ApplicationPort, ApplicationStatus, Blueprint, ConnectionMode,
    CreateApplicationFromBlueprintInput, EnvironmentVariable, PortInput, RegistryCredential, RuntimeType, SetHealthCheckInput,
    SetRegistryCredentialInput, SetResourceLimitsInput,
};
use crate::runtime::local_process::LocalProcessManager;
use vibessh_protocol::DesktopCommand;
use crate::runtime::{HealthStatus, ResourceUsage};
use crate::services::{self, JavaInstallation};
use crate::state::{AgentSessionManager, DnsSuffixState, LogFollow, LogFollowManager, SshSessionManager};
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::database_repository::DatabaseRepository;
use crate::storage::dns_repository::DnsRepository;
use crate::storage::firewall_rule_repository::FirewallRuleRepository;
use crate::storage::log_capture::LogCaptureStore;
use crate::storage::node_network_repository::NodeNetworkRepository;
use crate::storage::registry_credential_repository::RegistryCredentialRepository;
use crate::storage::server_repository::ServerRepository;

#[tauri::command]
pub fn list_applications(repo: State<ApplicationRepository>) -> AppResult<Vec<Application>> {
    services::list_applications(&repo)
}

#[tauri::command]
pub async fn list_paper_versions() -> AppResult<Vec<String>> {
    services::list_paper_versions().await
}

#[tauri::command]
pub async fn list_velocity_versions() -> AppResult<Vec<String>> {
    services::list_velocity_versions().await
}

#[tauri::command]
pub async fn list_waterfall_versions() -> AppResult<Vec<String>> {
    services::list_waterfall_versions().await
}

#[tauri::command]
pub async fn list_purpur_versions() -> AppResult<Vec<String>> {
    services::list_purpur_versions().await
}

#[tauri::command]
pub fn get_application(repo: State<ApplicationRepository>, id: Uuid) -> AppResult<ApplicationDetail> {
    services::get_application(&repo, id)
}

#[tauri::command]
pub fn list_blueprints(registry: State<BlueprintRegistry>) -> Vec<Blueprint> {
    services::list_blueprints(&registry)
}

#[tauri::command]
pub fn list_application_ports(repo: State<ApplicationRepository>, id: Uuid) -> AppResult<Vec<ApplicationPort>> {
    services::list_application_ports(&repo, id)
}

/// Which other Applications this one is allowed to reach on its Node.
///
/// Ids only - the frontend already holds the Application list this resolves
/// against, and it needs that list anyway to offer the ones not yet
/// connected.
#[tauri::command]
pub fn list_application_links(repo: State<ApplicationRepository>, id: Uuid) -> AppResult<Vec<Uuid>> {
    services::list_application_links(&repo, id)
}

/// Grants two Applications on the same Node the ability to reach each
/// other's ports. Symmetric - see `services::connect_applications`.
#[tauri::command]
pub async fn connect_applications(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    id: Uuid,
    peer_id: Uuid,
) -> AppResult<()> {
    services::connect_applications(&repo, &server_repo, &sessions, &local_process_manager, id, peer_id).await
}

#[tauri::command]
pub async fn disconnect_applications(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    id: Uuid,
    peer_id: Uuid,
) -> AppResult<()> {
    services::disconnect_applications(&repo, &server_repo, &sessions, &local_process_manager, id, peer_id).await
}

#[tauri::command]
pub async fn add_application_port(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    network_repo: State<'_, NodeNetworkRepository>,
    firewall_rule_repo: State<'_, FirewallRuleRepository>,
    sessions: State<'_, SshSessionManager>,
    id: Uuid,
    port: PortInput,
) -> AppResult<ApplicationPort> {
    services::add_application_port(&repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, id, &port).await
}

#[tauri::command]
pub async fn update_application_port(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    network_repo: State<'_, NodeNetworkRepository>,
    firewall_rule_repo: State<'_, FirewallRuleRepository>,
    sessions: State<'_, SshSessionManager>,
    id: Uuid,
    port_id: Uuid,
    port: PortInput,
) -> AppResult<ApplicationPort> {
    services::update_application_port(&repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, id, port_id, &port).await
}

/// "Sync Firewall" (Etap M2, Ports tab) - manually re-applies the current
/// desired rule set (this Application's Node's own SSH port plus every
/// published port across every Application on it), on top of the automatic
/// best-effort sync `add_application_port`/`update_application_port`
/// already trigger. Useful for a port declared before this feature
/// existed, or after a sync that failed the first time (host unreachable,
/// etc.) - always safe to re-run, additive and idempotent.
#[tauri::command]
pub async fn sync_application_node_firewall(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    network_repo: State<'_, NodeNetworkRepository>,
    firewall_rule_repo: State<'_, FirewallRuleRepository>,
    sessions: State<'_, SshSessionManager>,
    id: Uuid,
) -> AppResult<Option<services::FirewallSyncResult>> {
    services::sync_application_node_firewall(&repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, id).await
}

#[tauri::command]
pub async fn remove_application_port(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    network_repo: State<'_, NodeNetworkRepository>,
    firewall_rule_repo: State<'_, FirewallRuleRepository>,
    sessions: State<'_, SshSessionManager>,
    id: Uuid,
    port_id: Uuid,
) -> AppResult<()> {
    services::remove_application_port(&repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, id, port_id).await
}

#[tauri::command]
pub async fn create_application(
    repo: State<'_, ApplicationRepository>,
    registry: State<'_, BlueprintRegistry>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    java_root: State<'_, crate::state::JavaRoot>,
    input: CreateApplicationFromBlueprintInput,
) -> AppResult<ApplicationDetail> {
    services::create_application(&repo, &registry, &server_repo, &sessions, &java_root.0, input).await
}

#[tauri::command]
pub async fn update_application_config(
    repo: State<'_, ApplicationRepository>,
    registry: State<'_, BlueprintRegistry>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    java_root: State<'_, crate::state::JavaRoot>,
    id: Uuid,
    field_values: serde_json::Value,
) -> AppResult<ApplicationDetail> {
    services::update_application_config(&repo, &registry, &server_repo, &sessions, &java_root.0, id, field_values).await
}

/// Deleting an Application is a real teardown, not just a row delete - it
/// destroys the container, drops the databases, revokes the firewall rules,
/// removes the DNS name and removes the Node-side account. See
/// `services::delete_application` for why, and for the order.
///
/// Returns a report rather than `()` so the UI can tell a clean removal
/// apart from a partial one: if the Node was unreachable, the container is
/// still running and still holding its published port, and the operator
/// needs to know that rather than being told the delete succeeded.
///
/// `removeFiles` is passed explicitly by the caller and defaults to off -
/// it is the only step that destroys the operator's own data.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn delete_application(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    db_repo: State<'_, DatabaseRepository>,
    network_repo: State<'_, NodeNetworkRepository>,
    firewall_rule_repo: State<'_, FirewallRuleRepository>,
    dns_repo: State<'_, DnsRepository>,
    dns_suffix: State<'_, DnsSuffixState>,
    sessions: State<'_, SshSessionManager>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    log_capture: State<'_, LogCaptureStore>,
    id: Uuid,
    remove_files: Option<bool>,
) -> AppResult<services::ApplicationTeardownReport> {
    services::delete_application(
        &repo,
        &server_repo,
        &db_repo,
        &network_repo,
        &firewall_rule_repo,
        &dns_repo,
        &sessions,
        &local_process_manager,
        &log_capture,
        &dns_suffix.get(),
        id,
        services::ApplicationDeleteOptions { drop_databases: true, remove_files: remove_files.unwrap_or(false) },
    )
    .await
}

/// Writes a visible break into the captured log when a new run begins.
///
/// `log_capture` deliberately keeps output across a restart or a recreate,
/// so a brand new container's empty buffer never looks like "no logs" and
/// the lines explaining *why* something died survive the thing dying. The
/// cost of that is what a user reported: after a restart the previous
/// session simply continued, with nothing marking where the old run ended
/// and the new one began.
///
/// A separator keeps both properties. Clearing instead would answer the
/// same complaint by destroying the crash evidence the capture exists for.
///
/// Deliberately not translated: this is written into the stored log, which
/// outlives the session and may be read by somebody else, so it must not
/// change language with the interface. Never fatal - a missing separator is
/// cosmetic and must not fail an action that already succeeded.
async fn mark_new_log_session(log_capture: &LogCaptureStore, id: Uuid) {
    let line = format!(
        "===== VibeSSH: new session started at {} =====",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );
    if let Err(err) = log_capture.append(id, &[line]).await {
        log::warn!("couldn't mark a new log session for {id}: {err}");
    }
}

/// Ends a stream that has been displaced or asked to stop.
///
/// Its own function because the two transports end differently and every
/// path out of a console has to take one of them - a missed one is a
/// `docker logs -f` running on somebody's Node and, over SSH, one of the ten
/// sessions `sshd` allows per connection.
async fn end_follow(agents: &AgentSessionManager, follow: LogFollow) {
    match follow {
        // Dropping the handle closes the channel.
        LogFollow::Ssh(_) => {}
        LogFollow::Agent { server_id, follow_id } => {
            agents.unsubscribe_logs(follow_id).await;
            agents.send_command(server_id, DesktopCommand::StopFollowingLogs { follow_id }).await;
        }
    }
}

/// Opens a live console stream. Returns as soon as the follow is running;
/// output arrives on `applog://{follow_id}/line` and the stream's end on
/// `applog://{follow_id}/closed`.
///
/// **Two transports, one contract.** An Agent-mode Node already holds a
/// persistent connection, so the Agent runs the follow and pushes lines
/// over it - one connection per Node however many consoles are open, which
/// is how Pterodactyl's Wings does it and why that feels instant. An
/// SSH-mode Node has no such channel, so the Desktop opens its own and runs
/// `docker logs -f` there: one remote process per console, the best SSH
/// allows.
///
/// The frontend sees no difference. Both paths emit the same events under
/// the same id, so the console component contains no transport knowledge at
/// all - and neither will the next thing that wants live output.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn follow_application_logs(
    app: AppHandle,
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    agents: State<'_, AgentSessionManager>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    follows: State<'_, LogFollowManager>,
    id: Uuid,
    tail: u32,
    follow_id: String,
) -> AppResult<()> {
    let line_event = format!("applog://{follow_id}/line");
    let closed_event = format!("applog://{follow_id}/closed");

    let detail = services::get_application(&repo, id)?;
    if detail.application.runtime_type != RuntimeType::Docker {
        return Err(AppError::InvalidInput("live output is only available for Docker applications".to_string()));
    }
    log::info!("opening a live console for {} ({:?})", detail.application.name, detail.application.runtime_type);

    // Agent-mode: ask the Node's own daemon to do the following.
    if let Some(server_id) = detail.application.server_id {
        let server = server_repo.get(server_id)?.ok_or_else(|| AppError::NotFound(format!("server {server_id}")))?;
        if server.connection_mode == ConnectionMode::Agent {
            let parsed = Uuid::parse_str(&follow_id).map_err(|_| AppError::InvalidInput("the follow id must be a UUID".to_string()))?;
            let mut lines = agents.subscribe_logs(parsed).await;
            let container = format!("vibessh-app-{id}");
            if !agents
                .send_command(server_id, DesktopCommand::FollowLogs { follow_id: parsed, container, tail })
                .await
            {
                agents.unsubscribe_logs(parsed).await;
                return Err(AppError::Connection("this Node's agent isn't connected".to_string()));
            }

            tokio::spawn(async move {
                while let Some(line) = lines.recv().await {
                    if let Err(err) = app.emit(&line_event, line) {
                        log::warn!("couldn't deliver a console line to the UI: {err}");
                        break;
                    }
                }
                // The sink closed - the console unsubscribed, or the app is
                // shutting down. Telling the UI keeps its "live" label
                // honest instead of leaving it claiming a stream that ended.
                if let Err(err) = app.emit(&closed_event, None::<String>) {
                    log::warn!("couldn't tell the UI the console stream ended: {err}");
                }
            });

            // Keyed by Application: this ends whatever console was open on
            // it, which is what stops an abandoned follow surviving a
            // remount or a reconnect.
            if let Some(previous) = follows.replace(id, LogFollow::Agent { server_id, follow_id: parsed }).await {
                end_follow(&agents, previous).await;
            }
            return Ok(());
        }
    }

    // SSH-mode: the Desktop runs the follow itself.
    let app_for_lines = app.clone();
    let handle = services::follow_application_logs(
        &repo,
        &server_repo,
        &sessions,
        &local_process_manager,
        id,
        tail,
        move |line| {
            if let Err(err) = app_for_lines.emit(&line_event, line) {
                log::warn!("couldn't deliver a console line to the UI: {err}");
            }
        },
        move |reason| {
            if let Err(err) = app.emit(&closed_event, reason) {
                log::warn!("couldn't tell the UI the console stream ended: {err}");
            }
        },
    )
    .await?;

    if let Some(previous) = follows.replace(id, LogFollow::Ssh(handle)).await {
        end_follow(&agents, previous).await;
    }
    log::info!("live console streaming over SSH for application {id}");
    Ok(())
}

/// Stops the console stream on an Application. `false` means there was
/// nothing running, which is a race rather than an error.
///
/// Takes the Application rather than the follow id, matching the registry:
/// one Application has one console, and a caller that has lost track of
/// which follow it started can still end it.
#[tauri::command]
pub async fn stop_following_application_logs(
    agents: State<'_, AgentSessionManager>,
    follows: State<'_, LogFollowManager>,
    id: Uuid,
) -> AppResult<bool> {
    match follows.take(id).await {
        None => Ok(false),
        Some(follow) => {
            end_follow(&agents, follow).await;
            Ok(true)
        }
    }
}

#[tauri::command]
pub async fn start_application(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    registry_repo: State<'_, RegistryCredentialRepository>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    log_capture: State<'_, LogCaptureStore>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    let status = services::start_application(&repo, &server_repo, &sessions, &registry_repo, &local_process_manager, id).await?;
    mark_new_log_session(&log_capture, id).await;
    Ok(status)
}

#[tauri::command]
pub async fn stop_application(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    id: Uuid,
    graceful: bool,
) -> AppResult<ApplicationStatus> {
    services::stop_application(&repo, &server_repo, &sessions, &local_process_manager, id, graceful).await
}

#[tauri::command]
pub async fn restart_application(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    log_capture: State<'_, LogCaptureStore>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    let status = services::restart_application(&repo, &server_repo, &sessions, &local_process_manager, id).await?;
    mark_new_log_session(&log_capture, id).await;
    Ok(status)
}

/// "Recreate Container" (Etap M1, Docker only) - tears the container down
/// and creates it again from the Application's current config, so an edited
/// image/command/resource limit/restart policy actually takes effect.
#[tauri::command]
pub async fn recreate_application(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    registry_repo: State<'_, RegistryCredentialRepository>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    services::recreate_application(&repo, &server_repo, &sessions, &registry_repo, &local_process_manager, id).await
}

#[tauri::command]
pub async fn kill_application(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    services::kill_application(&repo, &server_repo, &sessions, &local_process_manager, id).await
}

#[tauri::command]
pub async fn refresh_application_status(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    services::refresh_application_status(&repo, &server_repo, &sessions, &local_process_manager, id).await
}

/// Renames an Application. See `services::rename_application` for what a
/// rename does and does not move - notably the network alias other
/// Applications resolve this one by, which follows the name from the next
/// restart onwards.
#[tauri::command]
pub async fn rename_application(
    repo: State<'_, ApplicationRepository>,
    id: Uuid,
    name: String,
) -> AppResult<ApplicationDetail> {
    services::rename_application(&repo, id, &name)
}

#[tauri::command]
pub async fn get_application_resource_usage(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    id: Uuid,
) -> AppResult<ResourceUsage> {
    services::application_resource_usage(&repo, &server_repo, &sessions, &local_process_manager, id).await
}

#[tauri::command]
pub async fn get_application_logs(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    log_capture: State<'_, LogCaptureStore>,
    id: Uuid,
    max_lines: u32,
) -> AppResult<Vec<String>> {
    services::application_logs(&repo, &server_repo, &sessions, &local_process_manager, &log_capture, id, max_lines).await
}

#[tauri::command]
pub async fn write_application_console(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    id: Uuid,
    input: String,
) -> AppResult<()> {
    services::application_console_write(&repo, &server_repo, &sessions, &local_process_manager, id, &input).await
}

#[tauri::command]
pub async fn get_application_health(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    id: Uuid,
) -> AppResult<HealthStatus> {
    services::application_health_check(&repo, &server_repo, &sessions, &local_process_manager, id).await
}

#[tauri::command]
pub fn set_application_health_check(
    repo: State<ApplicationRepository>,
    id: Uuid,
    input: SetHealthCheckInput,
) -> AppResult<ApplicationDetail> {
    services::set_application_health_check(&repo, id, input)
}

#[tauri::command]
pub fn set_application_resource_limits(
    repo: State<ApplicationRepository>,
    id: Uuid,
    input: SetResourceLimitsInput,
) -> AppResult<ApplicationDetail> {
    services::set_application_resource_limits(&repo, id, input)
}

#[tauri::command]
pub fn set_application_environment(
    repo: State<ApplicationRepository>,
    id: Uuid,
    environment: Vec<EnvironmentVariable>,
) -> AppResult<ApplicationDetail> {
    services::set_application_environment(&repo, id, environment)
}

#[tauri::command]
pub fn set_application_image(repo: State<ApplicationRepository>, id: Uuid, image: String) -> AppResult<ApplicationDetail> {
    services::set_application_image(&repo, id, image)
}

#[tauri::command]
pub async fn pull_application_image(
    repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    registry_repo: State<'_, RegistryCredentialRepository>,
    id: Uuid,
) -> AppResult<String> {
    services::pull_application_image(&repo, &server_repo, &sessions, &registry_repo, id).await
}

#[tauri::command]
pub fn list_registry_credentials(registry_repo: State<RegistryCredentialRepository>) -> AppResult<Vec<RegistryCredential>> {
    services::list_registry_credentials(&registry_repo)
}

#[tauri::command]
pub fn set_registry_credential(registry_repo: State<RegistryCredentialRepository>, input: SetRegistryCredentialInput) -> AppResult<RegistryCredential> {
    services::set_registry_credential(&registry_repo, input)
}

#[tauri::command]
pub fn remove_registry_credential(registry_repo: State<RegistryCredentialRepository>, id: Uuid) -> AppResult<()> {
    services::remove_registry_credential(&registry_repo, id)
}

#[tauri::command]
pub async fn detect_java_installations(
    server_repo: State<'_, ServerRepository>,
    sessions: State<'_, SshSessionManager>,
    server_id: Option<Uuid>,
) -> AppResult<Vec<JavaInstallation>> {
    services::detect_java_installations(&server_repo, &sessions, server_id).await
}
