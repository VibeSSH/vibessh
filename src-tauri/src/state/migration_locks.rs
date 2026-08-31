use std::collections::HashSet;

use tokio::sync::Mutex;
use uuid::Uuid;

/// Guards against migrating the same Application twice at once - a
/// multi-minute, multi-step operation (stop, copy files, cut over DNS,
/// delete the old instance), so a second concurrent call must fail fast
/// rather than block for the whole duration, same "bookkeeping, not a real
/// blocking lock" shape `state::FileTransferManager` already uses for a
/// similarly long-lived operation.
#[derive(Default)]
pub struct MigrationLockManager {
    in_progress: Mutex<HashSet<Uuid>>,
}

impl MigrationLockManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// `true` once this call is the one that claimed the lock - `false`
    /// means a migration for this Application is already running.
    pub async fn try_start(&self, application_id: Uuid) -> bool {
        self.in_progress.lock().await.insert(application_id)
    }

    pub async fn finish(&self, application_id: Uuid) {
        self.in_progress.lock().await.remove(&application_id);
    }
}
