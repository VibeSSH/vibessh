//! Durable, per-Application log history.
//!
//! An Application's runtime only ever offers a *tail*: `docker logs --tail N`
//! shows the last N lines of the container that exists right now, and a
//! recreated container starts empty. This store is what makes the Logs tab
//! show more than that - each poll appends whatever is new to a file that
//! survives restarts, recreates and reconnections.
//!
//! **Append-only, with occasional compaction.** The previous implementation
//! read the entire file, parsed it into a `Vec<String>`, extended it, joined
//! it back together and rewrote the whole thing - on every poll. With the
//! console polling every two seconds and a 5000-line cap, that was a
//! ~500 KB read plus a ~500 KB write every two seconds per open Application,
//! forever. Appending costs the size of the new lines instead, and the file
//! is only rewritten when it has grown past `COMPACT_ABOVE_BYTES`.
//!
//! **Writes are serialized per Application, and are atomic.** The old
//! read-modify-write had no lock, so two concurrent `application_logs` calls
//! for the same Application - a console poll and a manual refresh, which the
//! UI issues independently - could interleave and silently lose whichever
//! batch finished writing first. And `tokio::fs::write` truncates before it
//! writes, so a crash mid-write left a truncated log rather than the
//! previous one. Compaction now writes a sibling temp file and renames it
//! into place, which is atomic on every platform this runs on.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};

/// How many lines a compaction keeps.
///
/// Note this is what compaction trims *to*, not a hard ceiling the file
/// never exceeds: compaction is triggered by size (see
/// `COMPACT_ABOVE_BYTES`), so between two compactions the file can hold
/// somewhat more than this. What is guaranteed is that the file stays
/// bounded and that the newest lines are the ones kept.
const MAX_STORED_LINES: usize = 5000;

/// Compact once the file grows past this. Deliberately a byte threshold
/// rather than a line count: checking size is a single `metadata` call,
/// while counting lines would mean reading the whole file on every append -
/// exactly the cost this design exists to avoid.
///
/// Sized so that a file at the 5000-line cap (a few hundred KB of typical
/// log lines) compacts occasionally rather than constantly.
const COMPACT_ABOVE_BYTES: u64 = 2 * 1024 * 1024;

pub struct LogCaptureStore {
    dir: PathBuf,
    /// One lock per Application. Held across an append or a compaction so
    /// two concurrent polls cannot interleave - see the module doc.
    locks: Mutex<HashMap<Uuid, Arc<Mutex<()>>>>,
}

impl LogCaptureStore {
    pub fn new(dir: PathBuf) -> AppResult<Self> {
        std::fs::create_dir_all(&dir).map_err(|err| AppError::Storage(format!("failed to create the log capture directory: {err}")))?;
        Ok(Self { dir, locks: Mutex::new(HashMap::new()) })
    }

    fn path_for(&self, application_id: Uuid) -> PathBuf {
        self.dir.join(format!("{application_id}.log"))
    }

    async fn lock_for(&self, application_id: Uuid) -> Arc<Mutex<()>> {
        self.locks.lock().await.entry(application_id).or_default().clone()
    }

    async fn read_all(&self, application_id: Uuid) -> AppResult<Vec<String>> {
        match tokio::fs::read_to_string(self.path_for(application_id)).await {
            Ok(contents) => Ok(contents.lines().map(str::to_string).collect()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(err) => Err(AppError::Storage(format!("failed to read captured logs: {err}"))),
        }
    }

    pub async fn tail(&self, application_id: Uuid, max_lines: u32) -> AppResult<Vec<String>> {
        let all = self.read_all(application_id).await?;
        let skip = all.len().saturating_sub(max_lines as usize);
        Ok(all[skip..].to_vec())
    }

    pub async fn append(&self, application_id: Uuid, new_lines: &[String]) -> AppResult<()> {
        if new_lines.is_empty() {
            return Ok(());
        }
        let lock = self.lock_for(application_id).await;
        let _guard = lock.lock().await;

        let path = self.path_for(application_id);
        let mut buffer = String::with_capacity(new_lines.iter().map(|line| line.len() + 1).sum());
        for line in new_lines {
            buffer.push_str(line);
            buffer.push('\n');
        }

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
            .map_err(|err| AppError::Storage(format!("failed to open the captured log: {err}")))?;
        file.write_all(buffer.as_bytes())
            .await
            .map_err(|err| AppError::Storage(format!("failed to save captured logs: {err}")))?;
        // Flushed before the size check so compaction sees the real size and
        // the lines are durable even if compaction then fails.
        file.flush().await.map_err(|err| AppError::Storage(format!("failed to save captured logs: {err}")))?;
        drop(file);

        let too_big = tokio::fs::metadata(&path).await.map(|meta| meta.len() > COMPACT_ABOVE_BYTES).unwrap_or(false);
        if too_big {
            self.compact(application_id).await?;
        }
        Ok(())
    }

    /// Trims the file back to its last `MAX_STORED_LINES` lines.
    ///
    /// Writes a temp file and renames it over the original, so a crash
    /// partway through leaves the previous complete log rather than a
    /// truncated one. Called with the per-Application lock already held.
    async fn compact(&self, application_id: Uuid) -> AppResult<()> {
        let all = self.read_all(application_id).await?;
        if all.len() <= MAX_STORED_LINES {
            return Ok(());
        }
        let kept = &all[all.len() - MAX_STORED_LINES..];
        let mut contents = kept.join("\n");
        contents.push('\n');

        let path = self.path_for(application_id);
        // A sibling of the real file, so the rename below stays on one
        // filesystem - a rename across filesystems is not atomic and on some
        // platforms is not even permitted.
        let temp = path.with_extension("log.compacting");
        tokio::fs::write(&temp, contents)
            .await
            .map_err(|err| AppError::Storage(format!("failed to compact captured logs: {err}")))?;
        tokio::fs::rename(&temp, &path)
            .await
            .map_err(|err| AppError::Storage(format!("failed to compact captured logs: {err}")))?;
        Ok(())
    }

    /// Puts this Application's captured history aside and starts a new one.
    ///
    /// The history is what grows: it survives restarts, recreates and
    /// reconnections, and on a chatty Application it reaches thousands of
    /// lines that nobody wants to scroll past to reach today's failure. This
    /// empties it - and keeps every line, in `archive/`, because a log
    /// somebody deletes is usually a log somebody wants ten minutes later.
    ///
    /// **A rename, not a copy.** It is atomic, costs nothing on a two-megabyte
    /// file, and cannot leave a half-written duplicate behind if the disk
    /// fills or the app is closed mid-write. `archive/` is inside the same
    /// directory precisely so the rename stays within one filesystem.
    ///
    /// **What this cannot clear** is the runtime's own buffer. `docker logs`
    /// still holds whatever the container has written, and the next poll will
    /// capture it again - erasing that means truncating a file under
    /// `/var/lib/docker` as root, which is not this store's business. So the
    /// tab refills with the container's current tail rather than staying
    /// empty, and the caller says so before doing it.
    ///
    /// `Ok(None)` when there was nothing captured - no file, or an empty one.
    /// Nothing is written in that case, so an accidental second press does not
    /// litter `archive/` with empty files.
    pub async fn archive_and_clear(&self, application_id: Uuid) -> AppResult<Option<PathBuf>> {
        let lock = self.lock_for(application_id).await;
        let _guard = lock.lock().await;

        let path = self.path_for(application_id);
        let has_content = tokio::fs::metadata(&path).await.map(|meta| meta.len() > 0).unwrap_or(false);
        if !has_content {
            return Ok(None);
        }

        let archive_dir = self.dir.join("archive");
        tokio::fs::create_dir_all(&archive_dir)
            .await
            .map_err(|err| AppError::Storage(format!("failed to create the log archive directory: {err}")))?;

        let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let destination = archive_dir.join(format!("{application_id}-{stamp}.log"));
        tokio::fs::rename(&path, &destination)
            .await
            .map_err(|err| AppError::Storage(format!("failed to archive the captured logs: {err}")))?;

        Ok(Some(destination))
    }

    pub async fn delete(&self, application_id: Uuid) {
        let lock = self.lock_for(application_id).await;
        let _guard = lock.lock().await;
        let _ = tokio::fs::remove_file(self.path_for(application_id)).await;
    }

    pub async fn rename(&self, old_application_id: Uuid, new_application_id: Uuid) {
        let lock = self.lock_for(old_application_id).await;
        let _guard = lock.lock().await;
        let _ = tokio::fs::rename(self.path_for(old_application_id), self.path_for(new_application_id)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> LogCaptureStore {
        LogCaptureStore::new(std::env::temp_dir().join(format!("vibessh-log-capture-test-{}", Uuid::new_v4()))).unwrap()
    }

    fn lines(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    /// Clearing a log is the one operation here that destroys something, so
    /// what it keeps matters more than what it removes.
    #[tokio::test]
    async fn clearing_keeps_every_line_in_the_archive() {
        let store = temp_store();
        let id = Uuid::new_v4();
        store.append(id, &lines(&["first", "second", "third"])).await.unwrap();

        let archived = store.archive_and_clear(id).await.unwrap().expect("a captured log is archived");

        assert_eq!(store.tail(id, 100).await.unwrap(), Vec::<String>::new(), "the live history is empty afterwards");
        let kept = tokio::fs::read_to_string(&archived).await.unwrap();
        assert_eq!(kept.lines().collect::<Vec<_>>(), vec!["first", "second", "third"]);
    }

    /// The Application keeps running, and the next poll appends to a file that
    /// is no longer there. It has to come back rather than error.
    #[tokio::test]
    async fn capturing_continues_after_a_clear() {
        let store = temp_store();
        let id = Uuid::new_v4();
        store.append(id, &lines(&["before"])).await.unwrap();
        store.archive_and_clear(id).await.unwrap();

        store.append(id, &lines(&["after"])).await.unwrap();

        assert_eq!(store.tail(id, 100).await.unwrap(), lines(&["after"]));
    }

    /// Pressing it twice, or on an Application that has never said anything,
    /// must not fill the archive with empty files.
    #[tokio::test]
    async fn clearing_nothing_archives_nothing() {
        let store = temp_store();
        let id = Uuid::new_v4();

        assert!(store.archive_and_clear(id).await.unwrap().is_none(), "an application with no captured log");

        store.append(id, &lines(&["one"])).await.unwrap();
        assert!(store.archive_and_clear(id).await.unwrap().is_some());
        assert!(store.archive_and_clear(id).await.unwrap().is_none(), "a second press with nothing new");
    }

    /// One Application's clear is not another's - they share a directory and
    /// differ only by the id in the filename.
    #[tokio::test]
    async fn clearing_one_application_leaves_the_others_alone() {
        let store = temp_store();
        let cleared = Uuid::new_v4();
        let untouched = Uuid::new_v4();
        store.append(cleared, &lines(&["mine"])).await.unwrap();
        store.append(untouched, &lines(&["theirs"])).await.unwrap();

        store.archive_and_clear(cleared).await.unwrap();

        assert_eq!(store.tail(untouched, 100).await.unwrap(), lines(&["theirs"]));
    }

    #[tokio::test]
    async fn appended_lines_read_back_in_order() {
        let store = temp_store();
        let id = Uuid::new_v4();

        store.append(id, &lines(&["first", "second"])).await.unwrap();
        store.append(id, &lines(&["third"])).await.unwrap();

        assert_eq!(store.tail(id, 10).await.unwrap(), lines(&["first", "second", "third"]));
    }

    #[tokio::test]
    async fn tail_returns_only_the_last_n_lines() {
        let store = temp_store();
        let id = Uuid::new_v4();
        store.append(id, &lines(&["a", "b", "c", "d"])).await.unwrap();

        assert_eq!(store.tail(id, 2).await.unwrap(), lines(&["c", "d"]));
    }

    #[tokio::test]
    async fn tail_of_an_application_with_no_captured_logs_is_empty() {
        assert!(temp_store().tail(Uuid::new_v4(), 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn appending_nothing_does_not_create_a_file() {
        let store = temp_store();
        let id = Uuid::new_v4();
        store.append(id, &[]).await.unwrap();
        assert!(!store.path_for(id).exists());
    }

    /// The regression test for the interleaving finding: the old
    /// read-modify-write had no lock, so two concurrent polls for the same
    /// Application could each read the same starting state and one batch
    /// would be lost.
    #[tokio::test]
    async fn concurrent_appends_do_not_lose_lines() {
        let store = Arc::new(temp_store());
        let id = Uuid::new_v4();

        let mut tasks = Vec::new();
        for batch in 0..20 {
            let store = store.clone();
            tasks.push(tokio::spawn(async move {
                store.append(id, &lines(&[&format!("line-{batch}")])).await.unwrap();
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }

        let captured = store.tail(id, 100).await.unwrap();
        assert_eq!(captured.len(), 20, "every concurrent append must survive: {captured:?}");
        for batch in 0..20 {
            assert!(captured.contains(&format!("line-{batch}")), "lost line-{batch}");
        }
    }

    /// Compaction is triggered by file *size*, so the guarantee is that the
    /// file stays bounded and keeps the newest lines - not that the line
    /// count never exceeds `MAX_STORED_LINES` at any instant.
    #[tokio::test]
    async fn compaction_bounds_the_file_and_keeps_the_most_recent_lines() {
        let store = temp_store();
        let id = Uuid::new_v4();

        // Padded so the file crosses COMPACT_ABOVE_BYTES more than once.
        let padding = "x".repeat(400);
        let total = MAX_STORED_LINES * 2;
        for batch in 0..total {
            store.append(id, &lines(&[&format!("{batch}-{padding}")])).await.unwrap();
        }

        let size = tokio::fs::metadata(store.path_for(id)).await.unwrap().len();
        assert!(size <= COMPACT_ABOVE_BYTES * 2, "the file must stay bounded, got {size} bytes");

        let captured = store.tail(id, total as u32).await.unwrap();
        assert!(captured.len() < total, "the oldest lines must have been dropped");
        // The newest line survives; the very oldest does not.
        assert!(captured.last().unwrap().starts_with(&format!("{}-", total - 1)));
        assert!(!captured.iter().any(|line| line.starts_with("0-")));
    }

    #[tokio::test]
    async fn delete_removes_the_captured_history() {
        let store = temp_store();
        let id = Uuid::new_v4();
        store.append(id, &lines(&["a"])).await.unwrap();

        store.delete(id).await;
        assert!(store.tail(id, 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn rename_carries_history_to_a_new_application_id() {
        let store = temp_store();
        let old = Uuid::new_v4();
        let new = Uuid::new_v4();
        store.append(old, &lines(&["carried over"])).await.unwrap();

        store.rename(old, new).await;
        assert!(store.tail(old, 10).await.unwrap().is_empty());
        assert_eq!(store.tail(new, 10).await.unwrap(), lines(&["carried over"]));
    }
}
