//! `ApplicationFileProvider` - the abstraction behind an Application's own
//! Files tab (docs/architecture/APPLICATIONS_ARCHITECTURE.md's Files section; distinct
//! from the older, host-wide "Node Files" module in `commands::file_commands`,
//! which this deliberately does not touch or extend). Every operation is
//! confined to one Application's own `working_directory` - never the whole
//! host filesystem - via `sandbox::sanitize_relative_path` plus each
//! provider's own canonicalization-based escape check (see `local`/`sftp`).
//!
//! Reuses `vibessh_protocol::RemoteFileEntry` as the listing/metadata shape
//! rather than inventing a parallel type - Node Files and Application Files
//! describe the same kind of thing (name, path, is_dir, is_symlink, size,
//! modified_at, permissions).
//!
//! **Docker never goes through `docker exec`.** An Application's Docker
//! container only ever gets `working_directory` bind-mounted in -
//! Application Files always reads/writes that host directory directly, the
//! same way a non-Docker Application on the same host/location would, never
//! by shelling into the container. This matches the design brief's own
//! instruction ("Docker nie powinien mieć osobnego filesystem UI") and means
//! container filesystem changes need no restart to take effect (unless the
//! application itself needs to reload). A Docker Application that opted
//! into `runtime::docker::DockerConfig::run_as_dedicated_user` does get its
//! own fourth provider (`sudo_user::SudoUserApplicationFileProvider`) - not
//! because Docker itself needs one, but because that Application's files
//! are owned by its own dedicated Linux account (`crate::dedicated_user`),
//! not the connecting SSH admin SFTP already authenticates as, so plain
//! SFTP can no longer read/write them at all.
//!
//! **Agent provider intentionally absent.** The design calls for one
//! eventually (`AgentApplicationFileProvider`, application-scoped requests
//! the Agent itself resolves and sandboxes), but no Agent-side file API
//! exists yet (`runtime::docker`'s own doc comment notes the same "Agent
//! path deferred" decision for process management) - `provider_for` below
//! only ever resolves Local, SFTP, or the sudo-user provider, all reached
//! through the existing admin SSH connection, never a second Agent
//! transport.

pub mod archive;
pub mod local;
pub mod sandbox;
pub mod sftp;
pub mod sudo_user;

use std::path::Path;
use std::sync::Arc;

use vibessh_protocol::RemoteFileEntry;

use crate::errors::{AppError, AppResult};
use crate::models::{Application, ApplicationLocation, RuntimeType};
use crate::ssh::SshSession;

/// Progress callback every streaming transfer reports through - called with
/// the number of bytes transferred *in this chunk*, not a running total, so
/// a caller tracking cumulative bytes/speed just sums what it's given.
/// `+ Send` because `async_trait`'s generated future must be `Send`, and it
/// captures this reference across await points.
pub type ProgressFn<'a> = &'a mut (dyn FnMut(u64) + Send);

#[async_trait::async_trait]
pub trait ApplicationFileProvider: Send + Sync {
    async fn list_directory(&self, path: &str) -> AppResult<Vec<RemoteFileEntry>>;
    async fn metadata(&self, path: &str) -> AppResult<RemoteFileEntry>;
    /// Whole-file read - callers are responsible for checking `metadata`'s
    /// `size` against their own limit first (the file editor does, before
    /// ever calling this - see `commands::application_file_commands`).
    async fn read_file(&self, path: &str) -> AppResult<Vec<u8>>;
    /// Reads at most `len` bytes from `offset`.
    ///
    /// Exists so a file too large to edit can still be *looked at*, a window
    /// at a time, instead of being refused outright. A short result means the
    /// end of the file was reached - it is not an error.
    async fn read_file_range(&self, path: &str, offset: u64, len: usize) -> AppResult<Vec<u8>>;
    /// Create-or-truncate "save" semantics - matches every other writer in
    /// this codebase (`ssh::sftp::write_file`, `LocalFileProvider`-to-be).
    async fn write_file(&self, path: &str, contents: &[u8]) -> AppResult<()>;
    /// Single-level - the caller already knows the immediate parent exists
    /// (this is "New Folder" inside the folder currently open), not a
    /// `mkdir -p`.
    async fn create_directory(&self, path: &str) -> AppResult<()>;
    /// Removes a file, or a directory and everything under it.
    async fn delete(&self, path: &str) -> AppResult<()>;
    /// Covers both "Rename" (same parent, new name) and "Move" (new
    /// parent) - both are the same underlying primitive; the frontend just
    /// constructs a different destination path for each.
    async fn rename(&self, from: &str, to: &str) -> AppResult<()>;
    /// Recursive for a directory. Not a native operation on either backend
    /// (SFTP has none at all; a hardlink/reflink isn't guaranteed available
    /// even locally across filesystems), so this always does a real
    /// read-then-write per file - correctness over speed, since there's no
    /// better primitive to reach for.
    async fn copy(&self, from: &str, to: &str) -> AppResult<()>;
    /// `mode` is a raw POSIX permission value (e.g. `0o755`) - not
    /// meaningful for `LocalApplicationFileProvider` on Windows, which
    /// returns a clear `InvalidInput` rather than silently no-op'ing.
    async fn set_permissions(&self, path: &str, mode: u32) -> AppResult<()>;
    async fn download_file(&self, path: &str, local_dest: &Path, on_progress: ProgressFn<'_>) -> AppResult<()>;
    async fn upload_file(&self, local_src: &Path, path: &str, on_progress: ProgressFn<'_>) -> AppResult<()>;
}

/// `true` only for a Docker Application whose already-rendered
/// `runtime_config` set `runAsDedicatedUser` (Paper/Velocity/GenericJava,
/// via `blueprints::render_java_docker_config` - never
/// `GenericDockerBlueprint`, see that field's own doc comment on
/// `DockerConfig`). Reads the raw JSON directly rather than deserializing
/// the whole `DockerConfig` - `provider_for` below is called for every
/// `RuntimeType`, most of which don't have that shape at all, and a parse
/// failure here isn't this function's problem to surface.
pub(crate) fn wants_dedicated_user(application: &Application, runtime_config: &serde_json::Value) -> bool {
    application.runtime_type == RuntimeType::Docker && runtime_config.get("runAsDedicatedUser").and_then(serde_json::Value::as_bool).unwrap_or(false)
}

/// The one place that picks Local vs SFTP vs the sudo-user provider for a
/// given Application - same "one match site" convention
/// `runtime::runtime_for` already establishes for `ApplicationRuntime`.
/// Location (`Application::location()`, `server_id` alone) decides Local
/// vs. remote; `runtime_config` (see `wants_dedicated_user`) decides which
/// of the two remote providers actually owns this Application's files.
pub fn provider_for(application: &Application, runtime_config: &serde_json::Value, connection: Option<Arc<SshSession>>) -> AppResult<Box<dyn ApplicationFileProvider>> {
    match application.location() {
        ApplicationLocation::Local => Ok(Box::new(local::LocalApplicationFileProvider::new(application.working_directory.clone()))),
        ApplicationLocation::Remote => {
            let connection = connection.ok_or_else(|| AppError::Internal("a remote application's file provider requires a connection".into()))?;
            if wants_dedicated_user(application, runtime_config) {
                let username = crate::dedicated_user::username(application.id);
                Ok(Box::new(sudo_user::SudoUserApplicationFileProvider::new(connection, application.working_directory.clone(), username, application.id)))
            } else {
                Ok(Box::new(sftp::SftpApplicationFileProvider::new(connection, application.working_directory.clone())))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Walking a local folder that somebody dropped onto the window.
//
// Shared because both upload paths need it and neither owns it: an
// Application's files go through `ApplicationFileProvider`, a Node's go
// straight through the SSH session, but the local half of the work - which
// files exist, how deep, what they are called - is identical.
// ---------------------------------------------------------------------------

/// How deep a dropped folder may nest before the walk gives up.
///
/// Not a guess about real projects - a plugins directory is three or four
/// deep - but a stop for a tree that turns out to be unbounded. Without it a
/// pathological layout walks until it runs out of memory, with nothing on
/// screen to say why.
pub const MAX_UPLOAD_DEPTH: usize = 32;

/// One file the walk decided to send, and where it goes.
#[derive(Debug)]
pub struct PlannedFile {
    pub local: std::path::PathBuf,
    pub remote: String,
    pub size: u64,
}

/// Joins a remote path the way the Files tab does, so a directory dropped
/// into the application root lands beside the entries already listed there
/// rather than under a literal `./`.
pub fn join_remote(parent: &str, name: &str) -> String {
    if parent == "." || parent.is_empty() {
        name.to_string()
    } else {
        format!("{}/{name}", parent.trim_end_matches('/'))
    }
}

/// Walks `local_root`, returning the directories to create (parents first)
/// and the files to send.
///
/// **Symlinks are skipped, not followed.** A link pointing back up its own
/// tree makes the walk endless, and one pointing outside it would copy files
/// the user never dropped onto the window - both are worse than a folder that
/// arrives without its links.
pub async fn plan_directory_upload(local_root: &Path, remote_root: &str) -> AppResult<(Vec<String>, Vec<PlannedFile>)> {
    let mut directories = vec![remote_root.to_string()];
    let mut files: Vec<PlannedFile> = Vec::new();
    let mut queue: std::collections::VecDeque<(std::path::PathBuf, String, usize)> =
        std::collections::VecDeque::from([(local_root.to_path_buf(), remote_root.to_string(), 0usize)]);

    while let Some((local_dir, remote_dir, depth)) = queue.pop_front() {
        if depth >= MAX_UPLOAD_DEPTH {
            return Err(AppError::InvalidInput(format!(
                "{} nests deeper than {MAX_UPLOAD_DEPTH} levels - upload it in parts",
                local_root.display()
            )));
        }
        let mut entries = tokio::fs::read_dir(&local_dir)
            .await
            .map_err(|err| AppError::Internal(format!("couldn't read {}: {err}", local_dir.display())))?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|err| AppError::Internal(format!("couldn't read {}: {err}", local_dir.display())))?
        {
            // `file_type` on the entry does not follow the link, which is the
            // whole point - `metadata` would.
            let file_type = entry
                .file_type()
                .await
                .map_err(|err| AppError::Internal(format!("couldn't inspect {}: {err}", entry.path().display())))?;
            if file_type.is_symlink() {
                continue;
            }

            let name = entry.file_name().to_string_lossy().into_owned();
            // These names come off the local filesystem, so they are already
            // single components - checked anyway, because they are about to
            // become a remote path.
            if name.is_empty() || name.contains('/') || name == "." || name == ".." {
                continue;
            }
            let remote = join_remote(&remote_dir, &name);

            if file_type.is_dir() {
                directories.push(remote.clone());
                queue.push_back((entry.path(), remote, depth + 1));
            } else if file_type.is_file() {
                let size = entry
                    .metadata()
                    .await
                    .map_err(|err| AppError::Internal(format!("couldn't measure {}: {err}", entry.path().display())))?
                    .len();
                files.push(PlannedFile { local: entry.path(), remote, size });
            }
        }
    }

    Ok((directories, files))
}

#[cfg(test)]
mod upload_walk_tests {
    use super::*;

    /// A scratch tree, removed when the test finishes.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("vibessh-walk-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn dir(&self, rel: &str) -> std::path::PathBuf {
            let path = self.0.join(rel);
            std::fs::create_dir_all(&path).unwrap();
            path
        }
        fn file(&self, rel: &str, bytes: &[u8]) {
            let path = self.0.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_root_of_dot_does_not_become_a_literal_path_segment() {
        // The Files tab uses "." for an Application's own directory, so a
        // naive join would send everything to "./plugins" rather than
        // "plugins" and create a directory actually called ".".
        assert_eq!(join_remote(".", "plugins"), "plugins");
        assert_eq!(join_remote("", "plugins"), "plugins");
        assert_eq!(join_remote("plugins", "config"), "plugins/config");
        assert_eq!(join_remote("/srv/app/", "world"), "/srv/app/world");
    }

    #[tokio::test]
    async fn the_walk_keeps_the_shape_of_the_tree() {
        let scratch = Scratch::new();
        let root = scratch.dir("bundle");
        scratch.file("bundle/config.yml", b"a");
        scratch.file("bundle/nested/deep/data.bin", b"bb");
        scratch.dir("bundle/empty");

        let (mut directories, mut files) = plan_directory_upload(&root, "remote/bundle").await.unwrap();
        directories.sort();
        files.sort_by(|a, b| a.remote.cmp(&b.remote));

        assert_eq!(
            directories,
            vec![
                "remote/bundle".to_string(),
                "remote/bundle/empty".to_string(),
                "remote/bundle/nested".to_string(),
                "remote/bundle/nested/deep".to_string(),
            ],
            "an empty directory still has to be created - it is part of the shape"
        );
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].remote, "remote/bundle/config.yml");
        assert_eq!(files[0].size, 1);
        assert_eq!(files[1].remote, "remote/bundle/nested/deep/data.bin");
        assert_eq!(files[1].size, 2);
    }

    #[tokio::test]
    async fn a_tree_deeper_than_the_limit_is_refused_rather_than_walked_forever() {
        let scratch = Scratch::new();
        let mut rel = String::from("deep");
        for _ in 0..(MAX_UPLOAD_DEPTH + 2) {
            rel.push_str("/x");
        }
        scratch.file(&format!("{rel}/leaf.txt"), b"x");

        let err = plan_directory_upload(&scratch.0.join("deep"), "remote/deep").await.unwrap_err();
        assert!(
            format!("{err:?}").contains("nests deeper"),
            "expected the depth limit to be the reason, got {err:?}"
        );
    }
}
