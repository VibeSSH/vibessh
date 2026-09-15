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

/// Closes a remote file and waits for the server to confirm it.
///
/// Every open in this module goes through this on its way out, including the
/// failure paths, and that is not tidiness - it is the difference between an
/// upload working and the whole session refusing to open anything.
///
/// `russh_sftp` counts open handles on the client side, against the ceiling
/// the server advertises through `limits@openssh.com`. That counter goes
/// *down* only when a close is awaited: `File`'s `Drop` sends the close
/// without waiting for the reply (it cannot await - `Drop` is synchronous),
/// so the server frees the handle while the client's own tally keeps it
/// forever. After as many transfers as the limit allows, every further open
/// fails with "handle limit reached" even though nothing is open anywhere.
/// It looks exactly like a server problem and is not one.
///
/// This is a bug in the library, not in how we call it, and it is reported
/// upstream: <https://github.com/AspectUnk/russh-sftp/issues/98> (open
/// against russh-sftp 2.4.0). Closing by hand everywhere is the workaround
/// until it is fixed there; when it is, this helper and the `close_remote`
/// calls can go back to being ordinary drops.
async fn close_remote(file: &mut russh_sftp::client::fs::File, path: &str) -> AppResult<()> {
    file.shutdown()
        .await
        .map_err(|err| AppError::Connection(format!("couldn't close {path}: {err}")))
}

/// The body of `read_file_range`'s loop, lifted out so the caller can close
/// the file whether it succeeded or not.
async fn read_range(
    file: &mut russh_sftp::client::fs::File,
    path: &str,
    offset: u64,
    buf: &mut [u8],
) -> AppResult<usize> {
    file.seek(std::io::SeekFrom::Start(offset))
        .await
        .map_err(|err| AppError::Connection(format!("couldn't seek in {path}: {err}")))?;

    let mut filled = 0usize;
    while filled < buf.len() {
        let read = file
            .read(&mut buf[filled..])
            .await
            .map_err(|err| AppError::Connection(format!("couldn't read {path}: {err}")))?;

        if read == 0 {
            break;
        }

        filled += read;
    }

    Ok(filled)
}

/// Same idea for the progress-reporting download: the loop is separate so a
/// failure halfway through still reaches the close below it.
async fn pour_down(
    remote: &mut russh_sftp::client::fs::File,
    local: &mut LocalFile,
    remote_path: &str,
    local_path: &Path,
    on_progress: &mut (dyn FnMut(u64) + Send),
) -> AppResult<()> {
    let mut buf = vec![0u8; TRANSFER_CHUNK_SIZE];

    loop {
        let read = remote
            .read(&mut buf)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't download {remote_path}: {err}")))?;

        if read == 0 {
            return Ok(());
        }

        local
            .write_all(&buf[..read])
            .await
            .map_err(|err| AppError::Internal(format!("couldn't write {}: {err}", local_path.display())))?;

        on_progress(read as u64);
    }
}

/// And the upload the same way.
async fn pour_up(
    local: &mut LocalFile,
    remote: &mut russh_sftp::client::fs::File,
    local_path: &Path,
    remote_path: &str,
    on_progress: &mut (dyn FnMut(u64) + Send),
) -> AppResult<()> {
    let mut buf = vec![0u8; TRANSFER_CHUNK_SIZE];

    loop {
        let read = local
            .read(&mut buf)
            .await
            .map_err(|err| AppError::Internal(format!("couldn't read {}: {err}", local_path.display())))?;

        if read == 0 {
            return Ok(());
        }

        remote
            .write_all(&buf[..read])
            .await
            .map_err(|err| AppError::Connection(format!("couldn't upload to {remote_path}: {err}")))?;

        on_progress(read as u64);
    }
}

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

    /// Opens, reads and closes by hand rather than calling `SftpSession::read`,
    /// which drops the file instead of closing it - see `close_remote`.
    pub async fn read_file(&self, path: &str) -> AppResult<Vec<u8>> {
        let sftp = self.sftp().await?;
        let mut file = sftp
            .open(path)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't open {path}: {err}")))?;

        let mut buffer = Vec::new();
        let read = file
            .read_to_end(&mut buffer)
            .await
            .map(|_| ())
            .map_err(|err| AppError::Connection(format!("couldn't read {path}: {err}")));

        let closed = close_remote(&mut file, path).await;
        read?;
        closed?;
        Ok(buffer)
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
        let written = file
            .write_all(contents)
            .await
            .map_err(|err| AppError::Connection(format!("couldn't write {path}: {err}")));

        let closed = close_remote(&mut file, path).await;
        written?;
        closed
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
        // The remote file is already open, so a failure here has to close it
        // rather than drop it - the same leak this module's `close_remote`
        // exists to prevent, just on a path that is easy to miss. Creating
        // the local file first would avoid it, but would also leave an empty
        // file behind whenever the *remote* open is what fails.
        let mut local = match LocalFile::create(local_path).await {
            Ok(local) => local,
            Err(err) => {
                let _ = close_remote(&mut remote, remote_path).await;
                return Err(AppError::Internal(format!("couldn't create {}: {err}", local_path.display())));
            }
        };
        let copied = tokio::io::copy(&mut remote, &mut local)
            .await
            .map(|_| ())
            .map_err(|err| AppError::Connection(format!("couldn't download {remote_path}: {err}")));

        let closed = close_remote(&mut remote, remote_path).await;
        copied?;
        closed?;

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
        let mut buf = vec![0u8; len];
        let filled = read_range(&mut file, path, offset, &mut buf).await;
        let closed = close_remote(&mut file, path).await;

        let filled = filled?;
        closed?;

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
        let copied = tokio::io::copy(&mut local, &mut remote)
            .await
            .map(|_| ())
            .map_err(|err| AppError::Connection(format!("couldn't upload to {remote_path}: {err}")));

        let closed = close_remote(&mut remote, remote_path).await;
        copied?;
        closed
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
        // The remote file is already open, so a failure here has to close it
        // rather than drop it - the same leak this module's `close_remote`
        // exists to prevent, just on a path that is easy to miss. Creating
        // the local file first would avoid it, but would also leave an empty
        // file behind whenever the *remote* open is what fails.
        let mut local = match LocalFile::create(local_path).await {
            Ok(local) => local,
            Err(err) => {
                let _ = close_remote(&mut remote, remote_path).await;
                return Err(AppError::Internal(format!("couldn't create {}: {err}", local_path.display())));
            }
        };
        let copied =
            pour_down(&mut remote, &mut local, remote_path, local_path, on_progress).await;
        let closed = close_remote(&mut remote, remote_path).await;
        copied?;
        closed?;

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
        let copied =
            pour_up(&mut local, &mut remote, local_path, remote_path, on_progress).await;
        let closed = close_remote(&mut remote, remote_path).await;
        copied?;
        closed
    }
}
