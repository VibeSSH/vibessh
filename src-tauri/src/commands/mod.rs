// `pub mod` (not a re-export) so `tauri::generate_handler!` can find the
// hidden `__cmd__*` items the `#[tauri::command]` macro emits alongside
// each function — those only resolve through the function's real path.
pub mod actions_commands;
pub mod app_commands;
pub mod file_commands;
pub mod monitor_commands;
pub mod pairing_commands;
pub mod server_commands;
pub mod ssh_commands;
pub mod terminal_commands;
