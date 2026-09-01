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
use crate::services::ssh_service::get_or_connect;
use crate::state::{MigrationLockManager, SshSessionManager};
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::database_repository::DatabaseRepository;
use crate::storage::dns_repository::DnsRepository;
use crate::storage::firewall_rule_repository::FirewallRuleRepository;
use crate::storage::log_capture::LogCaptureStore;
use crate::storage::node_network_repository::NodeNetworkRepository;
use crate::storage::registry_credential_repository::RegistryCredentialRepository;
use crate::storage::server_repository::ServerRepository;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationResult {
    pub application: ApplicationDetail,
    pub files_copied: u64,
    pub dns_repointed: bool,
}

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
    log_capture: &LogCaptureStore,
    sessions: &SshSessionManager,
    locks: &MigrationLockManager,
    local_process_manager: &Arc<LocalProcessManager>,
    source_application_id: Uuid,
    target_server_id: Uuid,
) -> AppResult<MigrationResult> {
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
        log_capture,
        sessions,
        local_process_manager,
        source_application_id,
        target_server_id,
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
    log_capture: &LogCaptureStore,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    source_application_id: Uuid,
    target_server_id: Uuid,
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

    // Stop the source first - copying a directory a running Application is
    // still writing to (a Minecraft server saving its world, a database
    // flushing a page) would copy it mid-write.
    if matches!(source.application.status, ApplicationStatus::Running | ApplicationStatus::Starting) {
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
    let files_copied = match provision_target(app_repo, server_repo, network_repo, firewall_rule_repo, sessions, &source, &created.application, target_server_id).await {
        Ok(files_copied) => files_copied,
        Err(err) => {
            let _ = app_repo.delete(target_application_id);
            return Err(err);
        }
    };
    // Only written once the target row is otherwise fully provisioned - on
    // any earlier failure above, the target row (and so its keyring
    // namespace) is rolled back, so there'd be nothing to clean up; on a
    // failure here, the same rollback still applies rather than leaving a
    // target Application missing its secrets.
    if let Err(err) = application_service::store_secret_environment_values(target_application_id, &create_input.environment) {
        let _ = app_repo.delete(target_application_id);
        return Err(err);
    }

    // Step 5: cut the DNS alias over, if this service has one - the
    // hostname never changes, only which Application it resolves through.
    let dns_repointed = dns_repo.repoint_application(source_application_id, target_application_id)?.is_some();
    if dns_repointed {
        let _ = dns_service::sync_dns(dns_suffix, network_repo, server_repo, app_repo, dns_repo, sessions).await;
    }

    // Step 6: bring the new instance up, then retire the old one. Both are
    // best-effort past this point - the migration itself (a new, fully
    // configured Application with the old one's data) has already
    // succeeded; a start failure or a firewall sync hiccup is now the same
    // kind of already-surfaced, retryable problem as it would be for any
    // other Application, not a reason to unwind everything above.
    let _ = application_service::start_application(app_repo, server_repo, sessions, registry_repo, local_process_manager, target_application_id).await;
    // Carry captured log history over to the new id before the source row
    // (and, if this were skipped, its own orphaned capture file) is retired
    // - see `LogCaptureStore::rename`'s own doc comment.
    log_capture.rename(source_application_id, target_application_id).await;
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
    for warning in &teardown.warnings {
        log::warn!("retiring the migrated source application {source_application_id}: {warning}");
    }
    if let Some(source_server_id) = source.application.server_id {
        let _ = firewall_service::reconcile_node(app_repo, server_repo, network_repo, firewall_rule_repo, sessions, source_server_id).await;
    }
    let _ = firewall_service::reconcile_node(app_repo, server_repo, network_repo, firewall_rule_repo, sessions, target_server_id).await;

    let final_detail = application_service::get_application(app_repo, target_application_id)?;
    Ok(MigrationResult { application: final_detail, files_copied, dns_repointed })
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

    // Finally the working directory's contents, provider-to-provider.
    let source_connection = match source.application.server_id {
        None => None,
        Some(server_id) => Some(get_or_connect(server_repo, sessions, server_id).await?),
    };
    let source_provider = files::provider_for(&source.application, &source.runtime_config, source_connection)?;
    let target_connection = Some(get_or_connect(server_repo, sessions, target_server_id).await?);
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
    copy_directory(source_provider.as_ref(), target_provider.as_ref()).await
}

/// Walks `source` breadth-first-by-level from its root, mirroring every
/// file and directory onto `target`. Whole-file `read_file`/`write_file`
/// (in memory) rather than `download_file`/`upload_file` - there's no local
/// disk on either end of this copy to stage through, and this matches
/// `ApplicationFileProvider::copy`'s own "always a real read-then-write per
/// file, correctness over speed" stance on the same trait. A directory is
/// always created during its *parent's* turn, before ever being queued for
/// its own turn - so by the time this descends into it, its parent is
/// already guaranteed to exist, which is what `create_directory`'s
/// single-level contract requires.
async fn copy_directory(source: &dyn ApplicationFileProvider, target: &dyn ApplicationFileProvider) -> AppResult<u64> {
    let mut copied = 0u64;
    let mut pending = vec![String::new()];
    while let Some(dir) = pending.pop() {
        for entry in source.list_directory(&dir).await? {
            let relative = if dir.is_empty() { entry.name.clone() } else { format!("{dir}/{}", entry.name) };
            if entry.is_dir {
                target.create_directory(&relative).await?;
                pending.push(relative);
            } else {
                let bytes = source.read_file(&relative).await?;
                target.write_file(&relative, &bytes).await?;
                copied += 1;
            }
        }
    }
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

        let copied = copy_directory(&source, &target).await.unwrap();
        assert_eq!(copied, 2, "two real files, the directories themselves don't count");

        assert_eq!(std::fs::read(target_root.join("server.properties")).unwrap(), b"motd=hi");
        assert_eq!(std::fs::read(target_root.join("plugins/MyPlugin.jar")).unwrap(), b"jarbytes");
        assert!(target_root.join("plugins/data").is_dir());
        assert!(target_root.join("empty-dir").is_dir());

        std::fs::remove_dir_all(&source_root).ok();
        std::fs::remove_dir_all(&target_root).ok();
    }
}
