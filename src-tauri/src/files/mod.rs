//! `ApplicationFileProvider` - the abstraction behind an Application's own
//! Files tab (docs/APPLICATIONS_ARCHITECTURE.md's Files section; distinct
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
