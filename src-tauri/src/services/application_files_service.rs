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
//! docs/APPLICATIONS_ARCHITECTURE.md already documents for Applications as
//! a whole. `PERMISSION_*` below exist so the *names* are settled and
//! ready to wire into a real enforcement point once one exists, not as
//! dead ceremony - nothing in this module currently checks them.

use std::path::Path;
use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::files::{self, archive, ApplicationFileProvider};
use crate::models::Application;
use crate::services::ssh_service::get_or_connect;
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

async fn resolve_provider(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
) -> AppResult<(Application, Box<dyn ApplicationFileProvider>)> {
    let detail = app_repo.get(application_id)?.ok_or_else(|| AppError::NotFound(format!("application {application_id}")))?;
    let connection = match detail.application.server_id {
        None => None,
        Some(server_id) => Some(get_or_connect(server_repo, sessions, server_id).await?),
    };
    let provider = files::provider_for(&detail.application, connection)?;
    Ok((detail.application, provider))
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
pub async fn read_file_for_editor(
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    path: &str,
) -> AppResult<Vec<u8>> {
    let (_, provider) = resolve_provider(app_repo, server_repo, sessions, application_id).await?;
    let meta = provider.metadata(path).await?;
    if meta.size > MAX_EDITABLE_FILE_SIZE {
        return Err(AppError::InvalidInput(format!("'{path}' is too large to edit directly ({} bytes) - download it instead", meta.size)));
    }
    provider.read_file(path).await
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
    if backup {
        if let Ok(existing) = provider.read_file(path).await {
            let backup_path = history_entry_path(path, chrono::Utc::now());
            if let Some(history_dir) = backup_path.rsplit_once('/').map(|(dir, _)| dir) {
                let _ = archive::create_directory_all(provider.as_ref(), history_dir).await;
            }
            let _ = provider.write_file(&backup_path, &existing).await;
        }
    }
    atomic_write(provider.as_ref(), path, contents).await
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
    provider.upload_file(local_src, path, &mut reporter).await
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
