//! File browsing/transfer over the SFTP subsystem (see `SshSession::sftp`
//! in `client.rs` for how the subsystem channel itself gets negotiated and
//! cached). This module only translates between `russh_sftp`'s API and the
//! app's own `RemoteFileEntry`/`AppResult` shapes.

use std::path::Path;

use russh_sftp::protocol::OpenFlags;
use tokio::fs::File as LocalFile;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use vibessh_protocol::RemoteFileEntry;

use super::client::SshSession;
use crate::errors::{AppError, AppResult};

/// Chunk size for the progress-reporting upload/download variants below -
/// small enough to report progress responsively, large enough not to spend
/// most of the transfer on per-chunk SFTP protocol overhead.
const TRANSFER_CHUNK_SIZE: usize = 256 * 1024;

impl SshSession {
    pub async fn list_directory(&self, path: &str) -> AppResult<Vec<RemoteFileEntry>> {
        let sftp = self.sftp().await?;
        let entries = sftp
            .read_dir(path)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't list {path}: {err}")))?;

        Ok(entries
            .map(|entry| {
                let metadata = entry.metadata();
                RemoteFileEntry {
                    name: entry.file_name(),
                    path: entry.path(),
                    is_dir: metadata.is_dir(),
                    is_symlink: metadata.is_symlink(),
                    size: metadata.len(),
                    modified_at: metadata.modified().ok().map(Into::into),
                    // Masked to the permission bits only - the raw field
                    // also carries file-type bits (S_IFDIR etc.), which
                    // `is_dir`/`is_symlink` above already cover separately.
                    permissions: metadata.permissions.map(|bits| bits & 0o7777),
                }
            })
            .collect())
    }

    /// `lstat`, not `stat` - does not follow a symlink, so this is what
    /// `files::sandbox` uses to detect "is this path itself a symlink"
    /// before deciding whether its target needs a separate escape check
    /// (see that module's own doc comment for why `list_directory`'s
    /// per-entry `is_symlink` isn't enough on its own).
    pub async fn symlink_metadata(&self, path: &str) -> AppResult<RemoteFileEntry> {
        let sftp = self.sftp().await?;
        let metadata = sftp
            .symlink_metadata(path)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't stat {path}: {err}")))?;
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        Ok(RemoteFileEntry {
            name,
            path: path.to_string(),
            is_dir: metadata.is_dir(),
            is_symlink: metadata.is_symlink(),
            size: metadata.len(),
            modified_at: metadata.modified().ok().map(Into::into),
            permissions: metadata.permissions.map(|bits| bits & 0o7777),
        })
    }

    /// Where a symlink actually points - used to check whether that target
    /// resolves outside the application's sandboxed root before this
    /// runtime ever follows it.
    pub async fn read_link(&self, path: &str) -> AppResult<String> {
        let sftp = self.sftp().await?;
        sftp.read_link(path)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't read the symlink {path}: {err}")))
    }

    /// The SFTP `REALPATH` operation - resolves `..`/`.`/symlinks server-side
    /// into an absolute, canonical path, the same jailing primitive
    /// `Path::canonicalize` gives `LocalApplicationFileProvider` for free.
    /// `Err` (not a guess) when the server can't resolve it at all - most
    /// commonly because nothing exists at `path` yet, which callers handle
    /// by canonicalizing the parent directory instead (see
    /// `files::sftp::SftpApplicationFileProvider::resolve`).
    pub async fn canonicalize_path(&self, path: &str) -> AppResult<String> {
        let sftp = self.sftp().await?;
        sftp.canonicalize(path)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't resolve {path}: {err}")))
    }

    pub async fn rename(&self, old_path: &str, new_path: &str) -> AppResult<()> {
        let sftp = self.sftp().await?;
        sftp.rename(old_path, new_path)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't rename {old_path} to {new_path}: {err}")))
    }

    pub async fn remove_file(&self, path: &str) -> AppResult<()> {
        let sftp = self.sftp().await?;
        sftp.remove_file(path)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't delete {path}: {err}")))
    }

    /// Only removes an already-empty directory - the SFTP protocol itself
    /// has no recursive delete; a caller wanting to remove a non-empty
    /// directory tree walks it first (see
    /// `files::sftp::SftpApplicationFileProvider::delete`).
    pub async fn remove_dir(&self, path: &str) -> AppResult<()> {
        let sftp = self.sftp().await?;
        sftp.remove_dir(path)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't delete the directory {path}: {err}")))
    }

    /// `mode` is a raw POSIX permission value (e.g. `0o755`) - the caller
    /// (`services::application_files_service`) is what validates it's in
    /// range before this ever runs a request past the SSH server.
    pub async fn set_permissions(&self, path: &str, mode: u32) -> AppResult<()> {
        let sftp = self.sftp().await?;
        let mut metadata = sftp
            .metadata(path)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't stat {path}: {err}")))?;
        metadata.permissions = Some(mode);
        sftp.set_metadata(path, metadata)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't change permissions on {path}: {err}")))
    }

    pub async fn read_file(&self, path: &str) -> AppResult<Vec<u8>> {
        let sftp = self.sftp().await?;
        sftp.read(path)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't read {path}: {err}")))
    }

    pub async fn create_directory(&self, path: &str) -> AppResult<()> {
        let sftp = self.sftp().await?;
        sftp.create_dir(path)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't create directory {path}: {err}")))
    }

    /// Creates `path` if it doesn't exist yet, truncates it if it does -
    /// "save" semantics for a file editor, not `SftpSession::write`'s
    /// plain overwrite-only behavior (which fails on a path that isn't
    /// already there).
    pub async fn write_file(&self, path: &str, contents: &[u8]) -> AppResult<()> {
        let sftp = self.sftp().await?;
        let mut file = sftp
            .open_with_flags(path, OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't open {path} for writing: {err}")))?;
        file.write_all(contents)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't write {path}: {err}")))?;
        file.shutdown()
            .await
            .map_err(|err| AppError::Connection(format!("couldn't finish writing {path}: {err}")))
    }

    /// Streams `remote_path` straight to `local_path` via `tokio::io::copy` -
    /// unlike `read_file`, this never materializes the whole file as a
    /// `Vec<u8>` in memory (let alone twice, once as SFTP protocol frames and
    /// again as a JSON array crossing the Tauri IPC bridge), which matters
    /// once "a file" isn't a few KB of text but something upload/download is
    /// actually for.
    pub async fn download_file(&self, remote_path: &str, local_path: &Path) -> AppResult<()> {
        let sftp = self.sftp().await?;
        let mut remote = sftp
            .open(remote_path)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't open {remote_path} for reading: {err}")))?;
        let mut local = LocalFile::create(local_path)
            .await
            .map_err(|err| AppError::Internal(format!("couldn't create {}: {err}", local_path.display())))?;
        tokio::io::copy(&mut remote, &mut local)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't download {remote_path}: {err}")))?;
        local
            .flush()
            .await
            .map_err(|err| AppError::Internal(format!("couldn't finish writing {}: {err}", local_path.display())))
    }

    /// The upload counterpart of `download_file` - same streaming-copy
    /// reasoning, same create-or-truncate "save" semantics as `write_file`.
    /// Reads at most `len` bytes starting at `offset`.
    ///
    /// Short reads are normal here rather than an error: near the end of the
    /// file there is simply less than `len` left, and that is how the caller
    /// learns it has reached the end.
    pub async fn read_file_range(&self, path: &str, offset: u64, len: usize) -> AppResult<Vec<u8>> {
        let sftp = self.sftp().await?;
        let mut file = sftp
            .open(path)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't open {path}: {err}")))?;
        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(|err| AppError::Connection(format!("couldn't seek in {path}: {err}")))?;
        let mut buf = vec![0u8; len];
        let mut filled = 0usize;
        while filled < len {
            let read = file
                .read(&mut buf[filled..])
                .await
                .map_err(|err| AppError::Connection(format!("couldn't read {path}: {err}")))?;
            if read == 0 {
                break;
            }
            filled += read;
        }
        buf.truncate(filled);
        Ok(buf)
    }

    pub async fn upload_file(&self, local_path: &Path, remote_path: &str) -> AppResult<()> {
        let mut local = LocalFile::open(local_path)
            .await
            .map_err(|err| AppError::Internal(format!("couldn't open {}: {err}", local_path.display())))?;
        let sftp = self.sftp().await?;
        let mut remote = sftp
            .open_with_flags(remote_path, OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't open {remote_path} for writing: {err}")))?;
        tokio::io::copy(&mut local, &mut remote)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't upload to {remote_path}: {err}")))?;
        remote
            .shutdown()
            .await
            .map_err(|err| AppError::Connection(format!("couldn't finish writing {remote_path}: {err}")))
    }

    /// Same as `download_file`, but calls `on_progress` with the number of
    /// bytes just transferred after every chunk (not a running total) - a
    /// caller tracking cumulative bytes/speed just sums what it's given,
    /// rather than having to diff two totals itself. A manual chunked copy
    /// loop instead of `tokio::io::copy`, which has no per-chunk hook to
    /// report through - used by the Application Files transfer queue
    /// (`files::sftp`), not by the older Node Files module, which has no
    /// progress UI to feed.
    pub async fn download_file_with_progress(
        &self,
        remote_path: &str,
        local_path: &Path,
        on_progress: &mut (dyn FnMut(u64) + Send),
    ) -> AppResult<()> {
        let sftp = self.sftp().await?;
        let mut remote = sftp
            .open(remote_path)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't open {remote_path} for reading: {err}")))?;
        let mut local = LocalFile::create(local_path)
            .await
            .map_err(|err| AppError::Internal(format!("couldn't create {}: {err}", local_path.display())))?;
        let mut buf = vec![0u8; TRANSFER_CHUNK_SIZE];
        loop {
            let read = remote
                .read(&mut buf)
                .await
                .map_err(|err| AppError::Connection(format!("couldn't download {remote_path}: {err}")))?;
            if read == 0 {
                break;
            }
            local
                .write_all(&buf[..read])
                .await
                .map_err(|err| AppError::Internal(format!("couldn't write {}: {err}", local_path.display())))?;
            on_progress(read as u64);
        }
        local
            .flush()
            .await
            .map_err(|err| AppError::Internal(format!("couldn't finish writing {}: {err}", local_path.display())))
    }

    /// The upload counterpart of `download_file_with_progress`.
    pub async fn upload_file_with_progress(
        &self,
        local_path: &Path,
        remote_path: &str,
        on_progress: &mut (dyn FnMut(u64) + Send),
    ) -> AppResult<()> {
        let mut local = LocalFile::open(local_path)
            .await
            .map_err(|err| AppError::Internal(format!("couldn't open {}: {err}", local_path.display())))?;
        let sftp = self.sftp().await?;
        let mut remote = sftp
            .open_with_flags(remote_path, OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't open {remote_path} for writing: {err}")))?;
        let mut buf = vec![0u8; TRANSFER_CHUNK_SIZE];
        loop {
            let read = local
                .read(&mut buf)
                .await
                .map_err(|err| AppError::Internal(format!("couldn't read {}: {err}", local_path.display())))?;
            if read == 0 {
                break;
            }
            remote
                .write_all(&buf[..read])
                .await
                .map_err(|err| AppError::Connection(format!("couldn't upload to {remote_path}: {err}")))?;
            on_progress(read as u64);
        }
        remote
            .shutdown()
            .await
            .map_err(|err| AppError::Connection(format!("couldn't finish writing {remote_path}: {err}")))
    }
}
