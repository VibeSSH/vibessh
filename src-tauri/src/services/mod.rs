mod app_info_service;
mod application_service;
mod cloud_service;
mod java_service;
mod papermc_service;
mod ping_service;
mod server_service;
mod ssh_service;

use crate::errors::AppResult;

pub use app_info_service::get_app_info;
pub use application_service::{
    add_application_port, application_logs, application_resource_usage, create_application, delete_application, get_application,
    kill_application, list_application_ports, list_applications, list_blueprints, refresh_application_status,
    remove_application_port, restart_application, start_application, stop_application, update_application_port,
};
pub use java_service::{detect_java_installations, JavaInstallation};
pub use papermc_service::PapermcBuild;

/// Thin, project-fixed wrappers over `papermc_service`'s own
/// project-parameterized functions - `blueprints::{PaperBlueprint,
/// VelocityBlueprint}` and their matching Tauri commands each only ever
/// need one specific project, so callers don't have to know or repeat the
/// literal `"paper"`/`"velocity"` id themselves.
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

pub use cloud_service::{
    accept_invitation as cloud_accept_invitation, assign_role as cloud_assign_role, create_invitation as cloud_create_invitation,
    create_role as cloud_create_role, create_server as cloud_create_server, create_team as cloud_create_team,
    decline_invitation as cloud_decline_invitation, delete_role as cloud_delete_role, delete_server as cloud_delete_server,
    delete_team as cloud_delete_team, get_team as cloud_get_team, list_audit_events as cloud_list_audit_events,
    list_invitations as cloud_list_invitations, list_member_roles as cloud_list_member_roles, list_members as cloud_list_members,
    list_permissions as cloud_list_permissions, list_roles as cloud_list_roles, list_servers as cloud_list_servers,
    list_teams as cloud_list_teams, login as cloud_login, logout as cloud_logout, my_permissions as cloud_my_permissions,
    register as cloud_register, remove_member as cloud_remove_member, revoke_invitation as cloud_revoke_invitation,
    session_info as cloud_session_info, try_restore_session as cloud_try_restore_session,
    unassign_role as cloud_unassign_role, update_role as cloud_update_role,
};
pub use ping_service::ping_server;
pub use server_service::{create_server, delete_server, get_server, list_servers, update_server, upsert_agent_server};
pub use ssh_service::{
    container_logs as server_container_logs, create_directory as create_remote_directory,
    disable_service as disable_server_service,
    download_file as download_remote_file, enable_service as enable_server_service,
    execute_command as execute_ssh_command, get_metrics as get_server_metrics, list_containers as list_server_containers,
    list_directory as list_remote_directory, list_processes as list_server_processes,
    list_services as list_server_services, open_terminal as open_ssh_terminal, read_file as read_remote_file,
    remove_container as remove_server_container, restart_container as restart_server_container,
    restart_service as restart_server_service, start_container as start_server_container,
    start_service as start_server_service, stop_container as stop_server_container, stop_service as stop_server_service,
    test_connection as test_ssh_connection, upload_file as upload_remote_file, write_file as write_remote_file,
};
