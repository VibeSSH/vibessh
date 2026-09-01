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
    /// One lock per server, held by `services::ssh_service::get_or_connect`
    /// across its whole check-connect-insert sequence.
    ///
    /// Without it that sequence is a check-then-act race, and a trivially
    /// reachable one: the dashboard's 6-second metrics poll and any user
    /// action on the same Node routinely overlap. Both callers miss the
    /// cache, both perform a full TCP + auth handshake, and the second
    /// `insert` silently replaces the first - whose session is then never
    /// closed and never removed, leaking for the life of the process while
    /// its connection stays open on the Node.
    ///
    /// Keyed per server rather than one global lock, so connecting to a slow
    /// or unreachable Node does not serialize connections to every other one.
    connect_locks: Mutex<HashMap<Uuid, Arc<Mutex<()>>>>,
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

    /// The per-server connect lock. The caller holds the returned guard
    /// across its own cache re-check and connect - see `connect_locks`.
    pub async fn connect_lock(&self, server_id: Uuid) -> Arc<Mutex<()>> {
        self.connect_locks.lock().await.entry(server_id).or_default().clone()
    }

    /// Drops the cached session **and closes it**.
    ///
    /// Dropping the `Arc` alone was the previous behaviour, on the reasoning
    /// that the caller already knows the session is dead. That holds for a
    /// session killed by a transport error, but not for the other callers
    /// that reach this - a Node leaving the mesh, a reconnect after a
    /// recoverable failure - where the connection is still live and the Node
    /// would hold it open until its own idle timeout fired.
    /// `SshSession::close` sends a real SSH disconnect; a failure here is
    /// expected on an already-broken session and ignored, the point is to
    /// make the attempt.
    pub async fn remove(&self, server_id: Uuid) {
        // The map lock is released before awaiting the close: holding it
        // across a network round trip would block every other Node's
        // lookups behind one unreachable host.
        let session = self.sessions.lock().await.remove(&server_id);
        if let Some(session) = session {
            session.close().await;
        }
    }
}
