//! Removing an Application and everything VibeSSH created on its behalf.
//!
//! Its own module because it is the most consequential path in the service:
//! before Phase A it deleted a row and nothing else, leaving containers
//! running under `--restart unless-stopped` and still holding published
//! ports (S-007). The order of the steps here is the design - see
//! `delete_application`'s own doc comment.//!
//! Split out of a single 2685-line `application_service` (FIX_PLAN E.7).
//! Behaviour is unchanged; only the file boundaries moved.

use std::sync::Arc;

use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::runtime::local_process::LocalProcessManager;
use crate::runtime::RuntimeContext;
use crate::services::ssh_service::get_or_connect;
// The one shared implementation - this module used to carry its own
// byte-identical copy, one of six across the codebase.
use crate::ssh::SshSession;
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::credentials;
use crate::storage::firewall_rule_repository::FirewallRuleRepository;
use crate::storage::log_capture::LogCaptureStore;
use crate::storage::node_network_repository::NodeNetworkRepository;
use crate::storage::server_repository::ServerRepository;

use super::*;

/// The two teardown steps that destroy data VibeSSH cannot reconstruct, so
/// neither happens unless a caller explicitly asks.
///
/// `Default` is the conservative choice for both, which is what makes
/// `migration_service`'s use safe: retiring a migrated source Application
/// must remove its container and its Node-side identity, but must not drop
/// databases (migration does not move them, so dropping would destroy data
/// the operator still has) or delete files (they are the originals the copy
/// was made from).
#[derive(Debug, Clone, Copy, Default)]
pub struct ApplicationDeleteOptions {
    /// `DROP DATABASE` every database this Application owns. The rows
    /// cascade away regardless; without this the real databases are left
    /// behind on the host with nothing pointing at them.
    pub drop_databases: bool,
    /// `rm -rf` the Application's `working_directory` - a world save, a
    /// database volume, whatever the operator put there. The one step here
    /// that cannot be undone.
    pub remove_files: bool,
}

/// What a delete actually managed to clean up, and what it did not.
///
/// Deleting an Application touches a Docker container, a Linux account, a
/// firewall, a DNS record, one or more real databases and a directory of
/// files - on a machine that may go offline halfway through. Reporting a
/// bare `Ok(())` would mean the operator cannot tell "fully removed" from
/// "row gone, container still running and still holding the port", which is
/// exactly the state that made a later Application fail to start with an
/// unexplained "port is already allocated".
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationTeardownReport {
    pub container_removed: bool,
    pub databases_dropped: usize,
    pub firewall_synced: bool,
    pub dns_synced: bool,
    pub dedicated_account_removed: bool,
    pub working_directory_removed: bool,
    /// Human-readable description of every step that did not complete.
    /// Empty means the teardown was clean.
    pub warnings: Vec<String>,
}

/// Removes an Application and everything VibeSSH created on its behalf.
///
/// **The row used to be all that was deleted.** No container was destroyed,
/// no dedicated account removed, no firewall rule revoked, no database
/// dropped. The container kept running under `--restart unless-stopped`,
/// survived reboots, and kept its published port bound - and because a
/// replacement Application gets a fresh UUID, the port-collision check
/// (which only consults the database) reported the port free and then
/// `docker create` failed with a raw Docker error the operator could not
/// act on. Dedicated Linux accounts and application directories accumulated
/// on the Node with nothing left pointing at them.
///
/// Order is deliberate:
/// 1. destroy the runtime **before** the row goes away, because that needs
///    `runtime_config` to know what to destroy;
/// 2. drop databases while the credentials to do it still resolve;
/// 3. delete the row, so the firewall and DNS reconciles below compute a
///    desired state that no longer contains this Application;
/// 4. reconcile firewall and DNS, which is what actually revokes the rules
///    and removes the hostname;
/// 5. remove the Node-side identity and, only if asked, the files.
///
/// Every step is independent: one failing is recorded in `warnings` and the
/// rest still run. A half-cleaned Node is better than a Node where one
/// early failure left everything else behind too.
///
/// See [`ApplicationDeleteOptions`] for the two steps that are opt-in.
#[allow(clippy::too_many_arguments)]
pub async fn delete_application(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    db_repo: &crate::storage::database_repository::DatabaseRepository,
    network_repo: &NodeNetworkRepository,
    firewall_rule_repo: &FirewallRuleRepository,
    dns_repo: &crate::storage::dns_repository::DnsRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    log_capture: &LogCaptureStore,
    dns_suffix: &str,
    id: Uuid,
    options: ApplicationDeleteOptions,
) -> AppResult<ApplicationTeardownReport> {
    let mut report = ApplicationTeardownReport::default();
    let detail = get_application(repo, id)?;
    let server_id = detail.application.server_id;
    let working_directory = detail.application.working_directory.clone();
    let wants_dedicated_user = crate::files::wants_dedicated_user(&detail.application, &detail.runtime_config);
    // Read before the row goes away: `ON DELETE CASCADE` takes the
    // `application_links` rows with it, and step 7 needs to know which
    // per-connection Docker networks existed and which peers have to be
    // taken off them.
    let former_peers = detail.links.clone();

    // 1. The runtime itself. For Docker this is `docker rm -f`, which is
    //    the step whose absence left orphaned containers holding ports.
    match load_runtime(repo, server_repo, sessions, local_process_manager, id).await {
        Ok((detail, connection, runtime)) => {
            let ctx = RuntimeContext {
                application: &detail.application,
                runtime_config: &detail.runtime_config,
                environment: &detail.environment,
                ports: &detail.ports,
                links: &detail.links,
                connection,
            };
            match runtime.destroy(&ctx).await {
                Ok(()) => report.container_removed = true,
                Err(err) => report.warnings.push(format!("couldn't remove the container: {err}")),
            }
        }
        Err(err) => report.warnings.push(format!("couldn't reach the runtime to remove it: {err}")),
    }

    // 2. Databases, while the host's admin credentials still resolve.
    if options.drop_databases {
    match db_repo.list_databases(id) {
        Ok(databases) => {
            for database in databases {
                match crate::services::database_service::delete_application_database(db_repo, server_repo, sessions, database.id).await {
                    Ok(()) => report.databases_dropped += 1,
                    Err(err) => report.warnings.push(format!("couldn't drop the database '{}': {err}", database.database_name)),
                }
            }
        }
        Err(err) => report.warnings.push(format!("couldn't list this application's databases: {err}")),
    }
    }

    // 3. Secrets and captured logs - neither has anything left to belong to
    //    once the row is gone, and the OS keyring has no idea the row ever
    //    existed, so a secret would otherwise outlive it forever.
    for env in &detail.environment {
        if env.is_secret {
            if let Err(err) = credentials::delete_environment_secret(id, &env.key) {
                report.warnings.push(format!("couldn't remove the stored '{}' secret: {err}", env.key));
            }
        }
    }
    log_capture.delete(id).await;

    // 4. The row. `ON DELETE CASCADE` takes the ports, environment, runtime
    //    config, metadata and DNS record with it - which is what makes the
    //    two reconciles below compute a state without this Application.
    repo.delete(id)?;

    // 5. Firewall and DNS, now that the desired state no longer mentions it.
    if let Some(server_id) = server_id {
        match crate::services::firewall_service::reconcile_node(repo, server_repo, network_repo, firewall_rule_repo, sessions, server_id).await {
            Ok(_) => report.firewall_synced = true,
            Err(err) => report.warnings.push(format!("couldn't revoke this application's firewall rules: {err}")),
        }
        if network_repo.get(server_id)?.is_some() {
            match crate::services::dns_service::sync_dns(dns_suffix, network_repo, server_repo, repo, dns_repo, sessions).await {
                Ok(_) => report.dns_synced = true,
                Err(err) => report.warnings.push(format!("couldn't remove this application's DNS name: {err}")),
            }
        } else {
            report.dns_synced = true;
        }
    } else {
        report.firewall_synced = true;
        report.dns_synced = true;
    }

    // 6. Connections. The peers are still attached to the per-connection
    //    networks this Application shared with them, and their own rows no
    //    longer mention it, so reconciling each one is what actually takes
    //    them off. Doing this *after* the row is gone is what makes each
    //    peer's desired set come out without this Application in it.
    for peer in &former_peers {
        match load_runtime(repo, server_repo, sessions, local_process_manager, *peer).await {
            Ok((peer_detail, connection, runtime)) => {
                let ctx = RuntimeContext {
                    application: &peer_detail.application,
                    runtime_config: &peer_detail.runtime_config,
                    environment: &peer_detail.environment,
                    ports: &peer_detail.ports,
                    links: &peer_detail.links,
                    connection,
                };
                if let Err(err) = runtime.sync_connections(&ctx).await {
                    report.warnings.push(format!("couldn't disconnect '{}' from this application's network: {err}", peer_detail.application.name));
                }
            }
            Err(err) => report.warnings.push(format!("couldn't reach a connected application to disconnect it: {err}")),
        }
    }

    // 7. Node-side leftovers: the console fifo, the staging directory, the
    //    dedicated account, this Application's own Docker networks, and -
    //    only when explicitly asked - the files.
    if let Some(server_id) = server_id {
        match get_or_connect(server_repo, sessions, server_id).await {
            Ok(connection) => {
                cleanup_node_artifacts(&connection, id, wants_dedicated_user, &mut report).await;
                report.warnings.extend(crate::runtime::docker::remove_networks(connection.as_ref(), id, &former_peers).await);
                if options.remove_files {
                    match remove_working_directory(&connection, &working_directory).await {
                        Ok(()) => report.working_directory_removed = true,
                        Err(err) => report.warnings.push(format!("couldn't remove '{working_directory}': {err}")),
                    }
                }
            }
            Err(err) => report.warnings.push(format!("couldn't reach the Node to finish cleaning up: {err}")),
        }
    } else if options.remove_files {
        match tokio::fs::remove_dir_all(&working_directory).await {
            Ok(()) => report.working_directory_removed = true,
            Err(err) => report.warnings.push(format!("couldn't remove '{working_directory}': {err}")),
        }
    }

    Ok(report)
}

/// The per-Application files VibeSSH itself put on the Node outside the
/// Application's own directory: its console fifo, its file-staging
/// directory, and its schedules' cron file and run records. Plus the
/// dedicated Linux account, when it had one.
///
/// `userdel` without `--remove` on purpose: the account has no home
/// directory to remove (see `dedicated_user::ensure_provisioned`), and
/// `--remove` would additionally delete files it owns elsewhere, which is
/// exactly the Application data `remove_files` exists to gate.
async fn cleanup_node_artifacts(
    connection: &SshSession,
    application_id: Uuid,
    wants_dedicated_user: bool,
    report: &mut ApplicationTeardownReport,
) {
    let fifo = crate::ssh::command::quote(&format!("{}/{application_id}.stdin", crate::node_paths::CONSOLE_DIR));
    let staging = crate::ssh::command::quote(&format!("{}/{application_id}", crate::files::sudo_user::STAGING_ROOT));
    if let Err(err) = connection.execute_command(&format!("rm -f {fifo}; sudo rm -rf {staging}")).await {
        report.warnings.push(format!("couldn't remove this application's runtime files: {err}"));
    }
    // Its schedules: left behind, the cron file would go on starting and
    // stopping a container that no longer exists, every day, for good.
    if let Err(err) = crate::services::schedule_service::remove_from_node(connection, application_id).await {
        report.warnings.push(format!("couldn't remove this application's schedules from the Node: {err}"));
    }

    if !wants_dedicated_user {
        report.dedicated_account_removed = true;
        return;
    }
    let username = crate::dedicated_user::username(application_id);
    let command = format!("id -u {u} >/dev/null 2>&1 && sudo userdel {u} || true", u = crate::ssh::command::quote(&username));
    match connection.execute_command(&command).await {
        Ok(output) if output.exit_code == 0 => report.dedicated_account_removed = true,
        Ok(output) => report.warnings.push(format!("couldn't remove the '{username}' account: {}", output.stderr.trim())),
        Err(err) => report.warnings.push(format!("couldn't remove the '{username}' account: {err}")),
    }
}

/// Re-validated against the same rules that gated it at creation. This runs
/// `sudo rm -rf`, so a stale or hand-edited row naming `/` or `/etc` must
/// not be able to reach it just because it got past an older build.
async fn remove_working_directory(connection: &SshSession, working_directory: &str) -> AppResult<()> {
    crate::ssh::command::validate_application_directory(working_directory)?;
    let output = connection
        .execute_command(&format!("sudo rm -rf {}", crate::ssh::command::quote(working_directory)))
        .await?;
    if output.exit_code != 0 {
        let detail = output.stderr.trim();
        return Err(AppError::Connection(if detail.is_empty() { "rm failed".to_string() } else { detail.to_string() }));
    }
    Ok(())
}
