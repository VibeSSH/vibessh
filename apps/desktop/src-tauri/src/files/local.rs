//! `LocalApplicationFileProvider` - `ApplicationFileProvider` for a Local
//! Application (`server_id: None`), jailed to `working_directory` via
//! `std::path::Path::canonicalize` (which itself resolves any symlink
//! along the way) rather than a hand-rolled traversal check.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use chrono::{DateTime, Utc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use vibessh_protocol::RemoteFileEntry;

use crate::errors::{AppError, AppResult};

use super::sandbox::{is_within_root, relativize, sanitize_relative_path};
use super::{ApplicationFileProvider, ProgressFn};

const TRANSFER_CHUNK_SIZE: usize = 256 * 1024;

pub struct LocalApplicationFileProvider {
    root: PathBuf,
}

impl LocalApplicationFileProvider {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn canonical_root(&self) -> AppResult<PathBuf> {
        self.root
            .canonicalize()
            .map_err(|err| AppError::InvalidInput(format!("the application's working directory doesn't exist: {err}")))
    }

    /// Sanitizes `relative`, joins it under `root`, then canonicalizes the
    /// result (or, for a path that doesn't exist yet - a new file/folder,
    /// a rename's destination - canonicalizes its parent and re-appends the
    /// final component) and checks the outcome is still inside the
    /// canonicalized root. This is what actually stops a symlink planted
    /// under the root from pointing anywhere else - `sanitize_relative_path`
    /// alone only catches an obviously malicious *string*.
    fn resolve(&self, relative: &str) -> AppResult<PathBuf> {
        let relative = sanitize_relative_path(relative)?;
        let candidate = self.root.join(&relative);
        let canonical_root = self.canonical_root()?;
        let canonical = canonicalize_existing_or_parent(&candidate)?;
        if !is_within_root(&path_key(&canonical), &path_key(&canonical_root)) {
            return Err(AppError::InvalidInput("path escapes the application directory".into()));
        }
        Ok(canonical)
    }
}

fn canonicalize_existing_or_parent(candidate: &Path) -> AppResult<PathBuf> {
    if candidate.exists() {
        return candidate.canonicalize().map_err(|err| AppError::Internal(format!("couldn't resolve {}: {err}", candidate.display())));
    }
    let parent = candidate.parent().ok_or_else(|| AppError::InvalidInput("invalid path".into()))?;
    let file_name = candidate.file_name().ok_or_else(|| AppError::InvalidInput("invalid path".into()))?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|err| AppError::InvalidInput(format!("the containing directory doesn't exist: {err}")))?;
    Ok(canonical_parent.join(file_name))
}

/// Forward-slash-normalized for `is_within_root`'s own `/`-boundary check,
/// which would otherwise be fooled by Windows' native `\` separator.
fn path_key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(unix)]
fn permissions_of(metadata: Option<&std::fs::Metadata>) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    metadata.map(|m| m.permissions().mode() & 0o7777)
}

#[cfg(not(unix))]
fn permissions_of(_metadata: Option<&std::fs::Metadata>) -> Option<u32> {
    None
}

async fn entry_to_remote_file_entry(entry: &tokio::fs::DirEntry, canonical_root: &Path) -> RemoteFileEntry {
    let name = entry.file_name().to_string_lossy().into_owned();
    let path = relativize(&path_key(&entry.path()), &path_key(canonical_root));
    let is_symlink = entry.file_type().await.map(|t| t.is_symlink()).unwrap_or(false);
    // `DirEntry::metadata` follows a symlink - if that fails (a broken
    // link, or a target this process can't stat), this falls back to
    // reporting it as a non-directory, zero-size, unknown-modified entry
    // rather than failing the whole listing over one bad link.
    let metadata = entry.metadata().await.ok();
    RemoteFileEntry {
        name,
        path,
        is_dir: metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false),
        is_symlink,
        size: metadata.as_ref().map(|m| m.len()).unwrap_or(0),
        modified_at: metadata.as_ref().and_then(|m| m.modified().ok()).map(DateTime::<Utc>::from),
        permissions: permissions_of(metadata.as_ref()),
    }
}

/// Copies a tree, refusing to follow a symlink out of it.
///
/// `symlink_metadata` keeps this from *descending* into a symlinked
/// directory, which looks like the whole answer and is not: the file branch
/// then called `tokio::fs::copy`, which follows the link and copies whatever
/// it points at. A link named `notes.txt` pointing at `/etc/shadow` produced
/// a real `notes.txt` full of `/etc/shadow` inside the destination - a file
/// from outside the Application's directory, now inside it, where the file
/// browser will happily show it.
///
/// Links are skipped rather than recreated. Recreating them would carry the
/// escape into the copy, and a relative link that resolves inside the tree
/// is not worth the machinery to tell apart from one that does not.
fn copy_recursive<'a>(from: &'a Path, to: &'a Path) -> Pin<Box<dyn Future<Output = AppResult<()>> + Send + 'a>> {
    Box::pin(async move {
        let metadata = tokio::fs::symlink_metadata(from)
            .await
            .map_err(|err| AppError::Internal(format!("couldn't stat {}: {err}", from.display())))?;
        if metadata.file_type().is_symlink() {
            log::warn!("skipping the symlink {} while copying - a copy must not reach outside its source", from.display());
            return Ok(());
        }
        if metadata.is_dir() {
            tokio::fs::create_dir_all(to)
                .await
                .map_err(|err| AppError::Internal(format!("couldn't create {}: {err}", to.display())))?;
            let mut entries = tokio::fs::read_dir(from)
                .await
                .map_err(|err| AppError::Internal(format!("couldn't list {}: {err}", from.display())))?;
            while let Some(entry) = entries
                .next_entry()
                .await
                .map_err(|err| AppError::Internal(format!("couldn't read a directory entry: {err}")))?
            {
                let child_to = to.join(entry.file_name());
                copy_recursive(&entry.path(), &child_to).await?;
            }
            Ok(())
        } else {
            tokio::fs::copy(from, to)
                .await
                .map(|_| ())
                .map_err(|err| AppError::Internal(format!("couldn't copy to {}: {err}", to.display())))
        }
    })
}

async fn copy_with_progress(from: &Path, to: &Path, on_progress: ProgressFn<'_>) -> AppResult<()> {
    let mut source = tokio::fs::File::open(from).await.map_err(|err| AppError::Internal(format!("couldn't open {}: {err}", from.display())))?;
    let mut dest = tokio::fs::File::create(to).await.map_err(|err| AppError::Internal(format!("couldn't create {}: {err}", to.display())))?;
    let mut buf = vec![0u8; TRANSFER_CHUNK_SIZE];
    loop {
        let read = source.read(&mut buf).await.map_err(|err| AppError::Internal(format!("couldn't read {}: {err}", from.display())))?;
        if read == 0 {
            break;
        }
        dest.write_all(&buf[..read]).await.map_err(|err| AppError::Internal(format!("couldn't write {}: {err}", to.display())))?;
        on_progress(read as u64);
    }
    dest.flush().await.map_err(|err| AppError::Internal(format!("couldn't finish writing {}: {err}", to.display())))
}

#[async_trait::async_trait]
impl ApplicationFileProvider for LocalApplicationFileProvider {
    async fn list_directory(&self, path: &str) -> AppResult<Vec<RemoteFileEntry>> {
        let dir = self.resolve(path)?;
        let canonical_root = self.canonical_root()?;
        let mut read_dir = tokio::fs::read_dir(&dir).await.map_err(|err| AppError::Internal(format!("couldn't list {}: {err}", dir.display())))?;
        let mut entries = Vec::new();
        while let Some(entry) = read_dir.next_entry().await.map_err(|err| AppError::Internal(format!("couldn't read a directory entry: {err}")))? {
            entries.push(entry_to_remote_file_entry(&entry, &canonical_root).await);
        }
        Ok(entries)
    }

    async fn metadata(&self, path: &str) -> AppResult<RemoteFileEntry> {
        let resolved = self.resolve(path)?;
        let canonical_root = self.canonical_root()?;
        let name = resolved.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let symlink_meta = tokio::fs::symlink_metadata(&resolved)
            .await
            .map_err(|err| AppError::NotFound(format!("{path}: {err}")))?;
        let full_meta = tokio::fs::metadata(&resolved).await.ok();
        let is_dir = full_meta.as_ref().map(|m| m.is_dir()).unwrap_or(symlink_meta.is_dir());
        let size = full_meta.as_ref().map(|m| m.len()).unwrap_or(symlink_meta.len());
        let effective = full_meta.as_ref().unwrap_or(&symlink_meta);
        Ok(RemoteFileEntry {
            name,
            path: relativize(&path_key(&resolved), &path_key(&canonical_root)),
            is_dir,
            is_symlink: symlink_meta.is_symlink(),
            size,
            modified_at: effective.modified().ok().map(DateTime::<Utc>::from),
            permissions: permissions_of(Some(effective)),
        })
    }

    async fn read_file(&self, path: &str) -> AppResult<Vec<u8>> {
        let resolved = self.resolve(path)?;
        tokio::fs::read(&resolved).await.map_err(|err| AppError::Internal(format!("couldn't read {}: {err}", resolved.display())))
    }

    async fn read_file_range(&self, path: &str, offset: u64, len: usize) -> AppResult<Vec<u8>> {
        use tokio::io::{AsyncReadExt, AsyncSeekExt};
        let resolved = self.resolve(path)?;
        let mut file = tokio::fs::File::open(&resolved)
            .await
            .map_err(|err| AppError::Internal(format!("couldn't open {}: {err}", resolved.display())))?;
        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(|err| AppError::Internal(format!("couldn't seek in {}: {err}", resolved.display())))?;
        let mut buf = vec![0u8; len];
        let mut filled = 0usize;
        while filled < len {
            let read = file
                .read(&mut buf[filled..])
                .await
                .map_err(|err| AppError::Internal(format!("couldn't read {}: {err}", resolved.display())))?;
            if read == 0 {
                break;
            }
            filled += read;
        }
        buf.truncate(filled);
        Ok(buf)
    }

    async fn write_file(&self, path: &str, contents: &[u8]) -> AppResult<()> {
        let resolved = self.resolve(path)?;
        tokio::fs::write(&resolved, contents).await.map_err(|err| AppError::Internal(format!("couldn't write {}: {err}", resolved.display())))
    }

    async fn create_directory(&self, path: &str) -> AppResult<()> {
        let resolved = self.resolve(path)?;
        tokio::fs::create_dir(&resolved)
            .await
            .map_err(|err| AppError::Internal(format!("couldn't create directory {}: {err}", resolved.display())))
    }

    async fn delete(&self, path: &str) -> AppResult<()> {
        let resolved = self.resolve(path)?;
        let metadata = tokio::fs::symlink_metadata(&resolved).await.map_err(|err| AppError::NotFound(format!("{path}: {err}")))?;
        let result = if metadata.is_dir() && !metadata.is_symlink() {
            tokio::fs::remove_dir_all(&resolved).await
        } else {
            // Removes the symlink itself, not whatever it points to, for a
            // symlink entry - same semantics `rm`/`unlink` have.
            tokio::fs::remove_file(&resolved).await
        };
        result.map_err(|err| AppError::Internal(format!("couldn't delete {}: {err}", resolved.display())))
    }

    async fn rename(&self, from: &str, to: &str) -> AppResult<()> {
        let from_resolved = self.resolve(from)?;
        let to_resolved = self.resolve(to)?;
        tokio::fs::rename(&from_resolved, &to_resolved)
            .await
            .map_err(|err| AppError::Internal(format!("couldn't rename to {}: {err}", to_resolved.display())))
    }

    async fn copy(&self, from: &str, to: &str) -> AppResult<()> {
        let from_resolved = self.resolve(from)?;
        let to_resolved = self.resolve(to)?;
        copy_recursive(&from_resolved, &to_resolved).await
    }

    /// The same rules the Node's `curl` follows - http/https redirects only,
    /// five at most, the same size cap - streamed to a temporary file beside
    /// the target and renamed over it once complete.
    async fn fetch_url(&self, path: &str, url: &str) -> AppResult<u64> {
        let target = self.resolve(path)?;
        if target.is_dir() {
            return Err(AppError::InvalidInput(format!("'{path}' is a folder")));
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(5))
            .timeout(std::time::Duration::from_secs(590))
            .build()
            .map_err(|err| AppError::Internal(format!("couldn't start the download: {err}")))?;
        let mut response = client
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|err| AppError::Connection(format!("the download failed: {err}")))?;
        let limit = super::url_fetch::MAX_FETCH_BYTES;
        if response.content_length().is_some_and(|length| length > limit) {
            return Err(AppError::InvalidInput(super::url_fetch::describe_failure(63, "")));
        }
        let parent = target.parent().ok_or_else(|| AppError::InvalidInput("invalid path".into()))?;
        let temporary = parent.join(format!(".vibessh-fetch-{}", uuid::Uuid::new_v4()));
        let result: AppResult<u64> = async {
            let mut file = tokio::fs::File::create(&temporary)
                .await
                .map_err(|err| AppError::Internal(format!("couldn't create {}: {err}", temporary.display())))?;
            let mut written = 0u64;
            while let Some(chunk) = response.chunk().await.map_err(|err| AppError::Connection(format!("the download was interrupted: {err}")))? {
                written += chunk.len() as u64;
                if written > limit {
                    return Err(AppError::InvalidInput(super::url_fetch::describe_failure(63, "")));
                }
                file.write_all(&chunk).await.map_err(|err| AppError::Internal(format!("couldn't write the download: {err}")))?;
            }
            file.flush().await.map_err(|err| AppError::Internal(format!("couldn't write the download: {err}")))?;
            Ok(written)
        }
        .await;
        match result {
            Ok(written) => {
                tokio::fs::rename(&temporary, &target)
                    .await
                    .map_err(|err| AppError::Internal(format!("couldn't put the download in place: {err}")))?;
                Ok(written)
            }
            Err(err) => {
                if let Err(cleanup) = tokio::fs::remove_file(&temporary).await {
                    log::warn!("couldn't remove the partial download {}: {cleanup}", temporary.display());
                }
                Err(err)
            }
        }
    }

    async fn set_permissions(&self, path: &str, mode: u32) -> AppResult<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let resolved = self.resolve(path)?;
            let permissions = std::fs::Permissions::from_mode(mode);
            tokio::fs::set_permissions(&resolved, permissions)
                .await
                .map_err(|err| AppError::Internal(format!("couldn't change permissions on {}: {err}", resolved.display())))
        }
        #[cfg(not(unix))]
        {
            let _ = (path, mode);
            Err(AppError::InvalidInput("this platform has no POSIX file permissions to change".into()))
        }
    }

    async fn download_file(&self, path: &str, local_dest: &Path, on_progress: ProgressFn<'_>) -> AppResult<()> {
        let resolved = self.resolve(path)?;
        copy_with_progress(&resolved, local_dest, on_progress).await
    }

    async fn upload_file(&self, local_src: &Path, path: &str, on_progress: ProgressFn<'_>) -> AppResult<()> {
        let resolved = self.resolve(path)?;
        copy_with_progress(local_src, &resolved, on_progress).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vibessh-local-file-provider-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A copy must not reach outside the directory it is copying.
    ///
    /// `symlink_metadata` stops the walk descending *into* a symlinked
    /// directory, which reads like the whole defence and is not: the file
    /// branch used `tokio::fs::copy`, which follows the link and writes the
    /// target's contents into the destination as a real file. A link named
    /// innocuously and pointing at something outside the Application's
    /// directory therefore smuggled that file's contents in.
    #[cfg(unix)]
    #[tokio::test]
    async fn copying_does_not_follow_a_symlink_out_of_the_tree() {
        let root = temp_root();
        let outside = root.join("outside-secret");
        std::fs::write(&outside, b"a file the copy has no business touching").unwrap();

        let source = root.join("source");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("real.txt"), b"ordinary").unwrap();
        std::os::unix::fs::symlink(&outside, source.join("notes.txt")).unwrap();

        let destination = root.join("copy");
        copy_recursive(&source, &destination).await.unwrap();

        assert!(destination.join("real.txt").exists(), "ordinary files still copy");
        assert!(
            !destination.join("notes.txt").exists(),
            "the symlink was followed and its target copied in - that is the escape this guards against"
        );
    }

    #[tokio::test]
    async fn write_then_read_then_list_round_trips() {
        let root = temp_root();
        let provider = LocalApplicationFileProvider::new(&root);

        provider.write_file("server.properties", b"motd=hello").await.unwrap();
        assert_eq!(provider.read_file("server.properties").await.unwrap(), b"motd=hello");

        let listing = provider.list_directory(".").await.unwrap();
        assert_eq!(listing.len(), 1);
        assert_eq!(listing[0].name, "server.properties");
        assert!(!listing[0].is_dir);
    }

    #[tokio::test]
    async fn create_directory_then_list_shows_it() {
        let root = temp_root();
        let provider = LocalApplicationFileProvider::new(&root);

        provider.create_directory("plugins").await.unwrap();
        let listing = provider.list_directory(".").await.unwrap();
        assert_eq!(listing.len(), 1);
        assert!(listing[0].is_dir);
    }

    #[tokio::test]
    async fn a_dotdot_path_is_rejected_before_touching_the_filesystem() {
        let root = temp_root();
        let provider = LocalApplicationFileProvider::new(&root);
        assert!(provider.read_file("../../../etc/passwd").await.is_err());
        assert!(provider.write_file("../escape.txt", b"x").await.is_err());
    }

    #[tokio::test]
    async fn a_symlink_pointing_outside_the_root_is_blocked_on_access() {
        let root = temp_root();
        let outside = temp_root(); // a different temp dir - definitely outside `root`
        std::fs::write(outside.join("secret.txt"), b"top secret").unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.join("secret.txt"), root.join("link.txt")).unwrap();
        #[cfg(windows)]
        {
            // Creating a symlink on Windows needs SeCreateSymbolicLinkPrivilege
            // (admin, or Developer Mode enabled) - a real environment
            // constraint unrelated to whether the escape-blocking logic
            // below is correct. Skip rather than fail the whole suite on a
            // machine where this specific privilege isn't available; the
            // Unix CI path (and any Windows box with Developer Mode on)
            // still exercises it for real.
            if let Err(err) = std::os::windows::fs::symlink_file(outside.join("secret.txt"), root.join("link.txt")) {
                eprintln!("skipping: this Windows account can't create symlinks ({err}) - enable Developer Mode to run this test for real");
                return;
            }
        }

        let provider = LocalApplicationFileProvider::new(&root);
        // Listing still shows the entry (it exists, and is honestly
        // reported as a symlink) - only actually reading through it is
        // blocked.
        let listing = provider.list_directory(".").await.unwrap();
        assert_eq!(listing.len(), 1);
        assert!(listing[0].is_symlink);

        let result = provider.read_file("link.txt").await;
        assert!(result.is_err(), "reading through an escaping symlink must be blocked");
    }

    #[tokio::test]
    async fn a_listed_entrys_own_path_round_trips_back_into_delete() {
        // Regression test: `list_directory`'s entries used to carry the raw
        // absolute filesystem path, which `resolve` (via
        // `sanitize_relative_path`) then re-joined onto `root` a *second*
        // time - so passing a real listed entry's own `.path` straight back
        // into another provider call (exactly what the frontend does for
        // delete/rename/download/etc) silently pointed at a nonexistent,
        // doubly-nested path instead of the real file.
        let root = temp_root();
        let provider = LocalApplicationFileProvider::new(&root);
        provider.write_file("velocity.toml", b"config").await.unwrap();

        let listing = provider.list_directory(".").await.unwrap();
        let entry = listing.iter().find(|e| e.name == "velocity.toml").unwrap();

        provider.delete(&entry.path).await.unwrap();

        assert!(provider.list_directory(".").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_listed_subdirectorys_own_path_round_trips_back_into_a_further_listing() {
        let root = temp_root();
        let provider = LocalApplicationFileProvider::new(&root);
        provider.create_directory("plugins").await.unwrap();
        provider.write_file("plugins/MyPlugin.jar", b"jar").await.unwrap();

        let top_level = provider.list_directory(".").await.unwrap();
        let plugins_dir = top_level.iter().find(|e| e.name == "plugins").unwrap();
        assert_eq!(plugins_dir.path, "plugins", "a listed entry's path should be root-relative, not the raw absolute filesystem path");

        let nested = provider.list_directory(&plugins_dir.path).await.unwrap();
        assert_eq!(nested.len(), 1);
        assert_eq!(nested[0].name, "MyPlugin.jar");
    }

    #[tokio::test]
    async fn delete_removes_a_file_and_a_whole_directory_tree() {
        let root = temp_root();
        let provider = LocalApplicationFileProvider::new(&root);
        provider.write_file("a.txt", b"x").await.unwrap();
        provider.create_directory("dir").await.unwrap();
        provider.write_file("dir/b.txt", b"y").await.unwrap();

        provider.delete("a.txt").await.unwrap();
        provider.delete("dir").await.unwrap();

        assert!(provider.list_directory(".").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn rename_moves_a_file_to_a_new_name() {
        let root = temp_root();
        let provider = LocalApplicationFileProvider::new(&root);
        provider.write_file("old.txt", b"content").await.unwrap();

        provider.rename("old.txt", "new.txt").await.unwrap();

        assert!(provider.read_file("old.txt").await.is_err());
        assert_eq!(provider.read_file("new.txt").await.unwrap(), b"content");
    }

    #[tokio::test]
    async fn copy_duplicates_a_directory_tree() {
        let root = temp_root();
        let provider = LocalApplicationFileProvider::new(&root);
        provider.create_directory("src").await.unwrap();
        provider.write_file("src/file.txt", b"content").await.unwrap();

        provider.copy("src", "dst").await.unwrap();

        assert_eq!(provider.read_file("src/file.txt").await.unwrap(), b"content");
        assert_eq!(provider.read_file("dst/file.txt").await.unwrap(), b"content");
    }

    #[tokio::test]
    async fn download_then_upload_round_trips_through_progress_callbacks() {
        let root = temp_root();
        let provider = LocalApplicationFileProvider::new(&root);
        provider.write_file("server.jar", &vec![7u8; 600_000]).await.unwrap();

        let mut downloaded_bytes = 0u64;
        let dest = std::env::temp_dir().join(format!("vibessh-download-test-{}.jar", uuid::Uuid::new_v4()));
        provider.download_file("server.jar", &dest, &mut |n| downloaded_bytes += n).await.unwrap();
        assert_eq!(downloaded_bytes, 600_000);
        assert_eq!(std::fs::read(&dest).unwrap().len(), 600_000);

        let mut uploaded_bytes = 0u64;
        provider.upload_file(&dest, "server-copy.jar", &mut |n| uploaded_bytes += n).await.unwrap();
        assert_eq!(uploaded_bytes, 600_000);
        assert_eq!(provider.read_file("server-copy.jar").await.unwrap().len(), 600_000);
    }
}
