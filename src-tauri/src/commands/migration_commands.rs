use std::sync::Arc;

use tauri::State;
use uuid::Uuid;

use crate::errors::AppResult;
use crate::runtime::local_process::LocalProcessManager;
use crate::services::{self, MigrationResult};
use crate::state::{DnsSuffixState, MigrationLockManager, SshSessionManager};
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::database_repository::DatabaseRepository;
use crate::storage::dns_repository::DnsRepository;
use crate::storage::firewall_rule_repository::FirewallRuleRepository;
use crate::storage::log_capture::LogCaptureStore;
use crate::storage::node_network_repository::NodeNetworkRepository;
use crate::storage::registry_credential_repository::RegistryCredentialRepository;
use crate::storage::server_repository::ServerRepository;

/// "Migrate to another Node" (deferred piece of the original Vibe Network
/// plan, §7) - moves a Docker Application to a different Node: provisions
/// an identical instance there, copies its working directory over, cuts its
/// DNS alias (if any) to the new instance, then retires the old one. See
/// `services::migration_service`'s own doc comment for the full step order.
#[tauri::command]
pub async fn migrate_application(
    app_repo: State<'_, ApplicationRepository>,
    server_repo: State<'_, ServerRepository>,
    network_repo: State<'_, NodeNetworkRepository>,
    dns_repo: State<'_, DnsRepository>,
    db_repo: State<'_, DatabaseRepository>,
    dns_suffix: State<'_, DnsSuffixState>,
    firewall_rule_repo: State<'_, FirewallRuleRepository>,
    registry_repo: State<'_, RegistryCredentialRepository>,
    log_capture: State<'_, LogCaptureStore>,
    sessions: State<'_, SshSessionManager>,
    locks: State<'_, MigrationLockManager>,
    local_process_manager: State<'_, Arc<LocalProcessManager>>,
    id: Uuid,
    target_server_id: Uuid,
) -> AppResult<MigrationResult> {
    services::migrate_application(
        &app_repo,
        &server_repo,
        &network_repo,
        &dns_repo,
        &db_repo,
        &dns_suffix.get(),
        &firewall_rule_repo,
        &registry_repo,
        &log_capture,
        &sessions,
        &locks,
        &local_process_manager,
        id,
        target_server_id,
    )
    .await
}
