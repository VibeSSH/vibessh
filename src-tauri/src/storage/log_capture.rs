//! A small local, append-only capture of each Application's own log output -
//! plain text files under one directory, not SQLite (this is exactly the
//! "temp dir or UI cache" shape the design doc itself suggested, not a
//! queryable record). Exists because every `LogProvider::tail` this crate
//! has is a live pull from the runtime's own buffer (`docker logs`,
//! `journalctl`, ...), which is only ever as deep as that buffer currently
//! holds - `services::application_service::recreate_application` in
//! particular gives a Docker container a *brand new*, empty log buffer, so
//! without this, every Recreate would silently erase log history the user
//! might have opened the Logs tab specifically to go read. Capturing here
//! means history survives a recreate, a restart, and even the desktop app
//! itself closing and reopening - `services::application_service::
//! application_logs` is the only caller, see its own doc comment for how
//! the live fetch and this stored file get merged.

use std::path::PathBuf;

use uuid::Uuid;

use crate::errors::{AppError, AppResult};

/// Caps each Application's own capture file - a rolling window of recent
/// history, not an unbounded archive. Generous enough that "what happened
/// right before it crashed" is still there days later for anything that
/// isn't extremely chatty, small enough that this never becomes a real disk
/// consumer nobody asked for.
const MAX_STORED_LINES: usize = 5000;

pub struct LogCaptureStore {
    dir: PathBuf,
}

impl LogCaptureStore {
    /// Sync (not `tokio::fs`) deliberately - this only ever runs once, in
    /// Tauri's own sync `setup()` hook alongside every other directory this
    /// app creates at startup, and a one-time `create_dir_all` is cheap
    /// enough that reaching for `block_on` just to keep it async wouldn't
    /// buy anything. Every per-request method below (`tail`/`append`/
    /// `delete`/`rename`) is real async I/O, since those run on the hot
    /// path of an actual command.
    pub fn new(dir: PathBuf) -> AppResult<Self> {
        std::fs::create_dir_all(&dir).map_err(|err| AppError::Storage(format!("failed to create the log capture directory: {err}")))?;
        Ok(Self { dir })
    }

    fn path_for(&self, application_id: Uuid) -> PathBuf {
        self.dir.join(format!("{application_id}.log"))
    }

    /// Whatever's captured so far, oldest first - `[]` if nothing has ever
    /// been captured for this Application (a brand new Application, or one
    /// whose runtime has never actually produced any output yet).
    async fn read_all(&self, application_id: Uuid) -> AppResult<Vec<String>> {
        match tokio::fs::read_to_string(self.path_for(application_id)).await {
            Ok(contents) => Ok(contents.lines().map(str::to_string).collect()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(err) => Err(AppError::Storage(format!("failed to read captured logs: {err}"))),
        }
    }

    /// The last `max_lines` of whatever's captured - the read half of the
    /// merge `application_logs` does around every live fetch.
    pub async fn tail(&self, application_id: Uuid, max_lines: u32) -> AppResult<Vec<String>> {
        let all = self.read_all(application_id).await?;
        let skip = all.len().saturating_sub(max_lines as usize);
        Ok(all[skip..].to_vec())
    }

    /// Appends `new_lines` (already the caller's job to have deduplicated
    /// against what's already stored - see `application_service::
    /// merge_new_log_lines`) and prunes from the front if the result grows
    /// past `MAX_STORED_LINES`. A no-op write when `new_lines` is empty,
    /// rather than touching the file (and its mtime) for nothing.
    pub async fn append(&self, application_id: Uuid, new_lines: &[String]) -> AppResult<()> {
        if new_lines.is_empty() {
            return Ok(());
        }
        let mut all = self.read_all(application_id).await?;
        all.extend_from_slice(new_lines);
        if all.len() > MAX_STORED_LINES {
            let drop = all.len() - MAX_STORED_LINES;
            all.drain(..drop);
        }
        let mut contents = all.join("\n");
        contents.push('\n');
        tokio::fs::write(self.path_for(application_id), contents)
            .await
            .map_err(|err| AppError::Storage(format!("failed to save captured logs: {err}")))
    }

    /// Best-effort - called when the Application itself is deleted, same
    /// "the row/file is what actually matters, cleanup can't fail the
    /// caller" reasoning `services::application_service::delete_application`
    /// already applies to the OS keyring secrets it cleans up alongside a
    /// deleted row.
    pub async fn delete(&self, application_id: Uuid) {
        let _ = tokio::fs::remove_file(self.path_for(application_id)).await;
    }

    /// Carries a capture file over to a new Application id - used by
    /// `services::migration_service` so a migrated Application's log
    /// history survives the move instead of the old id's file just being
    /// orphaned (deleted along with the retired source row otherwise, same
    /// as any other Application). Best-effort and silent when there's
    /// nothing to carry over (a source Application whose runtime never
    /// produced any output yet) - not every migration has history to move.
    pub async fn rename(&self, old_application_id: Uuid, new_application_id: Uuid) {
        let _ = tokio::fs::rename(self.path_for(old_application_id), self.path_for(new_application_id)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> LogCaptureStore {
        let dir = std::env::temp_dir().join(format!("vibessh-log-capture-test-{}", Uuid::new_v4()));
        LogCaptureStore::new(dir).unwrap()
    }

    #[tokio::test]
    async fn tail_of_an_uncaptured_application_is_empty() {
        let store = temp_store();
        assert!(store.tail(Uuid::new_v4(), 50).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn append_then_tail_round_trips_the_lines() {
        let store = temp_store();
        let id = Uuid::new_v4();
        store.append(id, &["line one".to_string(), "line two".to_string()]).await.unwrap();
        assert_eq!(store.tail(id, 50).await.unwrap(), vec!["line one", "line two"]);
    }

    #[tokio::test]
    async fn repeated_appends_accumulate_in_order() {
        let store = temp_store();
        let id = Uuid::new_v4();
        store.append(id, &["a".to_string()]).await.unwrap();
        store.append(id, &["b".to_string(), "c".to_string()]).await.unwrap();
        assert_eq!(store.tail(id, 50).await.unwrap(), vec!["a", "b", "c"]);
    }

    #[tokio::test]
    async fn tail_only_returns_the_most_recent_lines() {
        let store = temp_store();
        let id = Uuid::new_v4();
        store.append(id, &["a".to_string(), "b".to_string(), "c".to_string()]).await.unwrap();
        assert_eq!(store.tail(id, 2).await.unwrap(), vec!["b", "c"]);
    }

    #[tokio::test]
    async fn appending_past_the_cap_drops_the_oldest_lines() {
        let store = temp_store();
        let id = Uuid::new_v4();
        let first_batch: Vec<String> = (0..MAX_STORED_LINES).map(|i| format!("line-{i}")).collect();
        store.append(id, &first_batch).await.unwrap();
        store.append(id, &["overflow".to_string()]).await.unwrap();

        let all = store.tail(id, MAX_STORED_LINES as u32 + 10).await.unwrap();
        assert_eq!(all.len(), MAX_STORED_LINES);
        assert_eq!(all.last().unwrap(), "overflow");
        assert_eq!(all.first().unwrap(), "line-1", "the very oldest line should have been dropped to make room");
    }

    #[tokio::test]
    async fn different_applications_dont_share_a_capture_file() {
        let store = temp_store();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        store.append(a, &["from a".to_string()]).await.unwrap();
        store.append(b, &["from b".to_string()]).await.unwrap();

        assert_eq!(store.tail(a, 50).await.unwrap(), vec!["from a"]);
        assert_eq!(store.tail(b, 50).await.unwrap(), vec!["from b"]);
    }

    #[tokio::test]
    async fn rename_carries_captured_history_over_to_the_new_id() {
        let store = temp_store();
        let old_id = Uuid::new_v4();
        let new_id = Uuid::new_v4();
        store.append(old_id, &["from before the migration".to_string()]).await.unwrap();

        store.rename(old_id, new_id).await;

        assert!(store.tail(old_id, 50).await.unwrap().is_empty());
        assert_eq!(store.tail(new_id, 50).await.unwrap(), vec!["from before the migration"]);
    }

    #[tokio::test]
    async fn rename_of_a_never_captured_application_is_a_silent_no_op() {
        let store = temp_store();
        store.rename(Uuid::new_v4(), Uuid::new_v4()).await;
    }

    #[tokio::test]
    async fn delete_removes_the_capture_file() {
        let store = temp_store();
        let id = Uuid::new_v4();
        store.append(id, &["something".to_string()]).await.unwrap();
        store.delete(id).await;
        assert!(store.tail(id, 50).await.unwrap().is_empty());
    }
}
