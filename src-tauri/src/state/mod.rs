mod pairing_session;
mod ssh_sessions;
mod terminal_sessions;

pub use pairing_session::PairingSession;
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
