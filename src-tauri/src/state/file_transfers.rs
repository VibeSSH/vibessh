use std::collections::HashMap;

use tokio::sync::Mutex;
use tokio::task::AbortHandle;

/// Tracks the in-flight `tokio::spawn` task behind each Application Files
/// upload/download by a frontend-chosen `transfer_id`, so the transfer
/// queue's "Cancel" button (design brief section 111: "Operacje: cancel,
/// retry, clear completed") has something real to act on - aborting the
/// task is a genuine cancellation (drops whatever I/O it was doing
/// mid-flight), not a UI-only "give up waiting" that leaves the transfer
/// still running in the background. No "Pause" here on purpose - neither
/// the local filesystem copy nor SFTP has a resumable-transfer protocol
/// this app speaks, and the brief itself says not to fake one.
#[derive(Default)]
pub struct FileTransferManager {
    handles: Mutex<HashMap<String, AbortHandle>>,
}

impl FileTransferManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register(&self, transfer_id: String, handle: AbortHandle) {
        self.handles.lock().await.insert(transfer_id, handle);
    }

    /// `true` if a running transfer with this id was actually found and
    /// aborted - `false` means it already finished (or never existed),
    /// which the caller treats as a no-op, not an error.
    pub async fn cancel(&self, transfer_id: &str) -> bool {
        match self.handles.lock().await.remove(transfer_id) {
            Some(handle) => {
                handle.abort();
                true
            }
            None => false,
        }
    }

    /// Removes the bookkeeping entry once a transfer finishes on its own
    /// (success or a real error, not a cancel) - `cancel` already removes
    /// it for the cancel path, so this is a no-op there.
    pub async fn clear(&self, transfer_id: &str) {
        self.handles.lock().await.remove(transfer_id);
    }
}
