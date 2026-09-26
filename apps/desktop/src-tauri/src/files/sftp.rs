//! `SftpApplicationFileProvider` - `ApplicationFileProvider` for a Remote
//! Application, over the exact same `SshSession`/SFTP machinery the older
//! Node Files module already uses (`ssh::sftp`) - no second transfer
//! protocol, just this sandboxing layer on top. Jailed to
//! `working_directory` via the SFTP `REALPATH` operation
//! (`SshSession::canonicalize_path`), the server-side equivalent of
//! `Path::canonicalize` - it resolves symlinks the same way, just on the
//! remote host instead of locally.

use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use vibessh_protocol::RemoteFileEntry;

use crate::errors::{AppError, AppResult};
use crate::ssh::SshSession;

use super::sandbox::{is_within_root, relativize, sanitize_relative_path};
use super::{ApplicationFileProvider, ProgressFn};

pub struct SftpApplicationFileProvider {
    connection: Arc<SshSession>,
    root: String,
}

impl SftpApplicationFileProvider {
    pub fn new(connection: Arc<SshSession>, root: String) -> Self {
        Self { connection, root }
    }

    async fn canonical_root(&self) -> AppResult<String> {
        self.connection
            .canonicalize_path(&self.root)
            .await
            .map_err(|_| AppError::InvalidInput("the application's working directory doesn't exist on the remote host".into()))
    }

    /// Sanitizes `relative`, joins it under `root`, then resolves the
    /// result via `REALPATH` (or, for a path that doesn't exist yet,
    /// resolves its parent instead and re-appends the final component
    /// lexically) and checks the outcome is still inside the resolved
    /// root - the actual defense against a symlink under the root pointing
    /// somewhere else, not just against a malicious path *string*.
    async fn resolve(&self, relative: &str) -> AppResult<String> {
        let relative = sanitize_relative_path(relative)?;
        let candidate = if relative.is_empty() { self.root.clone() } else { format!("{}/{}", self.root.trim_end_matches('/'), relative) };
        let canonical_root = self.canonical_root().await?;
        let canonical = match self.connection.canonicalize_path(&candidate).await {
            Ok(resolved) => resolved,
            Err(_) => {
                let (parent, name) = split_parent(&candidate)?;
                let canonical_parent = self
                    .connection
                    .canonicalize_path(&parent)
                    .await
                    .map_err(|_| AppError::InvalidInput("the containing directory doesn't exist".into()))?;
                format!("{}/{}", canonical_parent.trim_end_matches('/'), name)
            }
        };
        if !is_within_root(&canonical, &canonical_root) {
            return Err(AppError::InvalidInput("path escapes the application directory".into()));
        }
        Ok(canonical)
    }

    fn delete_resolved<'a>(&'a self, path: &'a str) -> Pin<Box<dyn Future<Output = AppResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let stat = self.connection.symlink_metadata(path).await?;
            if stat.is_dir && !stat.is_symlink {
                for entry in self.connection.list_directory(path).await? {
                    self.delete_resolved(&entry.path).await?;
                }
                self.connection.remove_dir(path).await
            } else {
                // A file, or a symlink (removes the link itself, not its
                // target - same semantics `rm`/`unlink` have).
                self.connection.remove_file(path).await
            }
        })
    }

    /// **Known limitation**: unlike `LocalApplicationFileProvider::copy`
    /// (which uses `create_dir_all`, tolerant of an already-existing
    /// destination), this expects `to` not to exist yet - SFTP has no
    /// idempotent "create if missing" directory operation to reach for, and
    /// distinguishing "already exists" from any other `MKDIR` failure isn't
    /// reliable through this crate's own string-wrapped errors. The
    /// frontend's own "Copy" action always proposes a fresh, non-colliding
    /// destination name, so this doesn't come up in the normal flow.
    fn copy_resolved<'a>(&'a self, from: &'a str, to: &'a str) -> Pin<Box<dyn Future<Output = AppResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let stat = self.connection.symlink_metadata(from).await?;
            // Same reasoning as `LocalApplicationFileProvider`: `read_file`
            // follows a symlink, so copying one would pull whatever it
            // points at - possibly outside the Application's directory -
            // into the copy as a real file. Skipped rather than recreated.
            if stat.is_symlink {
                log::warn!("skipping the symlink {from} while copying - a copy must not reach outside its source");
                return Ok(());
            }
            if stat.is_dir {
                self.connection.create_directory(to).await?;
                for entry in self.connection.list_directory(from).await? {
                    let child_to = format!("{}/{}", to.trim_end_matches('/'), entry.name);
                    self.copy_resolved(&entry.path, &child_to).await?;
                }
                Ok(())
            } else {
                let contents = self.connection.read_file(from).await?;
                self.connection.write_file(to, &contents).await
            }
        })
    }
}

fn split_parent(path: &str) -> AppResult<(String, String)> {
    match path.rsplit_once('/') {
        Some((parent, name)) if !name.is_empty() => Ok((if parent.is_empty() { "/".to_string() } else { parent.to_string() }, name.to_string())),
        _ => Err(AppError::InvalidInput("invalid path".into())),
    }
}

/// The script `fetch_url` runs: the shared `fetch_to`, into a temporary file
/// beside the target, renamed over it only once complete. Prints the size.
fn build_fetch_script(resolved: &str, url: &str) -> String {
    format!(
        r#"{function}target={target}
url={url}
if [ -d "$target" ]; then echo "'$target' is a directory" >&2; exit 4; fi
tmp=$(mktemp -- "$(dirname -- "$target")/.vibessh-fetch.XXXXXX") || exit 1
if fetch_to "$url" "$tmp"; then
    chmod 0644 -- "$tmp"
    mv -f -- "$tmp" "$target" || {{ rm -f -- "$tmp"; exit 1; }}
    wc -c < "$target"
else
    rc=$?
    rm -f -- "$tmp"
    exit "$rc"
fi
"#,
        function = crate::files::url_fetch::fetch_function(),
        target = crate::ssh::command::quote(resolved),
        url = crate::ssh::command::quote(url),
    )
}

#[async_trait::async_trait]
impl ApplicationFileProvider for SftpApplicationFileProvider {
    async fn list_directory(&self, path: &str) -> AppResult<Vec<RemoteFileEntry>> {
        let dir = self.resolve(path).await?;
        let canonical_root = self.canonical_root().await?;
        let mut entries = self.connection.list_directory(&dir).await?;
        for entry in &mut entries {
            entry.path = relativize(&entry.path, &canonical_root);
        }
        Ok(entries)
    }

    async fn metadata(&self, path: &str) -> AppResult<RemoteFileEntry> {
        let resolved = self.resolve(path).await?;
        let canonical_root = self.canonical_root().await?;
        let mut entry = self.connection.symlink_metadata(&resolved).await?;
        entry.path = relativize(&entry.path, &canonical_root);
        Ok(entry)
    }

    async fn read_file(&self, path: &str) -> AppResult<Vec<u8>> {
        let resolved = self.resolve(path).await?;
        self.connection.read_file(&resolved).await
    }

    async fn read_file_range(&self, path: &str, offset: u64, len: usize) -> AppResult<Vec<u8>> {
        let resolved = self.resolve(path).await?;
        self.connection.read_file_range(&resolved, offset, len).await
    }

    async fn write_file(&self, path: &str, contents: &[u8]) -> AppResult<()> {
        let resolved = self.resolve(path).await?;
        self.connection.write_file(&resolved, contents).await
    }

    async fn create_directory(&self, path: &str) -> AppResult<()> {
        let resolved = self.resolve(path).await?;
        self.connection.create_directory(&resolved).await
    }

    async fn delete(&self, path: &str) -> AppResult<()> {
        let resolved = self.resolve(path).await?;
        self.delete_resolved(&resolved).await
    }

    async fn rename(&self, from: &str, to: &str) -> AppResult<()> {
        let from_resolved = self.resolve(from).await?;
        let to_resolved = self.resolve(to).await?;
        self.connection.rename(&from_resolved, &to_resolved).await
    }

    async fn copy(&self, from: &str, to: &str) -> AppResult<()> {
        let from_resolved = self.resolve(from).await?;
        let to_resolved = self.resolve(to).await?;
        self.copy_resolved(&from_resolved, &to_resolved).await
    }

    /// As the connecting admin, the same identity SFTP writes as - so the
    /// file ends up owned exactly like an uploaded one.
    async fn fetch_url(&self, path: &str, url: &str) -> AppResult<u64> {
        let resolved = self.resolve(path).await?;
        let script = build_fetch_script(&resolved, url);
        let output = self.connection.execute_command(&script).await?;
        if output.exit_code != 0 {
            return Err(AppError::Connection(crate::files::url_fetch::describe_failure(output.exit_code, &output.stderr)));
        }
        Ok(output.stdout.trim().parse().unwrap_or(0))
    }

    async fn set_permissions(&self, path: &str, mode: u32) -> AppResult<()> {
        let resolved = self.resolve(path).await?;
        self.connection.set_permissions(&resolved, mode).await
    }

    async fn download_file(&self, path: &str, local_dest: &Path, on_progress: ProgressFn<'_>) -> AppResult<()> {
        let resolved = self.resolve(path).await?;
        self.connection.download_file_with_progress(&resolved, local_dest, on_progress).await
    }

    async fn upload_file(&self, local_src: &Path, path: &str, on_progress: ProgressFn<'_>) -> AppResult<()> {
        let resolved = self.resolve(path).await?;
        self.connection.upload_file_with_progress(local_src, &resolved, on_progress).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_parent_splits_on_the_last_slash() {
        assert_eq!(split_parent("/srv/app/plugins/x.jar").unwrap(), ("/srv/app/plugins".to_string(), "x.jar".to_string()));
        assert_eq!(split_parent("/srv/x.jar").unwrap(), ("/srv".to_string(), "x.jar".to_string()));
        assert_eq!(split_parent("/x.jar").unwrap(), ("/".to_string(), "x.jar".to_string()));
    }

    #[test]
    fn split_parent_rejects_a_path_with_no_slash_or_a_trailing_slash() {
        assert!(split_parent("x.jar").is_err());
        assert!(split_parent("/srv/app/").is_err());
    }
}
