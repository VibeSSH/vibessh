// `pub mod` (not a re-export) so `tauri::generate_handler!` can find the
// hidden `__cmd__*` items the `#[tauri::command]` macro emits alongside
// each function — those only resolve through the function's real path.
pub mod ai_commands;
pub mod actions_commands;
pub mod agent_session_commands;
pub mod app_commands;
pub mod application_backup_commands;
pub mod application_commands;
pub mod application_file_commands;
pub mod application_template_commands;
pub mod cloud_commands;
pub mod database_commands;
pub mod file_commands;
pub mod migration_commands;
pub mod monitor_commands;
pub mod network_commands;
pub mod pairing_commands;
pub mod pterodactyl_commands;
pub mod pterodactyl_import_command;
pub mod port_forward_commands;
pub mod server_commands;
pub mod ssh_commands;
pub mod terminal_commands;
