use std::collections::HashMap;

use tokio::sync::Mutex;
use tokio::task::AbortHandle;

/// Tracks the in-flight task behind each AI turn by its frontend-chosen
/// `turn_id`, so the chat panel's Stop button has something real to act on.
///
/// Same shape and same reasoning as `FileTransferManager`: aborting the
/// task drops the HTTP response mid-stream, which is a genuine stop rather
/// than a UI-only "stop showing me this" that leaves the request running
/// and still being billed by the provider. That distinction matters more
/// here than it does for a file transfer, because a metered endpoint keeps
/// charging for tokens nobody is going to read.
#[derive(Default)]
pub struct AiTurnManager {
    handles: Mutex<HashMap<String, AbortHandle>>,
}

impl AiTurnManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register(&self, turn_id: String, handle: AbortHandle) {
        self.handles.lock().await.insert(turn_id, handle);
    }

    /// `true` if a running turn with this id was found and aborted.
    /// `false` means it had already finished, which the caller treats as a
    /// no-op rather than an error - the user pressing Stop just as the last
    /// token arrives is a race they should never see the losing side of.
    pub async fn cancel(&self, turn_id: &str) -> bool {
        match self.handles.lock().await.remove(turn_id) {
            Some(handle) => {
                handle.abort();
                true
            }
            None => false,
        }
    }

    /// Drops the bookkeeping entry once a turn ends on its own. `cancel`
    /// already removes it on the cancel path, so this is a no-op there.
    pub async fn clear(&self, turn_id: &str) {
        self.handles.lock().await.remove(turn_id);
    }
}
