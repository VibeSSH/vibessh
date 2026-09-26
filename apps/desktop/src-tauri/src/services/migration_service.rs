//! Service migration (deferred piece of the original Vibe Network plan,
//! design doc §7) - moving a Docker Application from one Node to another:
//! provision an identical Application on the target, copy `working_directory`
//! byte-for-byte through the same `files::ApplicationFileProvider` every
//! Application already uses, repoint its DNS alias if it has one (the
//! hostname itself never changes - see `dns_service`'s own doc comment for
//! why repointing which Application it resolves through is the whole
//! story), then retire the old instance. Explicit, orchestrated,
//! UI-triggered - not live failover, since there's no HA/control-plane layer
//! in this codebase (a deliberate scope line, not a gap - see the design
//! doc's own §5 answer on where a future one would plug in).
//!
//! **Docker-only, on purpose**: the other three runtime types predate the
//! "Docker mandatory" direction (Etap M1) and were kept only for
//! backward compatibility with Applications that already used them -
//! extending migration to them would mean building separate,
//! never-to-be-used-again process-migration logic for a runtime nothing new
//! can be created with. Rejected outright, same "don't pretend two runtimes
//! have identical capabilities" stance the rest of this codebase already
//! takes (see `set_application_resource_limits`'s own doc comment).

use std::sync::Arc;

use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::files::{self, ApplicationFileProvider};
use crate::models::{ApplicationDetail, ApplicationStatus, CreateApplicationInput, HealthCheckType, PortInput, RuntimeType, SetHealthCheckInput};
use crate::runtime::local_process::LocalProcessManager;
use crate::services::application_service;
use crate::services::dns_service;
use crate::services::firewall_service;
use crate::services::schedule_service;
use crate::services::ssh_service::get_or_connect;
use crate::ssh::command::quote as shell_quote;
use crate::ssh::SshSession;
use crate::state::{MigrationLockManager, SshSessionManager};
use crate::storage::application_backup_repository::ApplicationBackupRepository;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::database_repository::DatabaseRepository;
use crate::storage::dns_repository::DnsRepository;
use crate::storage::firewall_rule_repository::FirewallRuleRepository;
use crate::storage::log_capture::LogCaptureStore;
use crate::storage::node_network_repository::NodeNetworkRepository;
use crate::storage::application_schedule_repository::ApplicationScheduleRepository;
use crate::storage::registry_credential_repository::RegistryCredentialRepository;
use crate::storage::server_repository::ServerRepository;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationResult {
    pub application: ApplicationDetail,
    pub files_copied: u64,
    pub dns_repointed: bool,
    /// Whether the migrated Application is actually running on the target.
    ///
    /// The start used to be fire-and-forget, so a migration that produced a
    /// correctly configured but *stopped* Application on an unreachable Node
    /// reported plain success (U-005). It is still not a reason to unwind
    /// the migration - the data is copied and the row is right - but the
    /// operator has to be told, because "migrated" reads as "running".
    pub started: bool,
    /// Every step after the point of no return that did not complete:
    /// the DNS sync, the start, retiring the source, and the firewall
    /// reconcile on each Node. Empty means the migration finished clean.
    ///
    /// These are warnings rather than errors on purpose - by the time any of
    /// them can fail, the target Application exists with the source's data
    /// and the migration has succeeded in the sense that matters. Returning
    /// an error would tell the operator to retry something that must not be
    /// retried.
    pub warnings: Vec<String>,
}

/// Where a migration has got to, sent to the UI as it goes.
///
/// A migration of a real Minecraft server copies thousands of files and can
/// run for many minutes; it used to be one silent call behind a button that
/// said "Migrating..." the whole time, with nothing in the log either, so a
/// slow copy and a hung one looked the same.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationProgress {
    pub phase: MigrationPhase,
    pub files_done: u64,
    pub files_total: u64,
    /// Measured against the sizes in the directory listing, so the
    /// percentage is by data rather than by file count - one world file can
    /// outweigh a thousand plugin configs.
    pub bytes_done: u64,
    pub bytes_total: u64,
    /// The file being copied right now, relative to the working directory.
    pub current: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MigrationPhase {
    Stopping,
    Preparing,
    Scanning,
    Copying,
    Starting,
    Finishing,
}

impl MigrationProgress {
    fn phase(phase: MigrationPhase) -> Self {
        Self { phase, files_done: 0, files_total: 0, bytes_done: 0, bytes_total: 0, current: None }
    }
}

/// Receives progress. `Sync` because it is held across awaits in a future
/// Tauri needs to be `Send`.
pub type ProgressSink<'a> = &'a (dyn Fn(MigrationProgress) + Send + Sync);

#[allow(clippy::too_many_arguments)]
pub async fn migrate_application(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    network_repo: &NodeNetworkRepository,
    dns_repo: &DnsRepository,
    db_repo: &DatabaseRepository,
    dns_suffix: &str,
    firewall_rule_repo: &FirewallRuleRepository,
    registry_repo: &RegistryCredentialRepository,
    schedule_repo: &ApplicationScheduleRepository,
    backup_repo: &ApplicationBackupRepository,
    log_capture: &LogCaptureStore,
    sessions: &SshSessionManager,
    locks: &MigrationLockManager,
    local_process_manager: &Arc<LocalProcessManager>,
    source_application_id: Uuid,
    target_server_id: Uuid,
    progress: ProgressSink<'_>,
) -> AppResult<MigrationResult> {
    crate::services::application_service::refuse_if_shared(app_repo, source_application_id, "applications.config")?;
    if !locks.try_start(source_application_id).await {
        return Err(AppError::InvalidInput("this application is already being migrated".into()));
    }
    let result = migrate_application_inner(
        app_repo,
        server_repo,
        network_repo,
        dns_repo,
        db_repo,
        dns_suffix,
        firewall_rule_repo,
        registry_repo,
        schedule_repo,
        backup_repo,
        log_capture,
        sessions,
        local_process_manager,
        source_application_id,
        target_server_id,
        progress,
    )
    .await;
    locks.finish(source_application_id).await;
    result
}

#[allow(clippy::too_many_arguments)]
async fn migrate_application_inner(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    network_repo: &NodeNetworkRepository,
    dns_repo: &DnsRepository,
    db_repo: &DatabaseRepository,
    dns_suffix: &str,
    firewall_rule_repo: &FirewallRuleRepository,
    registry_repo: &RegistryCredentialRepository,
    schedule_repo: &ApplicationScheduleRepository,
    backup_repo: &ApplicationBackupRepository,
    log_capture: &LogCaptureStore,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    source_application_id: Uuid,
    target_server_id: Uuid,
    progress: ProgressSink<'_>,
) -> AppResult<MigrationResult> {
    let source = app_repo
        .get(source_application_id)?
        .ok_or_else(|| AppError::NotFound(format!("application {source_application_id}")))?;

    if source.application.runtime_type != RuntimeType::Docker {
        return Err(AppError::InvalidInput("only Docker applications can be migrated between Nodes today".into()));
    }
    if source.application.server_id == Some(target_server_id) {
        return Err(AppError::InvalidInput("the target Node must be different from the application's current Node".into()));
    }
    server_repo.get(target_server_id)?.ok_or_else(|| AppError::NotFound(format!("server {target_server_id}")))?;

    // Before anything is stopped: databases hosted on this Node do not move,
    // and going ahead left them behind untracked while the Application
    // started somewhere its connection could not reach them.
    let databases = db_repo.list_databases(source_application_id)?;
    if !databases.is_empty() {
        let databases = databases.iter().map(|database| database.database_name.clone()).collect::<Vec<_>>().join(", ");
        return Err(AppError::MigrationHasDatabases { databases });
    }

    // Stop the source first - copying a directory a running Application is
    // still writing to (a Minecraft server saving its world, a database
    // flushing a page) would copy it mid-write.
    //
    // Decided from the Node, not from the stored status: a server a schedule
    // started while the record still said Stopped was copied while running.
    log::info!("migrating {} ({source_application_id}) to node {target_server_id}", source.application.name);
    let status = application_service::refresh_application_status(app_repo, server_repo, sessions, local_process_manager, source_application_id)
        .await
        .unwrap_or(source.application.status);
    let was_running = matches!(status, ApplicationStatus::Running | ApplicationStatus::Starting);
    if was_running {
        progress(MigrationProgress::phase(MigrationPhase::Stopping));
        application_service::stop_application(app_repo, server_repo, sessions, local_process_manager, source_application_id, true).await?;
    }

    // Step 1: provision the target Application with the exact same config
    // the source already has - not re-run through blueprint provisioning
    // (that would re-fetch "latest Paper build" or similar instead of
    // moving what's actually there), so this goes straight through the
    // repository the same way `application_service::create_application`
    // does internally, once its own blueprint step is done.
    // `source.environment` comes back from `app_repo.get` with every secret
    // row redacted (see `models::EnvironmentVariable::value`'s own doc
    // comment) - resolved back to real values here since this whole
    // function's job is to reproduce the source Application exactly on the
    // target, not to reproduce it with its secrets silently blanked out.
    let source_environment = application_service::resolve_environment_secrets(source_application_id, source.environment.clone())?;

    let create_input = CreateApplicationInput {
        server_id: Some(target_server_id),
        name: source.application.name.clone(),
        description: source.application.description.clone(),
        blueprint_id: source.application.blueprint_id.clone(),
        blueprint_version: source.application.blueprint_version,
        runtime_type: source.application.runtime_type,
        working_directory: source.application.working_directory.clone(),
        environment: source_environment,
        ports: vec![],
        runtime_config: source.runtime_config.clone(),
        metadata: source.metadata.clone(),
    };
    progress(MigrationProgress::phase(MigrationPhase::Preparing));
    application_service::ensure_working_directory_exists(server_repo, sessions, Some(target_server_id), &create_input.working_directory).await?;
    let created = app_repo.create(&create_input)?;
    let target_application_id = created.application.id;

    // Steps 2-4 (ports, health check, the file copy itself) are the part
    // that can genuinely fail partway - a bad SSH connection, a disk-full
    // target - and unlike steps 5-6 below, failing here means the source
    // Application is still perfectly intact and untouched. So on any error
    // here, the just-created target row is rolled back (best-effort) rather
    // than left behind as a broken, empty duplicate in the Applications
    // list - the source is exactly where it started, and the caller gets a
    // real error to retry, not a half-finished migration to clean up by hand.
    let files_copied = match provision_target(app_repo, server_repo, network_repo, firewall_rule_repo, sessions, &source, &created.application, target_server_id, progress).await {
        Ok(files_copied) => files_copied,
        Err(err) => {
            let err = roll_back_target(app_repo, target_application_id, err);
            return Err(restart_source(app_repo, server_repo, sessions, registry_repo, local_process_manager, source_application_id, was_running, err).await);
        }
    };
    // Only written once the target row is otherwise fully provisioned - on
    // any earlier failure above, the target row (and so its keyring
    // namespace) is rolled back, so there'd be nothing to clean up; on a
    // failure here, the same rollback still applies rather than leaving a
    // target Application missing its secrets.
    if let Err(err) = application_service::store_secret_environment_values(target_application_id, &create_input.environment) {
        let err = roll_back_target(app_repo, target_application_id, err);
        return Err(restart_source(app_repo, server_repo, sessions, registry_repo, local_process_manager, source_application_id, was_running, err).await);
    }

    // Step 5: cut the DNS alias over, if this service has one - the
    // hostname never changes, only which Application it resolves through.
    let mut warnings: Vec<String> = Vec::new();
    let dns_repointed = dns_repo.repoint_application(source_application_id, target_application_id)?.is_some();
    if dns_repointed {
        // A failed sync means the stored record points at the new instance
        // while `/etc/hosts` on every Node still resolves the old one - the
        // hostname keeps working, at the wrong address, which is worse than
        // it not working at all and so must not pass silently.
        if let Err(err) = dns_service::sync_dns(dns_suffix, network_repo, server_repo, app_repo, dns_repo, sessions).await {
            warnings.push(format!("the DNS name still resolves to the old instance until the next sync: {err}"));
        }
    }

    // Step 6: bring the new instance up, then retire the old one. Both are
    // best-effort past this point - the migration itself (a new, fully
    // configured Application with the old one's data) has already
    // succeeded; a start failure or a firewall sync hiccup is now the same
    // kind of already-surfaced, retryable problem as it would be for any
    // other Application, not a reason to unwind everything above.
    progress(MigrationProgress::phase(MigrationPhase::Starting));
    let started = match application_service::start_application(app_repo, server_repo, sessions, registry_repo, local_process_manager, target_application_id).await {
        Ok(_) => true,
        Err(err) => {
            warnings.push(format!("the migrated application didn't start on the target node: {err}"));
            false
        }
    };
    // Carry captured log history over to the new id before the source row
    // (and, if this were skipped, its own orphaned capture file) is retired
    // - see `LogCaptureStore::rename`'s own doc comment.
    progress(MigrationProgress::phase(MigrationPhase::Finishing));
    log_capture.rename(source_application_id, target_application_id).await;
    // Before the source row goes: its schedules would go with it, by cascade.
    // They follow the Application to its new Node, and the source's cron
    // file is removed by the teardown just below.
    if let Err(err) = schedule_service::move_schedules(server_repo, sessions, app_repo, schedule_repo, source_application_id, target_application_id).await {
        warnings.push(format!("the application's schedules weren't set up on the new node - open its Schedules tab and save one to retry: {err}"));
    }
    // The backup files travelled inside the working directory; their records
    // and the backup schedule follow them, instead of going with the source
    // row by cascade and leaving the copied files listed nowhere.
    if let Err(err) = backup_repo.move_to_application(source_application_id, target_application_id) {
        warnings.push(format!("the application's backups weren't carried over to its list, though the files were copied: {err}"));
    }
    // A connection is a Docker network on one Node, so it cannot follow an
    // Application to another. It used to disappear without a word.
    if !source.links.is_empty() {
        let peers: Vec<String> = source
            .links
            .iter()
            .map(|peer| app_repo.get(*peer).ok().flatten().map(|detail| detail.application.name).unwrap_or_else(|| peer.to_string()))
            .collect();
        warnings.push(format!(
            "its connections to {} were removed - applications on different Nodes can't share a Docker network; reach them over Vibe Network instead",
            peers.join(", ")
        ));
    }
    // Retires the source instance: destroys its container, removes its
    // Node-side identity, and revokes its firewall rules. Deliberately
    // neither drops databases (migration does not move them, so dropping
    // would destroy data the operator still has) nor deletes files (they
    // are the originals this migration just copied from) - see
    // `ApplicationDeleteOptions`.
    let teardown = application_service::delete_application(
        app_repo,
        server_repo,
        db_repo,
        network_repo,
        firewall_rule_repo,
        dns_repo,
        sessions,
        local_process_manager,
        log_capture,
        dns_suffix,
        source_application_id,
        application_service::ApplicationDeleteOptions::default(),
    )
    .await?;
    for warning in teardown.warnings {
        // Was log-only. Retiring the source is where an orphaned container
        // holding the old published port comes from, and the operator is the
        // only one who can act on that - a log line they never open is not
        // telling them.
        warnings.push(format!("the old instance wasn't fully retired: {warning}"));
    }
    if let Some(source_server_id) = source.application.server_id {
        if let Err(err) = firewall_service::reconcile_node(app_repo, server_repo, network_repo, firewall_rule_repo, sessions, source_server_id).await {
            warnings.push(format!("the old node's firewall still allows this application's ports: {err}"));
        }
    }
    match firewall_service::reconcile_node(app_repo, server_repo, network_repo, firewall_rule_repo, sessions, target_server_id).await {
        Ok(result) => {
            if let Some(err) = result.container_error {
                warnings.push(format!("the new node doesn't restrict this application's Vibe Network ports: {err}"));
            }
        }
        Err(err) => warnings.push(format!("the new node's firewall wasn't updated for this application's ports: {err}")),
    }

    let final_detail = application_service::get_application(app_repo, target_application_id)?;
    log::info!("migrated {} to node {target_server_id}: {files_copied} files, {} warning(s)", final_detail.application.name, warnings.len());
    Ok(MigrationResult { application: final_detail, files_copied, dns_repointed, started, warnings })
}

/// Undoes the target row created at the start of a migration that then
/// failed before the point of no return, and folds a failed undo into the
/// error the caller is about to see.
///
/// The delete used to be discarded. When it failed, the operator got the
/// original error and a broken, empty duplicate Application in their list
/// with no indication where it came from - and the natural next move, retry
/// the migration, then hit a name collision instead.
/// Starts the source again after a failed migration, if it was running
/// when the migration stopped it - otherwise a failure left the server down
/// while the error only talked about the target. Best-effort, and said in
/// the error when it fails too.
#[allow(clippy::too_many_arguments)]
async fn restart_source(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    registry_repo: &RegistryCredentialRepository,
    local_process_manager: &Arc<LocalProcessManager>,
    source_application_id: Uuid,
    was_running: bool,
    err: AppError,
) -> AppError {
    if !was_running {
        return err;
    }
    match application_service::start_application(app_repo, server_repo, sessions, registry_repo, local_process_manager, source_application_id).await {
        Ok(_) => err,
        Err(start_err) => {
            log::error!("the migration failed and the source {source_application_id} couldn't be started again: {start_err}");
            AppError::Internal(format!("{err}. The application was stopped for the migration and couldn't be started again on its own Node ({start_err}) - start it by hand"))
        }
    }
}

fn roll_back_target(app_repo: &ApplicationRepository, target_application_id: Uuid, err: AppError) -> AppError {
    let Err(undo) = app_repo.delete(target_application_id) else {
        return err;
    };
    log::error!("couldn't roll back the half-provisioned target application {target_application_id}: {undo}");
    AppError::Internal(format!(
        "{err}. An empty '{target_application_id}' application was also left behind on the target node and couldn't be removed \
         automatically ({undo}) - delete it before retrying"
    ))
}

/// Ports, health check, and the file copy for a freshly created target
/// Application - factored out of `migrate_application_inner` so that
/// function can roll the target row back on any error from this whole
/// group in one place, rather than repeating the same rollback after every
/// individual `?`.
async fn provision_target(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    network_repo: &NodeNetworkRepository,
    firewall_rule_repo: &FirewallRuleRepository,
    sessions: &SshSessionManager,
    source: &ApplicationDetail,
    target: &crate::models::Application,
    target_server_id: Uuid,
    progress: ProgressSink<'_>,
) -> AppResult<u64> {
    // Ports first, through the same command the Ports tab itself uses, so
    // bind-address resolution and the best-effort firewall sync behave
    // identically to a user adding these by hand.
    for port in &source.ports {
        let input = PortInput {
            name: port.name.clone(),
            protocol: port.protocol,
            bind_address: port.bind_address.clone(),
            internal_port: port.internal_port,
            external_port: port.external_port,
            visibility: port.visibility,
            required: port.required,
        };
        application_service::add_application_port(app_repo, server_repo, network_repo, firewall_rule_repo, sessions, target.id, &input).await?;
    }
    let target_after_ports = application_service::get_application(app_repo, target.id)?;

    // Then the health check, remapped from the source port's id to
    // whichever new port shares its name - ids are always fresh per
    // Application (see `ApplicationPort`'s own doc comment), so the old
    // `health_check_port_id` can't be reused directly.
    if source.application.health_check_type != HealthCheckType::Process {
        if let Some(old_port) = source.application.health_check_port_id.and_then(|id| source.ports.iter().find(|p| p.id == id)) {
            if let Some(new_port) = target_after_ports.ports.iter().find(|p| p.name == old_port.name) {
                application_service::set_application_health_check(
                    app_repo,
                    target.id,
                    SetHealthCheckInput {
                        health_check_type: source.application.health_check_type,
                        port_id: Some(new_port.id),
                        http_path: source.application.health_check_http_path.clone(),
                    },
                )?;
            }
        }
    }

    // Finally the working directory's contents. Node to Node as one tar
    // stream; file by file only when the source is this computer, which has
    // no `tar` over SSH to stream from.
    let target_connection = get_or_connect(server_repo, sessions, target_server_id).await?;
    if let Some(source_server_id) = source.application.server_id {
        let source_connection = get_or_connect(server_repo, sessions, source_server_id).await?;
        return stream_directory(&source_connection, &target_connection, &source.application.working_directory, &target.working_directory, progress).await;
    }
    let source_connection = None;
    let source_provider = files::provider_for(&source.application, &source.runtime_config, source_connection)?;
    let target_connection = Some(target_connection);
    // Deliberately `Value::Null` (never resolves `wants_dedicated_user`,
    // always SFTP-as-admin for this copy) rather than threading the
    // target's own freshly-rendered `runtime_config` through: the caller
    // right after this (`start_application`, `migrate_application_inner`'s
    // line just above this function) already `chown -R`s the whole
    // `working_directory` to the dedicated user on its very first start
    // (`runtime::docker::ensure_working_directory_owned_by_dedicated_user`)
    // regardless of which provider wrote these files - so the ownership
    // this copy leaves behind is corrected a moment later either way, and
    // this avoids this function needing to separately load the target's own
    // rendered config just to make the same eventual outcome happen one
    // step earlier.
    let target_provider = files::provider_for(target, &serde_json::Value::Null, target_connection)?;
    copy_directory(source_provider.as_ref(), target_provider.as_ref(), progress).await
}

/// Moves a working directory from one Node to another as a single `tar`
/// stream through this process.
///
/// The file-by-file copy below took several SSH round trips per file - a
/// listing, a staged read, a write - and held each file whole in memory, so
/// a Minecraft server's 1,765 files took long enough to look hung. This is
/// one channel on each Node for the whole directory.
///
/// Run as root on both ends, so nothing the source can read is left behind.
/// The target extracts with GNU tar's defaults, which are what keep a
/// compromised source Node from writing outside the directory: members
/// with a `..` component are skipped, a leading `/` is stripped, and
/// symlinks are only created once everything else is in place, so no member
/// can be written through one. Files end up owned by the connecting admin,
/// exactly as the SFTP copy left them; `start_application` re-owns the
/// directory for a dedicated user on its first start either way.
async fn stream_directory(source: &SshSession, target: &SshSession, source_directory: &str, target_directory: &str, progress: ProgressSink<'_>) -> AppResult<u64> {
    progress(MigrationProgress::phase(MigrationPhase::Scanning));
    let source_quoted = shell_quote(source_directory);
    let target_quoted = shell_quote(target_directory);

    // Counted on the Node, so a large tree is three numbers over the wire
    // rather than a line per file.
    let count = source
        .execute_command(&format!(
            "sudo find {source_quoted} -mindepth 1 -printf '%y %s\\n' | awk '{{ n++; if ($1 == \"f\") {{ f++; b += int(($2 + 511) / 512) * 512 }} }} END {{ print n + 0, f + 0, b + 0 }}'"
        ))
        .await?;
    if count.exit_code != 0 {
        return Err(AppError::Connection(format!("couldn't read the application's directory on the source node: {}", count.stderr.trim())));
    }
    let (entries, files_total, file_blocks) = parse_tree_count(&count.stdout)
        .ok_or_else(|| AppError::Internal(format!("unexpected directory count from the source node: {:?}", count.stdout.trim())))?;
    let bytes_total = estimated_tar_size(entries, file_blocks);
    log::info!("migration: streaming {files_total} files (about {bytes_total} bytes as tar) from {source_directory}");

    let mut tracker = TarTracker::default();
    let mut bytes_done = 0u64;
    let mut last_report: Option<std::time::Instant> = None;
    let report = |tracker: &TarTracker, bytes_done: u64| {
        progress(MigrationProgress {
            phase: MigrationPhase::Copying,
            files_done: tracker.files_seen.min(files_total),
            files_total,
            // Never past 100% on an estimate that came out a little low -
            // long names, for one, take header blocks the count leaves out.
            bytes_done: bytes_done.min(bytes_total),
            bytes_total,
            current: tracker.current.clone(),
        })
    };
    report(&tracker, 0);

    source
        .pipe_into(
            &format!("sudo tar -C {source_quoted} -cf - ."),
            target,
            &format!("sudo tar -C {target_quoted} -xf - --no-same-owner && sudo chown -R \"$(id -u):$(id -g)\" {target_quoted}"),
            |chunk| {
                tracker.feed(chunk);
                bytes_done += chunk.len() as u64;
                if last_report.is_none_or(|at| at.elapsed() >= std::time::Duration::from_millis(150)) {
                    report(&tracker, bytes_done);
                    last_report = Some(std::time::Instant::now());
                }
            },
        )
        .await?;

    let copied = tracker.files_seen;
    progress(MigrationProgress { phase: MigrationPhase::Copying, files_done: copied, files_total: copied, bytes_done: bytes_total, bytes_total, current: None });
    Ok(copied)
}

/// `entries files file_blocks` as the Node's `awk` prints them.
fn parse_tree_count(output: &str) -> Option<(u64, u64, u64)> {
    let mut numbers = output.split_whitespace().map(str::parse::<u64>);
    let counts = (numbers.next()?.ok()?, numbers.next()?.ok()?, numbers.next()?.ok()?);
    numbers.next().is_none().then_some(counts)
}

/// How long the tar stream for a tree will be: a 512-byte header for every
/// entry plus the directory itself, each file's data rounded up to whole
/// blocks, two zero blocks at the end, all padded to tar's 10 KiB record.
fn estimated_tar_size(entries: u64, file_blocks: u64) -> u64 {
    const RECORD: u64 = 10 * 1024;
    let raw = (entries + 1) * 512 + file_blocks + 1024;
    raw.div_ceil(RECORD) * RECORD
}

/// Follows a tar stream's headers as it goes past, for the name of the file
/// being copied and how many have been.
///
/// Reads only what it needs: each 512-byte header's name, size and type,
/// skipping the data between them. GNU long names (`L` entries) are
/// collected, since a Minecraft world's paths easily pass the 100-byte
/// field; everything else a header can carry is ignored.
#[derive(Default)]
struct TarTracker {
    header: Vec<u8>,
    /// Data and padding still to pass over before the next header.
    skip: u64,
    /// While inside an `L` entry: the long name being collected, and how
    /// many of its bytes are still to come.
    long_name: Option<(Vec<u8>, u64)>,
    /// A long name read, waiting for the header it belongs to.
    pending_name: Option<String>,
    files_seen: u64,
    current: Option<String>,
}

impl TarTracker {
    fn feed(&mut self, mut data: &[u8]) {
        while !data.is_empty() {
            if self.skip > 0 {
                let n = (self.skip.min(data.len() as u64)) as usize;
                if let Some((name, remaining)) = self.long_name.as_mut() {
                    let take = (*remaining).min(n as u64) as usize;
                    name.extend_from_slice(&data[..take]);
                    *remaining -= take as u64;
                }
                self.skip -= n as u64;
                data = &data[n..];
                if self.skip == 0 {
                    if let Some((name, _)) = self.long_name.take() {
                        self.pending_name = Some(clean_tar_name(&name));
                    }
                }
                continue;
            }
            let take = (512 - self.header.len()).min(data.len());
            self.header.extend_from_slice(&data[..take]);
            data = &data[take..];
            if self.header.len() == 512 {
                let header = std::mem::take(&mut self.header);
                self.read_header(&header);
            }
        }
    }

    fn read_header(&mut self, header: &[u8]) {
        if header.iter().all(|&byte| byte == 0) {
            return; // an end-of-archive block
        }
        let size = tar_size(&header[124..136]);
        let padded = size.div_ceil(512) * 512;
        let kind = header[156];
        if kind == b'L' {
            self.long_name = Some((Vec::new(), size));
            self.skip = padded;
            return;
        }
        let name = self.pending_name.take().unwrap_or_else(|| {
            let short = clean_tar_name(&header[..100]);
            let is_ustar = &header[257..262] == b"ustar";
            let prefix = if is_ustar { clean_tar_name(&header[345..500]) } else { String::new() };
            if prefix.is_empty() { short } else { format!("{prefix}/{short}") }
        });
        // Regular files ('0', or NUL from old writers) are what gets counted
        // and named; directories, links and extended headers are not.
        if kind == b'0' || kind == 0 {
            self.files_seen += 1;
            self.current = Some(name);
        }
        self.skip = if matches!(kind, b'0' | 0 | b'7' | b'x' | b'g' | b'K') { padded } else { 0 };
    }
}

/// A NUL-terminated header field, without tar's leading `./`.
fn clean_tar_name(field: &[u8]) -> String {
    let end = field.iter().position(|&byte| byte == 0).unwrap_or(field.len());
    let name = String::from_utf8_lossy(&field[..end]).into_owned();
    name.strip_prefix("./").map(str::to_string).unwrap_or(name)
}

/// A header's size field: octal text, or GNU's base-256 for large files.
fn tar_size(field: &[u8]) -> u64 {
    if field[0] & 0x80 != 0 {
        return field[1..].iter().fold(0u64, |acc, &byte| (acc << 8) | u64::from(byte));
    }
    let text = String::from_utf8_lossy(field);
    u64::from_str_radix(text.trim_matches(|c: char| c == '\0' || c == ' '), 8).unwrap_or(0)
}

/// Mirrors every file and directory under `source`'s root onto `target`.
///
/// Two passes. The first only lists, so the second can report how far along
/// it is against a known total. Directories are then created in the order
/// they were found - a parent is always listed, and so found, before its
/// children - which is what `create_directory`'s single-level contract
/// needs. Files are whole-file `read_file`/`write_file` in memory: there is
/// no local disk on either end of this copy to stage through, and this
/// matches `ApplicationFileProvider::copy`'s own "a real read-then-write per
/// file" stance on the same trait.
async fn copy_directory(source: &dyn ApplicationFileProvider, target: &dyn ApplicationFileProvider, progress: ProgressSink<'_>) -> AppResult<u64> {
    progress(MigrationProgress::phase(MigrationPhase::Scanning));
    let mut directories = Vec::new();
    let mut files: Vec<(String, u64)> = Vec::new();
    let mut pending = vec![String::new()];
    while let Some(dir) = pending.pop() {
        for entry in source.list_directory(&dir).await? {
            let relative = if dir.is_empty() { entry.name.clone() } else { format!("{dir}/{}", entry.name) };
            if entry.is_dir {
                directories.push(relative.clone());
                pending.push(relative);
            } else {
                files.push((relative, entry.size));
            }
        }
    }

    let files_total = files.len() as u64;
    let bytes_total: u64 = files.iter().map(|(_, size)| size).sum();
    log::info!("migration: copying {files_total} files ({bytes_total} bytes) in {} directories", directories.len());
    let report = |files_done: u64, bytes_done: u64, current: Option<&str>| {
        progress(MigrationProgress {
            phase: MigrationPhase::Copying,
            files_done,
            files_total,
            bytes_done,
            bytes_total,
            current: current.map(str::to_string),
        })
    };
    report(0, 0, None);

    for directory in &directories {
        target.create_directory(directory).await?;
    }

    // Reported every so often rather than per file: a server can have tens of
    // thousands of small files, and an event for each is more than the UI can
    // usefully draw. A large file is always announced, so the name on screen
    // is the one the copy is actually waiting on.
    const REPORT_EVERY: std::time::Duration = std::time::Duration::from_millis(150);
    const ALWAYS_ANNOUNCE_BYTES: u64 = 1024 * 1024;
    let mut last_report = std::time::Instant::now();
    let mut copied = 0u64;
    let mut bytes_done = 0u64;
    for (relative, size) in &files {
        if *size >= ALWAYS_ANNOUNCE_BYTES || last_report.elapsed() >= REPORT_EVERY {
            report(copied, bytes_done, Some(relative));
            last_report = std::time::Instant::now();
        }
        let bytes = source.read_file(relative).await?;
        target.write_file(relative, &bytes).await?;
        copied += 1;
        bytes_done += size;
    }
    report(copied, bytes_done, None);
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::local::LocalApplicationFileProvider;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("vibessh-migration-test-{name}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// One tar header block: name, octal size, type - the fields the tracker reads.
    fn header(name: &str, size: u64, kind: u8) -> Vec<u8> {
        let mut block = vec![0u8; 512];
        block[..name.len()].copy_from_slice(name.as_bytes());
        let size = format!("{size:011o}\0");
        block[124..136].copy_from_slice(size.as_bytes());
        block[156] = kind;
        block[257..263].copy_from_slice(b"ustar\0");
        block
    }

    fn entry(name: &str, data: &[u8], kind: u8) -> Vec<u8> {
        let mut bytes = header(name, data.len() as u64, kind);
        bytes.extend_from_slice(data);
        bytes.resize(bytes.len().div_ceil(512) * 512, 0);
        bytes
    }

    #[test]
    fn the_tar_tracker_counts_files_and_names_the_current_one_across_any_chunking() {
        let long_name = format!("./world/region/{}.mca", "r".repeat(120));
        let mut archive = Vec::new();
        archive.extend(entry("./", b"", b'5'));
        archive.extend(entry("./server.properties", b"motd=hi\n", b'0'));
        archive.extend(entry("./world/", b"", b'5'));
        archive.extend(entry("././@LongLink", format!("{long_name}\0").as_bytes(), b'L'));
        archive.extend(entry("./world/region/rrrr", &[7u8; 1500], b'0'));
        archive.extend(vec![0u8; 1024]);

        // Chunk sizes that land mid-header and mid-data, as TCP will.
        for chunk in [1, 7, 511, 513, 4096] {
            let mut tracker = TarTracker::default();
            for piece in archive.chunks(chunk) {
                tracker.feed(piece);
            }
            assert_eq!(tracker.files_seen, 2, "chunk size {chunk}");
            assert_eq!(tracker.current.as_deref(), Some(long_name.trim_start_matches("./")), "chunk size {chunk}");
        }
    }

    #[test]
    fn a_tar_size_field_reads_both_octal_and_base_256() {
        assert_eq!(tar_size(b"00000001750\0"), 1000);
        let mut big = [0u8; 12];
        big[0] = 0x80;
        big[8..].copy_from_slice(&(10_000_000_000u64 as u32).to_be_bytes());
        big[7] = 2; // 2 << 32 + low word
        assert_eq!(tar_size(&big), (2u64 << 32) | u64::from(10_000_000_000u64 as u32));
    }

    #[test]
    fn the_node_side_count_is_three_numbers_and_nothing_else() {
        assert_eq!(parse_tree_count("2515 1765 912857339\n"), Some((2515, 1765, 912857339)));
        assert_eq!(parse_tree_count("2515 1765"), None);
        assert_eq!(parse_tree_count("find: permission denied 1 2 3"), None);
        // One entry of one block: header for it and for ".", data, end blocks, one record.
        assert_eq!(estimated_tar_size(1, 512), 10 * 1024);
    }

    #[tokio::test]
    async fn copy_directory_mirrors_nested_files_and_empty_directories() {
        let source_root = temp_dir("source");
        std::fs::create_dir_all(source_root.join("plugins/data")).unwrap();
        std::fs::write(source_root.join("server.properties"), b"motd=hi").unwrap();
        std::fs::write(source_root.join("plugins/MyPlugin.jar"), b"jarbytes").unwrap();
        std::fs::create_dir_all(source_root.join("empty-dir")).unwrap();

        let target_root = temp_dir("target");
        let source = LocalApplicationFileProvider::new(source_root.to_string_lossy().into_owned());
        let target = LocalApplicationFileProvider::new(target_root.to_string_lossy().into_owned());

        let reports = std::sync::Mutex::new(Vec::new());
        let copied = copy_directory(&source, &target, &|p: MigrationProgress| reports.lock().unwrap().push(p)).await.unwrap();
        assert_eq!(copied, 2, "two real files, the directories themselves don't count");

        assert_eq!(std::fs::read(target_root.join("server.properties")).unwrap(), b"motd=hi");
        assert_eq!(std::fs::read(target_root.join("plugins/MyPlugin.jar")).unwrap(), b"jarbytes");
        assert!(target_root.join("plugins/data").is_dir());
        assert!(target_root.join("empty-dir").is_dir());

        // Ends on a report that accounts for every file and byte, so the bar
        // reaches 100% rather than stopping one file short.
        let reports = reports.into_inner().unwrap();
        let last = reports.last().expect("at least one report");
        assert_eq!(last.phase, MigrationPhase::Copying);
        assert_eq!((last.files_done, last.files_total), (2, 2));
        assert_eq!((last.bytes_done, last.bytes_total), (15, 15));

        std::fs::remove_dir_all(&source_root).ok();
        std::fs::remove_dir_all(&target_root).ok();
    }
}
