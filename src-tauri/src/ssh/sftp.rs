//! File browsing/transfer over the SFTP subsystem (see `SshSession::sftp`
//! in `client.rs` for how the subsystem channel itself gets negotiated and
//! cached). This module only translates between `russh_sftp`'s API and the
//! app's own `RemoteFileEntry`/`AppResult` shapes.

use std::path::Path;

use russh_sftp::protocol::OpenFlags;
use tokio::fs::File as LocalFile;
use tokio::io::AsyncWriteExt;

use vibessh_protocol::RemoteFileEntry;

use super::client::SshSession;
use crate::errors::{AppError, AppResult};

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
                }
            })
            .collect())
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
}
