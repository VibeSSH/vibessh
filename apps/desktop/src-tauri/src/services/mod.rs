mod ai_service;
mod app_info_service;
mod application_backup_service;
mod application_files_service;
mod application_service;
mod cloud_service;
mod database_service;
mod dns_service;
mod firewall_service;
pub mod java_runtime_service;
pub mod server_discovery_service;
mod java_service;
mod migration_service;
mod network_service;
mod node_state_service;
mod papermc_service;
mod pterodactyl_import_service;
mod pterodactyl_run_service;
mod ping_service;
mod purpur_service;
mod server_service;
mod ssh_service;
pub mod team_access_service;
pub mod team_application_service;

use crate::errors::AppResult;

pub use app_info_service::{get_app_info, local_docker_available};
pub use application_backup_service::{
    create_backup, delete_backup, get_backup_destination, get_backup_schedule, list_backups, restore_backup, run_due_backups,
    set_backup_destination, set_backup_schedule, test_backup_destination,
};
pub use application_files_service::{
    clear_file_history, compress as compress_application_files, copy as copy_application_file, create_directory as create_application_directory, delete as delete_application_file,
    download_file as download_application_file, extract_archive as extract_application_archive, get_metadata as get_application_file_metadata,
    list_directory as list_application_files, list_file_history, read_file_for_editor as read_application_file, rename as rename_application_file,
    restore_file_history, save_file as save_application_file, set_permissions as set_application_file_permissions,
    read_file_window as read_application_file_window, FileWindow,
    upload_directory as upload_application_directory, upload_file as upload_application_file,
    write_file as write_application_file, FileHistoryVersion,
};
pub use ai_service::{
    ai_config_view, ai_quota, build_ai_context, resolve_provider as resolve_ai_provider, run_turn as run_ai_turn,
    set_ai_config, test_ai_connection,
};
pub use application_service::{
    add_application_port, application_console_write, change_application_blueprint, application_health_check, application_logs, application_resource_usage, clear_application_logs, run_application_command,
    rename_application,
    follow_application_logs, follow_application_stats, StatsSample,
    refresh_vibe_network_bind_addresses,
    create_application, delete_application, get_application, kill_application, list_application_ports, list_applications,
    ApplicationDeleteOptions, ApplicationTeardownReport,
    connect_applications, disconnect_applications, list_application_links,
    list_blueprints, list_registry_credentials, recreate_application, refresh_application_status, remove_application_port,
    remove_registry_credential, restart_application,
    pull_application_image, set_application_environment, set_application_health_check, set_application_image, set_application_resource_limits,
    set_registry_credential, start_application,
    stop_application, update_application_config, update_application_port,
};
pub use dns_service::{
    create_alias as create_dns_alias, delete_alias as delete_dns_alias, get_dns_suffix, list_records as list_dns_records, resolve_dns_view,
    set_dns_suffix, sync_dns, update_alias as update_dns_alias, verify_alias as verify_dns_alias, DnsAliasWithSync, DnsSyncResult,
};
pub use firewall_service::{
    add_custom_firewall_rule, desired_rules as preview_node_firewall_rules, enable_node_firewall, node_firewall_overview,
    node_listening_sockets, reconcile_node as sync_node_firewall, remove_custom_firewall_rule, sync_application_node_firewall, FirewallSyncResult,
    ListeningSocket, NodeFirewallOverview,
};
pub use network_service::{
    join_node, leave_node, list_members as list_network_members, list_node_endpoints, mesh_status, reconcile_mesh, sync_vibe_network,
    MeshReconcileResult, NodeEndpoint, NodeMeshStatus, PeerHandshake, VibeNetworkSyncResult,
};
pub use node_state_service::{reconcile_node, sync_status as node_sync_status};
pub use pterodactyl_import_service::{
    build_plan as build_pterodactyl_plan, MigrationPlan as PterodactylMigrationPlan, NodeOverride as PterodactylNodeOverride,
    PlannedServer as PterodactylPlannedServer,
};
pub use pterodactyl_run_service::{import_server as import_pterodactyl_server, ImportOutcome as PterodactylImportOutcome, ImportStep as PterodactylImportStep};
pub use database_service::{
    create_application_database, create_database_host, delete_application_database, delete_database_host, install_database_server, repair_database_reachability,
    list_application_databases, list_database_hosts, phpmyadmin_url, reset_application_database_password,
    reveal_application_database_password, set_database_host_phpmyadmin, update_database_host,
};
pub use java_service::{detect_java_installations, JavaInstallation};
pub use migration_service::{migrate_application, MigrationProgress, MigrationResult};
pub use papermc_service::PapermcBuild;
pub use purpur_service::PurpurBuild;

/// Thin, project-fixed wrappers over `papermc_service`'s own
/// project-parameterized functions - `blueprints::{PaperBlueprint,
/// VelocityBlueprint, WaterfallBlueprint}` and their matching Tauri commands
/// each only ever need one specific project, so callers don't have to know
/// or repeat the literal `"paper"`/`"velocity"`/`"waterfall"` id themselves.
pub async fn list_paper_versions() -> AppResult<Vec<String>> {
    papermc_service::list_versions("paper").await
}

pub async fn latest_paper_build(version: &str) -> AppResult<PapermcBuild> {
    papermc_service::latest_build("paper", version).await
}

pub async fn list_velocity_versions() -> AppResult<Vec<String>> {
    papermc_service::list_versions("velocity").await
}

pub async fn latest_velocity_build(version: &str) -> AppResult<PapermcBuild> {
    papermc_service::latest_build("velocity", version).await
}

pub async fn list_waterfall_versions() -> AppResult<Vec<String>> {
    papermc_service::list_versions("waterfall").await
}

pub async fn latest_waterfall_build(version: &str) -> AppResult<PapermcBuild> {
    papermc_service::latest_build("waterfall", version).await
}

pub async fn list_purpur_versions() -> AppResult<Vec<String>> {
    purpur_service::list_versions().await
}

pub async fn latest_purpur_build(version: &str) -> AppResult<PurpurBuild> {
    purpur_service::latest_build(version).await
}

pub use cloud_service::{
    list_device_keys as cloud_list_device_keys, list_pending_revocations as cloud_list_pending_revocations,
    revoke_device_key as cloud_revoke_device_key,
    assign_role as cloud_assign_role, change_password as cloud_change_password,
    create_role as cloud_create_role, create_server as cloud_create_server, create_team as cloud_create_team,
    delete_role as cloud_delete_role, delete_server as cloud_delete_server,
    delete_team as cloud_delete_team, get_team as cloud_get_team, list_audit_events as cloud_list_audit_events,
    list_member_roles as cloud_list_member_roles, list_members as cloud_list_members,
    list_permissions as cloud_list_permissions, list_roles as cloud_list_roles, list_servers as cloud_list_servers,
    list_teams as cloud_list_teams, login as cloud_login, logout as cloud_logout, my_permissions as cloud_my_permissions,
    provision_member as cloud_provision_member, register as cloud_register, remove_member as cloud_remove_member,
    session_info as cloud_session_info, try_restore_session as cloud_try_restore_session,
    unassign_role as cloud_unassign_role, update_role as cloud_update_role,
};
pub use ping_service::ping_server;
pub use server_service::{
    create_server, delete_server, get_server, set_server_icon, install_docker, install_ufw, install_wireguard, list_servers, probe_node_capabilities,
    replace_ssh_password, update_server,
    upgrade_server_to_agent, upsert_agent_server,
};
pub use ssh_service::{
    compress_paths as compress_remote_paths, container_logs as server_container_logs, create_directory as create_remote_directory,
    delete_path as delete_remote_path, disable_service as disable_server_service,
    download_file as download_remote_file, enable_service as enable_server_service,
    read_file_window as read_remote_file_window,
    execute_command as execute_ssh_command, extract_archive as extract_remote_archive, get_metrics as get_server_metrics,
    list_containers as list_server_containers,
    list_directory as list_remote_directory, list_processes as list_server_processes,
    list_services as list_server_services, open_terminal as open_ssh_terminal, read_file as read_remote_file,
    remove_container as remove_server_container, rename_path as rename_remote_path, restart_container as restart_server_container,
    restart_service as restart_server_service, set_permissions as set_remote_permissions, start_container as start_server_container,
    start_port_forward, start_service as start_server_service, stop_container as stop_server_container, stop_service as stop_server_service,
    test_connection as test_ssh_connection, upload_directory as upload_remote_directory,
    upload_file as upload_remote_file, write_file as write_remote_file,
};
