mod agent_sessions;
mod ai_turns;
mod backup_destination_state;
pub mod cloud_session;
mod dns_suffix_state;
mod file_transfers;
mod migration_locks;
mod pairing_session;
mod port_forward_sessions;
mod ssh_sessions;
mod terminal_sessions;

pub use agent_sessions::AgentSessionManager;
pub use ai_turns::AiTurnManager;
pub use backup_destination_state::BackupDestinationState;
pub use cloud_session::CloudState;
pub use dns_suffix_state::DnsSuffixState;
pub use file_transfers::FileTransferManager;
pub use migration_locks::MigrationLockManager;
pub use pairing_session::PairingSession;
pub use port_forward_sessions::PortForwardManager;
pub use ssh_sessions::SshSessionManager;
pub use terminal_sessions::TerminalSessionManager;

/// Shared application state, injected into every Tauri command via `State<AppState>`.
///
/// Etap 1 only needs static metadata. `ServerRepository` (Etap 2) and
/// `SshSessionManager` (Etap 3) are managed separately rather than as fields
/// here, same reasoning as `PairingSession` below: they have nothing to do
/// with static app metadata and would just make this struct's locking story
/// more confusing for no benefit.
pub struct AppState {
    pub app_name: String,
    pub app_version: String,
}

impl AppState {
    pub fn new(app_name: impl Into<String>, app_version: impl Into<String>) -> Self {
        Self {
            app_name: app_name.into(),
            app_version: app_version.into(),
        }
    }
}
