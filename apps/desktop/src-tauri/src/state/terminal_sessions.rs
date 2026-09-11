use std::collections::HashMap;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::ssh::TerminalHandle;

/// Tracks open interactive terminals by id so `write_to_terminal`/
/// `resize_terminal`/`close_terminal` commands can address one without
/// holding a reference across the IPC boundary. Removing a handle (on
/// `close`, or when the frontend never gets around to it and the app
/// closes) drops its `TerminalHandle`, which is what actually ends the
/// background task and closes the remote channel - see `ssh::client`.
#[derive(Default)]
pub struct TerminalSessionManager {
    sessions: Mutex<HashMap<Uuid, TerminalHandle>>,
}

impl TerminalSessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn insert(&self, terminal_id: Uuid, handle: TerminalHandle) {
        self.sessions.lock().await.insert(terminal_id, handle);
    }

    /// `false` means there's no open terminal with this id (already closed,
    /// or it never existed) - callers surface that as a "not found" error
    /// rather than silently dropping the write/resize.
    pub async fn write(&self, terminal_id: Uuid, data: Vec<u8>) -> bool {
        match self.sessions.lock().await.get(&terminal_id) {
            Some(handle) => {
                handle.write(data);
                true
            }
            None => false,
        }
    }

    pub async fn resize(&self, terminal_id: Uuid, cols: u32, rows: u32) -> bool {
        match self.sessions.lock().await.get(&terminal_id) {
            Some(handle) => {
                handle.resize(cols, rows);
                true
            }
            None => false,
        }
    }

    pub async fn close(&self, terminal_id: Uuid) {
        self.sessions.lock().await.remove(&terminal_id);
    }
}
