use std::collections::HashMap;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::models::PortForwardStatus;
use crate::ssh::PortForwardHandle;

/// Tracks open SSH tunnels by id, same shape as `TerminalSessionManager`
/// (see its own doc comment) - purely in-memory, nothing persisted, since a
/// tunnel only means anything for as long as the SSH session under it does.
/// Removing a handle (on `stop`, or when the app closes and this state is
/// simply dropped) drops its `PortForwardHandle`, which is what actually
/// closes the listener/cancels the Node-side forward - see
/// `ssh::port_forward`.
#[derive(Default)]
pub struct PortForwardManager {
    forwards: Mutex<HashMap<Uuid, (PortForwardStatus, PortForwardHandle)>>,
}

impl PortForwardManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn insert(&self, status: PortForwardStatus, handle: PortForwardHandle) {
        self.forwards.lock().await.insert(status.id, (status, handle));
    }

    pub async fn list(&self) -> Vec<PortForwardStatus> {
        self.forwards.lock().await.values().map(|(status, _)| status.clone()).collect()
    }

    /// `false` means there's no open forward with this id (already stopped,
    /// or it never existed) - the same "not found" convention
    /// `TerminalSessionManager::write`/`resize` already use.
    pub async fn stop(&self, id: Uuid) -> bool {
        match self.forwards.lock().await.remove(&id) {
            Some((_, handle)) => {
                handle.stop();
                true
            }
            None => false,
        }
    }

    /// Every forward tied to one server, stopped and removed - used when
    /// that server itself is deleted, so a stale tunnel doesn't outlive the
    /// server row it was opened against.
    pub async fn stop_all_for_server(&self, server_id: Uuid) {
        let mut forwards = self.forwards.lock().await;
        let stale: Vec<Uuid> = forwards.iter().filter(|(_, (status, _))| status.server_id == server_id).map(|(id, _)| *id).collect();
        for id in stale {
            if let Some((_, handle)) = forwards.remove(&id) {
                handle.stop();
            }
        }
    }
}
