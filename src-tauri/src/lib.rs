mod commands;
mod errors;
mod models;
mod services;
mod ssh;
mod state;
mod storage;
mod transport;

use state::AppState;
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
        .invoke_handler(tauri::generate_handler![commands::app_commands::get_app_info])
        .run(tauri::generate_context!())
        .expect("error while running VibeSSH");
}
