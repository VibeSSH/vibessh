//! File browsing/transfer over the SFTP subsystem (see `SshSession::sftp`
//! in `client.rs` for how the subsystem channel itself gets negotiated and
//! cached). This module only translates between `russh_sftp`'s API and the
//! app's own `RemoteFileEntry`/`AppResult` shapes.

use russh_sftp::protocol::OpenFlags;
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
}
