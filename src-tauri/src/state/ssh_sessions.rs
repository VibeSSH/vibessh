use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::ssh::SshSession;

/// Caches one live, authenticated SSH connection per server id so repeated
/// command execution doesn't re-authenticate (and re-run the TOFU host key
/// check) on every call - see `services::ssh_service` for what resolves a
/// server id into credentials and populates this. A `tokio::sync::Mutex`,
/// not `std::sync::Mutex`, because callers hold the lock while awaiting a
/// fresh connection on a cache miss.
#[derive(Default)]
pub struct SshSessionManager {
    sessions: Mutex<HashMap<Uuid, Arc<SshSession>>>,
}

impl SshSessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn get(&self, server_id: Uuid) -> Option<Arc<SshSession>> {
        self.sessions.lock().await.get(&server_id).cloned()
    }

    pub async fn insert(&self, server_id: Uuid, session: Arc<SshSession>) {
        self.sessions.lock().await.insert(server_id, session);
    }

    /// Drops the cached session without closing it - the caller already
    /// knows it's dead (e.g. a failed command), so the next `get_or_connect`
    /// reconnects instead of handing out the same broken handle again.
    pub async fn remove(&self, server_id: Uuid) {
        self.sessions.lock().await.remove(&server_id);
    }
}
