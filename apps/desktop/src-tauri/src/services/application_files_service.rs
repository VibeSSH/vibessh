//! Application Files - the per-Application file manager (design brief's
//! "Application Files / SFTP" section), scoped to one Application's own
//! `working_directory` via `files::ApplicationFileProvider`, never the
//! host-wide filesystem the older Node Files module (`commands::file_commands`)
//! exposes. Every function here resolves the right provider
//! (`files::provider_for`) for the target Application first, so callers
//! (Tauri commands) never touch `files::local`/`files::sftp` directly.
//!
//! **Permission strings, not permission enforcement**: the design brief
//! calls for `applications.files.{view,download,upload,create,edit,rename,
//! delete,chmod}` as distinct RBAC permissions, separate from
//! `nodes.files.*` for the older host-wide module. This codebase's only
//! real RBAC is the cloud Team/Role backend (`services::cloud_service`),
//! entirely separate infrastructure from this local, per-device SQLite
//! feature - the same "RBAC hook, not RBAC" gap
//! docs/architecture/APPLICATIONS_ARCHITECTURE.md already documents for Applications as
//! a whole. `PERMISSION_*` below exist so the *names* are settled and
//! ready to wire into a real enforcement point once one exists, not as
//! dead ceremony - nothing in this module currently checks them.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::files::{self, archive, ApplicationFileProvider};
use crate::models::Application;
use crate::services::ssh_service::get_or_connect;
use crate::ssh::SshSession;
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::server_repository::ServerRepository;
use vibessh_protocol::RemoteFileEntry;

/// Matches `FileEditorPanel.tsx`'s own existing limit for the older Node
/// Files editor - kept consistent across both editors rather than
/// inventing a second number for the same kind of decision.
pub const MAX_EDITABLE_FILE_SIZE: u64 = 1024 * 1024;

/// Settled permission-string catalog (see this module's own doc comment
/// for why nothing enforces these yet).
// Nothing enforces these yet, by design (see the module doc comment) - the
// catalog is settled ahead of the permission system that will read it, so
// that system does not get to invent a second set of names. Kept compiling
// rather than commented out precisely so it cannot silently drift.
#[allow(dead_code)]
pub mod permissions {
    pub const VIEW: &str = "applications.files.view";
    pub const DOWNLOAD: &str = "applications.files.download";
    pub const UPLOAD: &str = "applications.files.upload";
    pub const CREATE: &str = "applications.files.create";
    pub const EDIT: &str = "applications.files.edit";
    pub const RENAME: &str = "applications.files.rename";
    pub const DELETE: &str = "applications.files.delete";
    pub const CHMOD: &str = "applications.files.chmod";
}

/// Which `(SSH session, application)` pairs have already had their
/// dedicated account and file helper verified.
///
/// Both checks are cheap to describe and expensive to run: provisioning
/// executes `getent`/`id` behind `sudo`, and the helper check `sudo cat`s
/// the deployed script to compare it byte for byte. `resolve_provider` runs
/// on *every* file operation, so opening a directory cost two extra command
/// round trips before the listing itself - which is what walking between
/// directories felt like.
///
/// Keyed on the session rather than the server: a dropped and reconnected
/// session is a new session, so a Node that rebooted, or whose connection
/// was replaced, is verified again without anyone having to remember to
/// invalidate anything. Ids are monotonic and never reused, so a remembered
/// pair can never come to mean a different connection.
///
/// The set is bounded rather than pruned. Entries for dead sessions are two
/// integers each and stop being consulted the moment their session is gone;
/// past the cap the whole thing is dropped, whose only cost is one more
/// verification per live session.
static FILE_ACCESS_VERIFIED: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<(u64, Uuid)>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

const FILE_ACCESS_VERIFIED_CAP: usize = 512;

fn needs_readiness_check(session_id: u64, application_id: Uuid) -> bool {
    !FILE_ACCESS_VERIFIED.lock().expect("file access readiness mutex poisoned").contains(&(session_id, application_id))
}

fn mark_ready(session_id: u64, application_id: Uuid) {
    let mut verified = FILE_ACCESS_VERIFIED.lock().expect("file access readiness mutex poisoned");
    if verified.len() >= FILE_ACCESS_VERIFIED_CAP {
        verified.clear();
    }
    verified.insert((session_id, application_id));
}

pub(crate) async fn resolve_provider(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
) -> AppResult<(Application, Box<dyn ApplicationFileProvider>)> {
    let detail = app_repo.get(application_id)?.ok_or_else(|| AppError::NotFound(format!("application {application_id}")))?;
    let connection = match detail.application.server_id {
        None => None,
        Some(server_id) => Some(connect_with_live_sftp(server_repo, sessions, server_id).await?),
    };
    if let Some(connection) = &connection {
        if files::wants_dedicated_user(&detail.application, &detail.runtime_config) && needs_readiness_check(connection.id(), application_id) {
            // Best-effort, proactive: Application Files must work for an
            // Application that opted into a dedicated account but has never
            // actually been started yet (`runtime::docker::start`/`restart`
            // is normally what provisions the account and the helper
            // script) - not only after the user happens to click
            // Start/Restart first. A failure here isn't swallowed silently:
            // `files::provider_for`'s provider will still surface a clear
            // error from the helper itself (e.g. "unknown user") right
            // after this if provisioning genuinely didn't work.
            let username = crate::dedicated_user::username(detail.application.id);
            // Both best-effort: the file operation that follows fails on
            // its own with a message about the file the user actually asked
            // for. But when it does, this is the reason, and without these
            // lines that reason exists nowhere.
            let account = crate::dedicated_user::ensure_provisioned(connection, &username).await;
            if let Err(err) = &account {
                log::warn!("couldn't provision the dedicated account for application {application_id}, file access may fail: {err}");
            }
            let helper = files::sudo_user::ensure_helper_installed(connection).await;
            if let Err(err) = &helper {
                log::warn!("couldn't install the file helper for application {application_id}, file access may fail: {err}");
            }
            // Only a clean pass is remembered. A failure has to be retried
            // on the next operation, because the next operation is where
            // somebody finds out it did not work.
            if account.is_ok() && helper.is_ok() {
                mark_ready(connection.id(), application_id);
            }
        }
    }
    let provider = files::provider_for(&detail.application, &detail.runtime_config, connection)?;
    Ok((detail.application, provider))
}

/// `SshSession` opens its SFTP subsystem channel once and caches it for the
/// session's whole lifetime (`OnceCell`, see `ssh::client`) - unlike
/// `execute_command`, which opens a fresh exec channel every call, so
/// nothing about a cached `SshSession` continuing to run plain commands
/// fine proves its SFTP channel is still alive. A real Node's SFTP
/// subsystem can die on its own (an idle timeout scoped tighter than the
/// main connection's, the remote sshd recycling subsystem channels) while
/// the cached `SshSession` otherwise looks healthy - every file operation
/// would then keep failing with a misleadingly specific error (e.g.
/// `resolve()`'s own "the containing directory doesn't exist", from a
/// `REALPATH` call that's actually failing because the channel is dead, not
/// because the directory is missing) for as long as this app runs, since
/// nothing ever resets that `OnceCell`. A cheap `REALPATH "."` here is the
/// same probe `resolve()` would make anyway as its first real SFTP call -
/// this just makes it early enough to still recover: on failure, drop the
/// whole cached `SshSession` (not just its SFTP channel, which has no reset
/// of its own) and reconnect fresh, same "dead session, drop and retry
/// once" recovery `ssh_service::execute_command` already does for plain
/// commands.
/// Deliberately *not* `ssh_service::retry_on_connection_failure`, unlike the
/// four blocks that were folded into it: this is a liveness probe, not a
/// retry. It runs a cheap `REALPATH "."` to find out whether the cached
/// session is dead *before* handing it to a caller, because the failure it
/// prevents is not an error the caller could retry - it is `resolve()`
/// caching a wrong answer in a `OnceCell` nothing ever resets. The shared
/// helper retries an operation that already failed; this one makes sure the
/// operation never runs against a dead session in the first place.
/// How long a successful probe is trusted for.
///
/// The probe exists to catch an SFTP channel that died on its own while the
/// connection around it looks healthy - an idle timeout scoped tighter than
/// the main session's, or an sshd recycling subsystem channels. Those are
/// things that happen to an *idle* channel, so re-probing a channel that
/// answered a second ago catches nothing and costs a round trip on every
/// file operation. Walking through directories now probes once; coming back
/// to the browser after a while probes again, which is when it can actually
/// have died.
const SFTP_PROBE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

static SFTP_LAST_PROBE: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<u64, std::time::Instant>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn probe_is_still_fresh(session_id: u64) -> bool {
    SFTP_LAST_PROBE
        .lock()
        .expect("sftp probe mutex poisoned")
        .get(&session_id)
        .is_some_and(|at| at.elapsed() < SFTP_PROBE_INTERVAL)
}

fn record_probe(session_id: u64) {
    let mut probes = SFTP_LAST_PROBE.lock().expect("sftp probe mutex poisoned");
    // Same bound, same reasoning, as `FILE_ACCESS_VERIFIED`.
    if probes.len() >= FILE_ACCESS_VERIFIED_CAP {
        probes.clear();
    }
    probes.insert(session_id, std::time::Instant::now());
}

async fn connect_with_live_sftp(server_repo: &ServerRepository, sessions: &SshSessionManager, server_id: Uuid) -> AppResult<Arc<SshSession>> {
    let connection = get_or_connect(server_repo, sessions, server_id).await?;
    if probe_is_still_fresh(connection.id()) {
        return Ok(connection);
    }
    if connection.canonicalize_path(".").await.is_ok() {
        record_probe(connection.id());
        return Ok(connection);
    }
    sessions.remove(server_id).await;
    let reconnected = get_or_connect(server_repo, sessions, server_id).await?;
    // A fresh session's channel has just been negotiated; the next
    // operation does not need to ask again.
    record_probe(reconnected.id());
    Ok(reconnected)
}

pub async fn list_directory(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    path: &str,
) -> AppResult<Vec<RemoteFileEntry>> {
    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;
    let mut entries = provider.list_directory(path).await?;
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(entries)
}

pub async fn get_metadata(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    path: &str,
) -> AppResult<RemoteFileEntry> {
    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;
    provider.metadata(path).await
}

/// Enforces `MAX_EDITABLE_FILE_SIZE` server-side too - the frontend already
/// checks a file's size before ever offering to open it in the editor, but
/// this is the actual trust boundary; a stale or buggy frontend check isn't
/// one.
/// One window of a file, plus what the caller needs to ask for the next one.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileWindow {
    pub bytes: Vec<u8>,
    /// The file's size right now, so the caller can show how far through it
    /// is and know when it has reached the end.
    pub total_size: u64,
    /// Where the next window starts. Equal to `total_size` at the end.
    pub next_offset: u64,
}

/// Reads part of a file, for looking at one too large to load whole.
///
/// **What the caller must not do with the result.** This is a window, not the
/// file. Writing a partially loaded buffer back would truncate everything
/// after it - for a log or a world data file that is silent, total loss. The
/// editor keeps a partly loaded file read-only for exactly this reason, and
/// nothing here can enforce that on its behalf.
pub async fn read_file_window(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    path: &str,
    offset: u64,
    len: usize,
) -> AppResult<FileWindow> {
    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;
    let total_size = provider.metadata(path).await?.size;
    let bytes = provider.read_file_range(path, offset, len).await?;
    Ok(FileWindow { next_offset: offset + bytes.len() as u64, bytes, total_size })
}

pub async fn read_file_for_editor(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    path: &str,
) -> AppResult<Vec<u8>> {
    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;
    provider.read_file_capped(path, MAX_EDITABLE_FILE_SIZE).await
}

/// Plain create-or-truncate write, no atomicity/backup - used for "New
/// File" and anything else that isn't specifically the editor's Save
/// action (see `save_file` for that one).
pub async fn write_file(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    path: &str,
    contents: &[u8],
) -> AppResult<()> {
    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;
    provider.write_file(path, contents).await
}

/// The editor's own Save action - `backup` (design brief section 114,
/// "Backup before save... tylko przy save", never on every keystroke)
/// copies the file's current content into
/// `.vibessh/history/<path>/<timestamp>` first, best-effort: a backup
/// failure is not allowed to block the actual save the user asked for.
/// The save itself goes through `atomic_write` (temp file + rename)
/// wherever the provider can actually do that atomically - see that
/// function's own doc comment for the one case it can't.
pub async fn save_file(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    path: &str,
    contents: &[u8],
    backup: bool,
) -> AppResult<()> {
    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;
    let backup_path = backup.then(|| history_entry_path(path, chrono::Utc::now()));
    // Backup and write together, in one round trip, where the provider can.
    if let Some(result) = provider.save_in_one_call(path, contents, backup_path.as_deref()).await {
        return result;
    }
    if let Some(backup_path) = &backup_path {
        // Best-effort, as it always was - but said, not swallowed: a save
        // that went through without its history copy is worth a line in
        // the log when somebody later looks for the version before it.
        if let Err(err) = back_up_in_place(provider.as_ref(), path, backup_path).await {
            log::warn!("couldn't keep a history copy of {path} before saving it: {err}");
        }
    }
    atomic_write_via_temp(provider.as_ref(), path, contents).await
}

/// Copies a file to its history entry on the Node itself.
///
/// It used to read the old contents down to this machine and write them
/// back up as the backup. For an Application with its own account every
/// read and write is a staged `sudo` helper round trip of several SSH
/// channels each, so the backup alone made a save of a 19-line config take
/// seconds; a copy is one helper call, a single `cp` on the Node.
///
/// The history directory is only created when the copy fails for want of
/// it - the first save of a file - instead of being walked segment by
/// segment on every save.
async fn back_up_in_place(provider: &dyn ApplicationFileProvider, path: &str, backup_path: &str) -> AppResult<()> {
    let first = match provider.copy(path, backup_path).await {
        Ok(()) => return Ok(()),
        Err(err) => err,
    };
    let Some(history_dir) = backup_path.rsplit_once('/').map(|(dir, _)| dir) else {
        return Err(first);
    };
    archive::create_directory_all(provider, history_dir).await?;
    provider.copy(path, backup_path).await
}

pub async fn create_directory(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    path: &str,
) -> AppResult<()> {
    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;
    provider.create_directory(path).await
}

pub async fn delete(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    path: &str,
) -> AppResult<()> {
    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;
    provider.delete(path).await
}

/// Covers both "Rename" and "Move" - see `ApplicationFileProvider::rename`'s
/// own doc comment for why there's only one backend primitive for both.
pub async fn rename(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    from: &str,
    to: &str,
) -> AppResult<()> {
    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;
    provider.rename(from, to).await
}

pub async fn copy(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    from: &str,
    to: &str,
) -> AppResult<()> {
    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;
    provider.copy(from, to).await
}

/// Downloads a link into `path`, relative to the Application's root - the
/// Files tab's "Download from a link". The Node fetches it itself; see
/// `files::url_fetch` for which links are accepted and why.
pub async fn fetch_url(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    path: &str,
    url: &str,
) -> AppResult<u64> {
    let url = files::url_fetch::validate_fetch_url(url)?;
    if path.trim().is_empty() || path.ends_with('/') {
        return Err(AppError::InvalidInput("give the downloaded file a name".into()));
    }
    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;
    provider.fetch_url(path, &url).await
}

/// Compresses `paths` into a new `.zip` at `destination_path`, all relative
/// to the Application's root.
///
/// The same `files::archive::create_zip` the Node-wide Files page uses, over
/// this Application's own provider - so a dedicated-account Application's
/// files are read, and the archive written, as that account, inside its
/// root, exactly like every other file operation here.
pub async fn compress(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    paths: &[String],
    destination_path: &str,
) -> AppResult<()> {
    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;
    archive::create_zip(provider.as_ref(), paths, destination_path).await
}

pub async fn set_permissions(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    path: &str,
    mode: u32,
) -> AppResult<()> {
    if mode > 0o7777 {
        return Err(AppError::InvalidInput("not a valid POSIX permission value".into()));
    }
    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;
    provider.set_permissions(path, mode).await
}

/// `on_progress(transferred, total)` - already throttled (see
/// `throttled_reporter`) to a UI-friendly rate, not called once per raw
/// I/O chunk.
pub async fn download_file(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    path: &str,
    local_dest: &Path,
    on_progress: impl FnMut(u64, u64) + Send + 'static,
) -> AppResult<()> {
    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;
    let total = provider.metadata(path).await?.size;
    let mut reporter = throttled_reporter(total, on_progress);
    provider.download_file(path, local_dest, &mut reporter).await
}

/// Uploads a whole local directory into `path`, keeping its shape.
///
/// The progress this reports is the sum over every file, not per file, so a
/// folder of four hundred small files shows one bar that fills once rather
/// than four hundred that each fill instantly.
///
/// Unlike the single-file upload there is no `.vibessh-partial` staging here:
/// that trick protects one known destination path, and a half-finished
/// directory has no single path to protect. An interrupted folder upload
/// therefore leaves what it managed to send, which is visible in the Files
/// tab rather than hidden.
pub async fn upload_directory(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    local_src: &Path,
    path: &str,
    on_progress: impl FnMut(u64, u64) + Send + 'static,
) -> AppResult<()> {
    let name = local_src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| AppError::InvalidInput(format!("{} has no name to copy", local_src.display())))?;
    let remote_root = crate::files::join_remote(path, &name);

    let (directories, files) = crate::files::plan_directory_upload(local_src, &remote_root).await?;
    let total: u64 = files.iter().map(|f| f.size).sum();

    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;

    // Parents before children: the walk is breadth-first, so the order it
    // produced is already correct. An existing directory is not an error -
    // dropping a folder onto one that is already there should merge into it,
    // the way copying a directory does everywhere else.
    for directory in &directories {
        if let Err(err) = provider.create_directory(directory).await {
            if provider.metadata(directory).await.is_err() {
                return Err(err);
            }
        }
    }

    let mut reporter = throttled_reporter(total, on_progress);
    for file in &files {
        provider.upload_file(&file.local, &file.remote, &mut reporter).await?;
    }
    // A folder of nothing but empty files still deserves a finished bar.
    if total == 0 {
        reporter(0);
    }
    Ok(())
}

pub async fn upload_file(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    local_src: &Path,
    path: &str,
    on_progress: impl FnMut(u64, u64) + Send + 'static,
) -> AppResult<()> {
    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;
    let total = tokio::fs::metadata(local_src)
        .await
        .map_err(|err| AppError::Internal(format!("couldn't read {}: {err}", local_src.display())))?
        .len();
    let mut reporter = throttled_reporter(total, on_progress);

    // Uploads land on a `.vibessh-partial` sibling and only move to the real
    // path once the transfer has completed.
    //
    // Without this, an upload that failed partway - a dropped connection, a
    // cancelled transfer - left a *truncated file at the real path*,
    // silently replacing whatever was there. For the files this feature is
    // actually used on (a server jar, a world archive, a config read at
    // boot) that is a broken Application with nothing to indicate why.
    //
    // Two limits worth stating plainly rather than implying they are
    // handled. Cancellation aborts the task, so the future is dropped
    // rather than returning `Err` - the real path is still protected, but
    // the `.vibessh-partial` fragment is left behind and shows up in the
    // Files tab. And an overwrite has to unlink the destination before the
    // rename (SFTP's rename fails when the target exists), so there is a
    // brief window where the path does not exist at all. That is a much
    // better failure than a truncated file: a missing file is obvious
    // immediately, a half-written jar is not.
    let partial = format!("{path}.vibessh-partial");
    match provider.upload_file(local_src, &partial, &mut reporter).await {
        Ok(()) => {
            // Only when something is actually there - `delete` on a missing
            // path is an error on some providers.
            if provider.metadata(path).await.is_ok() {
                provider.delete(path).await?;
            }
            provider.rename(&partial, path).await
        }
        Err(err) => {
            // Best-effort, and logged rather than discarded: a leftover
            // fragment is not fatal but it is confusing.
            if let Err(cleanup_err) = provider.delete(&partial).await {
                log::warn!("couldn't remove the partial upload at '{partial}': {cleanup_err}");
            }
            Err(err)
        }
    }
}

/// Extracts an archive that's already sitting in the Application's own
/// sandbox (uploaded the normal way, streamed straight to disk/SFTP - see
/// `upload_file`) into `destination`. Deliberately takes a path, not raw
/// bytes: reading the archive happens server-side, through the exact same
/// provider `write_file` extracts into, so a multi-hundred-MB world backup
/// never needs to round-trip through this JS boundary a second time just
/// to be extracted. See `files::archive::extract_zip` for the two-layer
/// Zip Slip protection this goes through.
pub async fn extract_archive(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    source_path: &str,
    destination: &str,
) -> AppResult<u32> {
    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;
    let archive_bytes = provider.read_file(source_path).await?;
    archive::extract_zip(provider.as_ref(), &archive_bytes, destination).await
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileHistoryVersion {
    /// The filesystem-safe timestamp this version was saved under (also
    /// what `restore_file_history` expects back) - not necessarily a
    /// literal display string, the frontend formats it.
    pub timestamp: String,
    pub size: u64,
}

pub async fn list_file_history(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    path: &str,
) -> AppResult<Vec<FileHistoryVersion>> {
    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;
    let history_dir = history_dir_for(path);
    match provider.list_directory(&history_dir).await {
        Ok(entries) => {
            let mut versions: Vec<FileHistoryVersion> =
                entries.into_iter().filter(|e| !e.is_dir).map(|e| FileHistoryVersion { timestamp: e.name, size: e.size }).collect();
            versions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
            Ok(versions)
        }
        // No history directory yet just means no backups exist - not an
        // error the UI needs to show.
        Err(_) => Ok(vec![]),
    }
}

pub async fn restore_file_history(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    path: &str,
    timestamp: &str,
) -> AppResult<()> {
    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;
    let backup_path = format!("{}/{timestamp}", history_dir_for(path));
    let contents = provider.read_file(&backup_path).await?;
    atomic_write(provider.as_ref(), path, &contents).await
}

/// Deletes every saved backup version for `path` in one go - the same
/// `delete()` a normal file/folder removal already goes through (its own
/// doc comment: "a file, or a directory and everything under it"), just
/// pointed at the history directory instead of the file itself. A missing
/// history directory (nothing was ever backed up) is treated as already
/// having nothing to clear, not an error - matching `list_file_history`'s
/// own "no history dir yet just means no backups exist" stance.
pub async fn clear_file_history(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    path: &str,
) -> AppResult<()> {
    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;
    let history_dir = history_dir_for(path);
    if provider.metadata(&history_dir).await.is_err() {
        return Ok(());
    }
    provider.delete(&history_dir).await
}

fn history_dir_for(path: &str) -> String {
    format!(".vibessh/history/{}", path.trim_start_matches('/'))
}

fn history_entry_path(path: &str, timestamp: chrono::DateTime<chrono::Utc>) -> String {
    // Colons aren't valid in a Windows filename, hence dashes instead of
    // RFC3339's own `:` - still lexically sortable, which is all
    // `list_file_history`'s own newest-first sort needs.
    format!("{}/{}", history_dir_for(path), timestamp.format("%Y-%m-%dT%H-%M-%SZ"))
}

/// Write-to-a-temp-name-then-rename - atomic on every filesystem/SFTP
/// server that supports an overwriting rename. **Documented fallback**:
/// classic SFTPv3 (still what plenty of servers speak) has no atomic
/// "rename, replacing an existing destination" - when that rename fails,
/// this falls back to writing `path` directly (not atomic in that case,
/// but still correct), and always cleans up the temp file either way.
async fn atomic_write(provider: &dyn ApplicationFileProvider, path: &str, contents: &[u8]) -> AppResult<()> {
    if let Some(result) = provider.save_in_one_call(path, contents, None).await {
        return result;
    }
    atomic_write_via_temp(provider, path, contents).await
}

/// `atomic_write` without asking the provider for its one-call save first -
/// for a caller that has just been told it has none.
async fn atomic_write_via_temp(provider: &dyn ApplicationFileProvider, path: &str, contents: &[u8]) -> AppResult<()> {
    let temp_path = format!("{path}.vibessh-tmp-{}", Uuid::new_v4());
    provider.write_file(&temp_path, contents).await?;
    match provider.rename(&temp_path, path).await {
        Ok(()) => Ok(()),
        Err(_) => {
            let result = provider.write_file(path, contents).await;
            let _ = provider.delete(&temp_path).await;
            result
        }
    }
}

/// Wraps a provider's raw per-chunk `FnMut(u64)` (bytes just transferred)
/// into a `(transferred_total, total)` callback emitted at most a few
/// times a second - a caller (the Tauri command layer, turning this into
/// an event) shouldn't have to also implement its own rate limiting.
fn throttled_reporter(total: u64, mut on_progress: impl FnMut(u64, u64) + Send + 'static) -> impl FnMut(u64) + Send + 'static {
    const MIN_INTERVAL: Duration = Duration::from_millis(150);
    let mut transferred = 0u64;
    let mut last_emit = Instant::now() - MIN_INTERVAL;
    move |delta: u64| {
        transferred += delta;
        let now = Instant::now();
        if transferred >= total || now.duration_since(last_emit) >= MIN_INTERVAL {
            on_progress(transferred, total);
            last_emit = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CreateApplicationInput, RuntimeType};
    use crate::state::SshSessionManager;
    use crate::storage::server_repository::ServerRepository;

    fn temp_setup() -> (ApplicationRepository, ServerRepository, SshSessionManager, Uuid) {
        let db_path = std::env::temp_dir().join(format!("vibessh-application-files-service-test-{}.sqlite3", Uuid::new_v4()));
        let app_repo = ApplicationRepository::open(&db_path).unwrap();
        let server_repo = ServerRepository::open(&db_path).unwrap();

        let working_directory = std::env::temp_dir().join(format!("vibessh-application-files-service-workdir-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&working_directory).unwrap();

        let detail = app_repo
            .create(&CreateApplicationInput {
                server_id: None,
                name: "Files Test App".to_string(),
                description: None,
                blueprint_id: "generic".to_string(),
                blueprint_version: 1,
                runtime_type: RuntimeType::LocalProcess,
                working_directory: working_directory.to_string_lossy().into_owned(),
                environment: vec![],
                ports: vec![],
                runtime_config: serde_json::json!({ "command": "sh", "args": [] }),
                metadata: serde_json::json!({}),
            })
            .unwrap();

        (app_repo, server_repo, SshSessionManager::new(), detail.application.id)
    }

    #[tokio::test]
    async fn write_then_read_file_round_trips_through_the_service_layer() {
        let (app_repo, server_repo, sessions, application_id) = temp_setup();

        write_file(&app_repo, &server_repo, &sessions, application_id, "server.properties", b"motd=hello").await.unwrap();
        let contents = read_file_for_editor(&app_repo, &server_repo, &sessions, application_id, "server.properties").await.unwrap();

        assert_eq!(contents, b"motd=hello");
    }

    #[tokio::test]
    async fn read_file_for_editor_rejects_a_file_over_the_size_limit() {
        let (app_repo, server_repo, sessions, application_id) = temp_setup();
        let big = vec![0u8; (MAX_EDITABLE_FILE_SIZE + 1) as usize];
        write_file(&app_repo, &server_repo, &sessions, application_id, "world.dat", &big).await.unwrap();

        let result = read_file_for_editor(&app_repo, &server_repo, &sessions, application_id, "world.dat").await;

        assert!(matches!(result, Err(AppError::InvalidInput(_))));
    }

    #[tokio::test]
    async fn save_file_with_backup_creates_a_history_entry_and_restore_recovers_the_old_content() {
        let (app_repo, server_repo, sessions, application_id) = temp_setup();
        write_file(&app_repo, &server_repo, &sessions, application_id, "config.yml", b"version: 1").await.unwrap();

        // No history yet - nothing has gone through `save_file` with
        // `backup: true` yet.
        assert!(list_file_history(&app_repo, &server_repo, &sessions, application_id, "config.yml").await.unwrap().is_empty());

        save_file(&app_repo, &server_repo, &sessions, application_id, "config.yml", b"version: 2", true).await.unwrap();
        let history = list_file_history(&app_repo, &server_repo, &sessions, application_id, "config.yml").await.unwrap();
        assert_eq!(history.len(), 1, "the pre-save content ('version: 1') should have been backed up once");

        // The live file reflects the new save.
        assert_eq!(read_file_for_editor(&app_repo, &server_repo, &sessions, application_id, "config.yml").await.unwrap(), b"version: 2");

        // Restoring the backup brings the old content back.
        restore_file_history(&app_repo, &server_repo, &sessions, application_id, "config.yml", &history[0].timestamp).await.unwrap();
        assert_eq!(read_file_for_editor(&app_repo, &server_repo, &sessions, application_id, "config.yml").await.unwrap(), b"version: 1");
    }

    #[tokio::test]
    async fn clear_file_history_removes_every_saved_version() {
        let (app_repo, server_repo, sessions, application_id) = temp_setup();
        write_file(&app_repo, &server_repo, &sessions, application_id, "config.yml", b"version: 1").await.unwrap();
        save_file(&app_repo, &server_repo, &sessions, application_id, "config.yml", b"version: 2", true).await.unwrap();
        // Consecutive saves within the same second collapse into one backup
        // entry (`history_entry_path`'s timestamp is second-granularity) -
        // this only needs "at least one exists", not an exact count.
        assert!(!list_file_history(&app_repo, &server_repo, &sessions, application_id, "config.yml").await.unwrap().is_empty());

        clear_file_history(&app_repo, &server_repo, &sessions, application_id, "config.yml").await.unwrap();

        assert!(list_file_history(&app_repo, &server_repo, &sessions, application_id, "config.yml").await.unwrap().is_empty());
        // The live file itself is untouched - only the backups are gone.
        assert_eq!(read_file_for_editor(&app_repo, &server_repo, &sessions, application_id, "config.yml").await.unwrap(), b"version: 2");
    }

    #[tokio::test]
    async fn clear_file_history_on_a_file_with_no_backups_yet_is_a_no_op_not_an_error() {
        let (app_repo, server_repo, sessions, application_id) = temp_setup();
        write_file(&app_repo, &server_repo, &sessions, application_id, "config.yml", b"version: 1").await.unwrap();

        clear_file_history(&app_repo, &server_repo, &sessions, application_id, "config.yml").await.unwrap();
    }

    #[tokio::test]
    async fn save_file_without_backup_creates_no_history_entry() {
        let (app_repo, server_repo, sessions, application_id) = temp_setup();
        write_file(&app_repo, &server_repo, &sessions, application_id, "config.yml", b"version: 1").await.unwrap();

        save_file(&app_repo, &server_repo, &sessions, application_id, "config.yml", b"version: 2", false).await.unwrap();

        assert!(list_file_history(&app_repo, &server_repo, &sessions, application_id, "config.yml").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_dotdot_path_is_rejected_end_to_end_through_the_service_layer() {
        let (app_repo, server_repo, sessions, application_id) = temp_setup();
        let result = read_file_for_editor(&app_repo, &server_repo, &sessions, application_id, "../../../etc/passwd").await;
        assert!(matches!(result, Err(AppError::InvalidInput(_))));
    }

    #[tokio::test]
    async fn delete_then_list_directory_no_longer_shows_the_removed_file() {
        let (app_repo, server_repo, sessions, application_id) = temp_setup();
        write_file(&app_repo, &server_repo, &sessions, application_id, "temp.txt", b"x").await.unwrap();

        delete(&app_repo, &server_repo, &sessions, application_id, "temp.txt").await.unwrap();

        let listing = list_directory(&app_repo, &server_repo, &sessions, application_id, ".").await.unwrap();
        assert!(listing.iter().all(|e| e.name != "temp.txt"));
    }

    #[tokio::test]
    async fn set_permissions_rejects_a_mode_outside_the_valid_posix_range() {
        let (app_repo, server_repo, sessions, application_id) = temp_setup();
        write_file(&app_repo, &server_repo, &sessions, application_id, "run.sh", b"#!/bin/sh").await.unwrap();

        let result = set_permissions(&app_repo, &server_repo, &sessions, application_id, "run.sh", 0o10000).await;

        assert!(matches!(result, Err(AppError::InvalidInput(_))));
    }

    #[tokio::test]
    async fn extract_archive_reads_an_already_uploaded_zip_from_the_sandbox_and_extracts_it() {
        let (app_repo, server_repo, sessions, application_id) = temp_setup();

        let mut zip_bytes = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_bytes));
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file("plugins/MyPlugin.jar", options).unwrap();
            std::io::Write::write_all(&mut writer, b"jar-bytes").unwrap();
            writer.finish().unwrap();
        }
        write_file(&app_repo, &server_repo, &sessions, application_id, "upload.zip", &zip_bytes).await.unwrap();

        let extracted = extract_archive(&app_repo, &server_repo, &sessions, application_id, "upload.zip", ".").await.unwrap();

        assert_eq!(extracted, 1);
        assert_eq!(
            read_file_for_editor(&app_repo, &server_repo, &sessions, application_id, "plugins/MyPlugin.jar").await.unwrap(),
            b"jar-bytes"
        );
    }
}

#[cfg(test)]
mod readiness_tests {
    use super::*;

    /// A distinct session id per test, so tests sharing the process-wide
    /// caches cannot see each other's entries.
    fn fresh_session_id() -> u64 {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1_000_000);
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    #[test]
    fn a_session_is_verified_once_and_then_remembered() {
        let session = fresh_session_id();
        let application = Uuid::new_v4();
        assert!(needs_readiness_check(session, application));
        mark_ready(session, application);
        assert!(!needs_readiness_check(session, application));
    }

    // The account is per application: verifying one says nothing about the
    // next one on the same Node.
    #[test]
    fn another_application_on_the_same_session_is_still_checked() {
        let session = fresh_session_id();
        let application = Uuid::new_v4();
        mark_ready(session, application);
        assert!(needs_readiness_check(session, Uuid::new_v4()));
    }

    // The reason this is keyed on the session rather than the server: a
    // reconnect must re-verify, because the Node may have rebooted.
    #[test]
    fn a_reconnect_is_verified_again() {
        let application = Uuid::new_v4();
        let first = fresh_session_id();
        mark_ready(first, application);
        assert!(needs_readiness_check(fresh_session_id(), application));
    }

    #[test]
    fn a_fresh_session_has_no_probe_to_trust() {
        let session = fresh_session_id();
        assert!(!probe_is_still_fresh(session));
        record_probe(session);
        assert!(probe_is_still_fresh(session));
    }
}
