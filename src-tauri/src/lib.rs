// `pub` (not just `mod`) so the integration test in `tests/agent_client.rs`
// can drive it directly - everything else here only needs in-crate tests
// (pairing_commands' own test lives inside that module, see its file for why).
pub mod agent_client;
mod commands;
mod errors;
mod models;
mod services;
// `pub` for the same reason as `agent_client` above - `tests/ssh_client.rs`
// drives `ssh::connect` directly against a local mock SSH server.
pub mod ssh;
mod state;
mod storage;
mod transport;

use state::{AppState, PairingSession, SshSessionManager, TerminalSessionManager};
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
        .manage(AppState::new("VibeSSH", env!("CARGO_PKG_VERSION")))
        .manage(PairingSession::new())
        .manage(SshSessionManager::new())
        .manage(TerminalSessionManager::new())
        .setup(|app| {
            // Needs the resolved app data dir, which only exists once the
            // app is running - can't be built alongside the other .manage()
            // calls above.
            let db_path = app.path().app_data_dir()?.join("servers.sqlite3");
            app.manage(ServerRepository::open(&db_path)?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_commands::get_app_info,
            commands::pairing_commands::generate_pairing_code,
            commands::pairing_commands::pairing_code_ttl_seconds,
            commands::pairing_commands::start_agent_pairing,
            commands::pairing_commands::cancel_agent_pairing,
            commands::server_commands::create_server,
            commands::server_commands::update_server,
            commands::server_commands::delete_server,
            commands::server_commands::get_server,
            commands::server_commands::list_servers,
            commands::ssh_commands::test_ssh_connection,
            commands::ssh_commands::execute_ssh_command,
            commands::terminal_commands::open_terminal,
            commands::terminal_commands::write_to_terminal,
            commands::terminal_commands::resize_terminal,
            commands::terminal_commands::close_terminal,
            commands::file_commands::list_remote_directory,
            commands::file_commands::read_remote_file,
            commands::file_commands::write_remote_file,
            commands::monitor_commands::get_server_metrics,
            commands::monitor_commands::list_server_processes,
            commands::actions_commands::list_server_services,
            commands::actions_commands::restart_server_service,
            commands::actions_commands::list_server_containers,
            commands::actions_commands::restart_server_container,
        ])
        .run(tauri::generate_context!())
        .expect("error while running VibeSSH");
}
