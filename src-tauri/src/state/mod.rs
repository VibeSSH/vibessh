mod pairing_session;

pub use pairing_session::PairingSession;

/// Shared application state, injected into every Tauri command via `State<AppState>`.
///
/// Etap 1 only needs static metadata. Later stages attach the server
/// repository (Etap 2) and the SSH session manager (Etap 3) here as
/// additional fields, each behind its own lock so unrelated commands never
/// contend on the same mutex. `PairingSession` (Etap H) is managed
/// separately rather than as a field here, since it has nothing to do with
/// static app metadata and would just make this struct's locking story
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
