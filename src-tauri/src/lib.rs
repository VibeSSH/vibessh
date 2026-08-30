// `pub` (not just `mod`) so the integration test in `tests/agent_client.rs`
// can drive it directly - everything else here only needs in-crate tests
// (pairing_commands' own test lives inside that module, see its file for why).
pub mod agent_client;
mod blueprints;
pub mod cloud_client;
mod commands;
mod errors;
mod models;
mod runtime;
mod services;
// `pub` for the same reason as `agent_client` above - `tests/ssh_client.rs`
// drives `ssh::connect` directly against a local mock SSH server.
pub mod ssh;
mod state;
mod storage;
mod transport;

use blueprints::BlueprintRegistry;
use runtime::local_process::LocalProcessManager;
use state::{AppState, CloudState, PairingSession, SshSessionManager, TerminalSessionManager};
use std::sync::Arc;
use storage::application_repository::ApplicationRepository;
use storage::server_repository::ServerRepository;
use tauri::Manager;
use tauri_plugin_log::{Target, TargetKind};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([
                    Target::new(TargetKind::Stdout),
                    Target::new(TargetKind::LogDir { file_name: None }),
                ])
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new("VibeSSH", env!("CARGO_PKG_VERSION")))
        .manage(PairingSession::new())
        .manage(SshSessionManager::new())
        .manage(TerminalSessionManager::new())
        // Arc-wrapped (unlike the two managers above) because
        // `LocalProcessRuntime` needs an owned, cheaply-cloneable handle to
        // construct itself with, not just a borrow scoped to one command -
        // see runtime::local_process's own doc comment.
        .manage(Arc::new(LocalProcessManager::new()))
        // Read-only after construction (no interior mutability needed) -
        // see blueprints::mod's own doc comment for why this is the whole
        // persistence story for built-in blueprints in this phase.
        .manage(BlueprintRegistry::with_builtins())
        .setup(|app| {
            // Needs the resolved app data dir, which only exists once the
            // app is running - can't be built alongside the other .manage()
            // calls above.
            let db_path = app.path().app_data_dir()?.join("servers.sqlite3");
            app.manage(ServerRepository::open(&db_path)?);
            // Same physical file as ServerRepository above (Applications'
            // server_id is a real foreign key into servers, which only
            // means something within one SQLite file) - see
            // ApplicationRepository::open's own doc comment for why this
            // is a second independent Connection rather than a shared one.
            app.manage(ApplicationRepository::open(&db_path)?);
            // Same physical file again - Application Databases (Phase 11)
            // foreign keys into both `applications` and `servers`.
            app.manage(storage::database_repository::DatabaseRepository::open(&db_path)?);

            let config_dir = app.path().app_config_dir()?;
            let backend_url = storage::cloud_config::load_backend_url(&config_dir)?;
            app.manage(CloudState::new(backend_url));

            // Silently turns a keyring-stored refresh token from a previous
            // run back into a live session, if there is one - see
            // services::cloud_try_restore_session's own doc comment for why
            // this is fire-and-forget rather than something setup() waits on
            // or surfaces an error for. Spawned *after* app.manage() above,
            // not before - the spawned task looks the state up by type, so
            // it must already be registered before this can run.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let state = handle.state::<CloudState>();
                services::cloud_try_restore_session(&state).await;
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_commands::get_app_info,
            commands::pairing_commands::generate_pairing_code,
            commands::pairing_commands::pairing_code_ttl_seconds,
            commands::pairing_commands::start_agent_pairing,
            commands::pairing_commands::cancel_agent_pairing,
            commands::application_commands::list_applications,
            commands::application_commands::list_paper_versions,
            commands::application_commands::list_velocity_versions,
            commands::application_commands::list_application_ports,
            commands::application_commands::add_application_port,
            commands::application_commands::update_application_port,
            commands::application_commands::remove_application_port,
            commands::application_commands::get_application,
            commands::application_commands::list_blueprints,
            commands::application_commands::create_application,
            commands::application_commands::delete_application,
            commands::application_commands::start_application,
            commands::application_commands::stop_application,
            commands::application_commands::restart_application,
            commands::application_commands::kill_application,
            commands::application_commands::refresh_application_status,
            commands::application_commands::get_application_resource_usage,
            commands::application_commands::get_application_logs,
            commands::application_commands::get_application_health,
            commands::application_commands::set_application_health_check,
            commands::application_commands::set_application_resource_limits,
            commands::application_commands::detect_java_installations,
            commands::database_commands::list_database_hosts,
            commands::database_commands::create_database_host,
            commands::database_commands::delete_database_host,
            commands::database_commands::set_database_host_phpmyadmin,
            commands::database_commands::list_application_databases,
            commands::database_commands::create_application_database,
            commands::database_commands::delete_application_database,
            commands::database_commands::reveal_application_database_password,
            commands::database_commands::reset_application_database_password,
            commands::server_commands::create_server,
            commands::server_commands::update_server,
            commands::server_commands::delete_server,
            commands::server_commands::get_server,
            commands::server_commands::list_servers,
            commands::server_commands::upsert_agent_server,
            commands::ssh_commands::test_ssh_connection,
            commands::ssh_commands::execute_ssh_command,
            commands::ssh_commands::ping_server,
            commands::terminal_commands::open_terminal,
            commands::terminal_commands::write_to_terminal,
            commands::terminal_commands::resize_terminal,
            commands::terminal_commands::close_terminal,
            commands::file_commands::list_remote_directory,
            commands::file_commands::create_remote_directory,
            commands::file_commands::read_remote_file,
            commands::file_commands::write_remote_file,
            commands::file_commands::download_remote_file,
            commands::file_commands::upload_remote_file,
            commands::monitor_commands::get_server_metrics,
            commands::monitor_commands::list_server_processes,
            commands::actions_commands::list_server_services,
            commands::actions_commands::restart_server_service,
            commands::actions_commands::start_server_service,
            commands::actions_commands::stop_server_service,
            commands::actions_commands::enable_server_service,
            commands::actions_commands::disable_server_service,
            commands::actions_commands::list_server_containers,
            commands::actions_commands::restart_server_container,
            commands::actions_commands::start_server_container,
            commands::actions_commands::stop_server_container,
            commands::actions_commands::remove_server_container,
            commands::actions_commands::get_server_container_logs,
            commands::cloud_commands::cloud_register,
            commands::cloud_commands::cloud_login,
            commands::cloud_commands::cloud_logout,
            commands::cloud_commands::cloud_session_info,
            commands::cloud_commands::cloud_get_backend_url,
            commands::cloud_commands::cloud_set_backend_url,
            commands::cloud_commands::cloud_list_teams,
            commands::cloud_commands::cloud_create_team,
            commands::cloud_commands::cloud_list_members,
            commands::cloud_commands::cloud_get_team,
            commands::cloud_commands::cloud_list_permissions,
            commands::cloud_commands::cloud_list_roles,
            commands::cloud_commands::cloud_create_role,
            commands::cloud_commands::cloud_update_role,
            commands::cloud_commands::cloud_delete_role,
            commands::cloud_commands::cloud_list_member_roles,
            commands::cloud_commands::cloud_assign_role,
            commands::cloud_commands::cloud_unassign_role,
            commands::cloud_commands::cloud_list_servers,
            commands::cloud_commands::cloud_create_server,
            commands::cloud_commands::cloud_delete_server,
            commands::cloud_commands::cloud_my_permissions,
            commands::cloud_commands::cloud_remove_member,
            commands::cloud_commands::cloud_delete_team,
            commands::cloud_commands::cloud_list_invitations,
            commands::cloud_commands::cloud_create_invitation,
            commands::cloud_commands::cloud_revoke_invitation,
            commands::cloud_commands::cloud_accept_invitation,
            commands::cloud_commands::cloud_decline_invitation,
            commands::cloud_commands::cloud_list_audit_events,
        ])
        .run(tauri::generate_context!())
        .expect("error while running VibeSSH");
}
