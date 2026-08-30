// `pub` (not just `mod`) so the integration test in `tests/agent_client.rs`
// can drive it directly - everything else here only needs in-crate tests
// (pairing_commands' own test lives inside that module, see its file for why).
pub mod agent_client;
mod commands;
mod errors;
mod models;
mod services;
mod ssh;
mod state;
mod storage;
mod transport;

use state::{AppState, PairingSession};
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
        .invoke_handler(tauri::generate_handler![
            commands::app_commands::get_app_info,
            commands::pairing_commands::generate_pairing_code,
            commands::pairing_commands::pairing_code_ttl_seconds,
            commands::pairing_commands::start_agent_pairing,
            commands::pairing_commands::cancel_agent_pairing,
        ])
        .run(tauri::generate_context!())
        .expect("error while running VibeSSH");
}
