use std::sync::Mutex;

use tauri::async_runtime::JoinHandle;

/// Holds the single in-flight `agent_client::run` task, if any. Starting a
/// new pairing attempt aborts whatever was running before - the UI only
/// ever has one "Add Server: Install Vibe Agent" flow open at a time, and
/// "regenerate code" is explicitly a cancel-and-restart, not a second
/// concurrent attempt.
#[derive(Default)]
pub struct PairingSession {
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl PairingSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn replace(&self, handle: JoinHandle<()>) {
        let mut guard = self.handle.lock().expect("pairing session mutex poisoned");
        if let Some(previous) = guard.take() {
            previous.abort();
        }
        *guard = Some(handle);
    }

    pub fn cancel(&self) {
        let mut guard = self.handle.lock().expect("pairing session mutex poisoned");
        if let Some(handle) = guard.take() {
            handle.abort();
        }
    }
}
