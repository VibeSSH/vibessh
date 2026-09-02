use std::collections::HashMap;

use tokio::sync::Mutex;

use crate::ssh::client::FollowHandle;

/// Holds the live `docker logs -f` stream behind each open console.
///
/// Unlike `FileTransferManager` and `AiTurnManager`, which keep an
/// `AbortHandle` and abort a task, this keeps the handle itself and stops
/// the follow by **dropping** it - `FollowHandle`'s drop closes the remote
/// channel, which is what actually ends `docker logs -f` on the Node. A
/// forgotten entry here is not a leaked task but a command left running on
/// somebody's server, writing into a socket nobody reads, so every path out
/// of the console has to reach `stop`.
#[derive(Default)]
pub struct LogFollowManager {
    follows: Mutex<HashMap<String, FollowHandle>>,
}

impl LogFollowManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a new stream, replacing (and so stopping) any previous one
    /// under the same id.
    ///
    /// Replacing rather than rejecting matters: a console that reconnects -
    /// the tab was closed and reopened, or the page remounted - would
    /// otherwise leave the first `docker logs -f` running forever, and the
    /// second stream would deliver every line twice.
    pub async fn insert(&self, follow_id: String, handle: FollowHandle) {
        self.follows.lock().await.insert(follow_id, handle);
    }

    /// `true` if a stream with this id was actually found and stopped.
    /// `false` means it had already ended, which is a no-op rather than an
    /// error - the console closing twice is a race, not a fault.
    pub async fn stop(&self, follow_id: &str) -> bool {
        self.follows.lock().await.remove(follow_id).is_some()
    }
}
