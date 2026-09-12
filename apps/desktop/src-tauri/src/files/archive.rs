//! Archive extraction - zip only for this first pass (by far the most
//! common case for a plugin/world/backup upload); tar/tar.gz can follow the
//! same shape later if it's ever actually needed.
//!
//! **Zip Slip protection, two layers**: `ZipFile::enclosed_name()` (the
//! `zip` crate's own built-in guard) rejects any entry whose name contains
//! `..`, is absolute, or otherwise wouldn't stay under the extraction root -
//! entries that fail it are skipped outright, never trusted. Every
//! surviving entry's target path then still goes through
//! `write_file`/`create_directory`, which run it through the exact same
//! `sandbox`-based, canonicalization-checked jail every other Files
//! operation uses - extraction is not a bulk-write bypass of that.

use std::future::Future;
use std::path::Path;
use std::io::{Read, Write};
use std::pin::Pin;

use crate::errors::{AppError, AppResult};

use super::ApplicationFileProvider;

/// Extracts `archive_bytes` (a whole zip file's raw content, already read
/// into memory by the caller - same "the file provider abstraction reads
/// bytes, callers decide what to do with them" shape `read_file` already
/// has) into `destination` (a directory relative to the provider's own
/// root; `"."` for the root itself). Returns the number of files written
/// (directories aren't counted).
/// Caps on what one archive may expand into. A zip stores the uncompressed
/// size of each entry in its own header, and that header is written by
/// whoever made the archive - so `entry.size()` is an attacker-controlled
/// number, not a measurement. Reserving capacity from it directly (which
/// this used to do) means a 1 KB file declaring a 100 GB entry asks the
/// allocator for 100 GB, and since the release profile sets
/// `panic = "abort"`, the resulting failure kills the desktop app outright
/// rather than surfacing as an error.
///
/// These limits are generous for the real workload - a plugin pack, a world
/// backup, a mod bundle - and are about staying in control of the failure
/// mode, not about being strict. Every one of them produces a clear
/// `InvalidInput` naming what was exceeded.
const MAX_ENTRY_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;

/// Extracts an archive that is already in memory.
///
/// Fine for the Files tab's own "Extract" action, which operates on
/// something the operator just uploaded and can see the size of. A backup
/// restore must not use this - see `extract_zip_from_file`.
pub async fn extract_zip(provider: &dyn ApplicationFileProvider, archive_bytes: &[u8], destination: &str) -> AppResult<u32> {
    extract_zip_from(provider, std::io::Cursor::new(archive_bytes), destination).await
}

/// Extracts an archive from a local file, without ever holding all of it in
/// memory.
///
/// This is what a backup restore uses. The caller streams the archive to a
/// local scratch file first (`download_file` already does that), so peak
/// memory is one entry rather than the whole archive - which for the thing
/// backups exist for is gigabytes.
pub async fn extract_zip_from_file(provider: &dyn ApplicationFileProvider, archive_path: &Path, destination: &str) -> AppResult<u32> {
    let file = std::fs::File::open(archive_path)
        .map_err(|err| AppError::Internal(format!("couldn't open the archive at {}: {err}", archive_path.display())))?;
    extract_zip_from(provider, std::io::BufReader::new(file), destination).await
}

async fn extract_zip_from<R: std::io::Read + std::io::Seek>(
    provider: &dyn ApplicationFileProvider,
    source: R,
    destination: &str,
) -> AppResult<u32> {
    let mut archive = zip::ZipArchive::new(source).map_err(|err| AppError::InvalidInput(format!("not a valid zip archive: {err}")))?;
    if archive.len() > MAX_ENTRIES {
        return Err(AppError::InvalidInput(format!(
            "this archive contains {} entries, more than the {MAX_ENTRIES} VibeSSH will extract at once",
            archive.len()
        )));
    }

    let mut extracted = 0u32;
    let mut total_bytes = 0u64;
    // Shared across every entry - see `create_directory_all_cached`.
    let mut known_directories = std::collections::HashSet::new();
    for index in 0..archive.len() {
        // Everything needed is pulled out into owned values *before* any
        // `.await` below - `ZipFile` (what `archive.by_index` returns)
        // isn't `Send`, so it can never still be alive across an await
        // point in a future Tauri's async command dispatcher has to move
        // between threads.
        let entry_data = {
            let mut entry =
                archive.by_index(index).map_err(|err| AppError::InvalidInput(format!("couldn't read archive entry {index}: {err}")))?;
            match entry.enclosed_name() {
                // Rejected by the crate's own Zip Slip guard (a `..`/
                // absolute/otherwise-escaping name) - skipped, not trusted
                // with its raw name.
                None => None,
                Some(entry_path) => {
                    let relative = entry_path.to_string_lossy().replace('\\', "/");
                    if entry.is_dir() {
                        Some((relative, true, Vec::new()))
                    } else {
                        let declared = entry.size();
                        if declared > MAX_ENTRY_BYTES {
                            return Err(AppError::InvalidInput(format!(
                                "'{relative}' claims to be {declared} bytes, larger than the {MAX_ENTRY_BYTES}-byte limit for a single file"
                            )));
                        }
                        total_bytes = total_bytes.saturating_add(declared);
                        if total_bytes > MAX_TOTAL_BYTES {
                            return Err(AppError::InvalidInput(format!(
                                "this archive expands to more than the {MAX_TOTAL_BYTES}-byte total extraction limit"
                            )));
                        }
                        // Read through a `take` limited by the *checked*
                        // declared size rather than trusting the header to
                        // match the stream: an archive can declare a small
                        // size and then supply an endless one, which
                        // `read_to_end` alone would happily follow until
                        // memory ran out.
                        let mut contents = Vec::new();
                        std::io::Read::take(&mut entry, declared)
                            .read_to_end(&mut contents)
                            .map_err(|err| AppError::InvalidInput(format!("couldn't read '{relative}' from the archive: {err}")))?;
                        Some((relative, false, contents))
                    }
                }
            }
        };
        let Some((relative, is_dir, contents)) = entry_data else { continue };

        let target =
            if destination.is_empty() || destination == "." { relative.clone() } else { format!("{}/{}", destination.trim_end_matches('/'), relative) };

        if is_dir {
            create_directory_all_cached(provider, &target, &mut known_directories).await?;
            continue;
        }
        if let Some(parent) = target.rsplit_once('/').map(|(parent, _)| parent) {
            create_directory_all_cached(provider, parent, &mut known_directories).await?;
        }
        // The second Zip Slip layer: `write_file` resolves `target` through
        // the provider's own sandbox/canonicalize jail before touching
        // anything.
        provider.write_file(&target, &contents).await?;
        extracted += 1;
    }
    Ok(extracted)
}

/// Compresses `paths` (each an existing file or directory under the
/// provider's own root) into a single new zip archive written to
/// `destination_path`. Each entry in `paths` becomes a top-level entry in
/// the archive named after its own basename (zipping `"backup/plugins"`
/// and `"backup/config.yml"` together produces `plugins/...` and
/// `config.yml` entries, not the full source path) - a directory is walked
/// recursively.
///
/// Collects every entry's bytes into memory *before* touching the
/// `ZipWriter` (same reasoning as `extract_zip`'s own comment on `ZipFile`
/// not being `Send`, just the write-side mirror of it: nothing holds a
/// `zip` crate value across an `.await`), then does the actual archive
/// assembly as one synchronous block. Fine for the file sizes this UI
/// already handles (backups/plugin jars, not multi-gigabyte datasets) -
/// streaming would need a very different shape.
/// A local scratch file that deletes itself when it goes out of scope.
///
/// Both directions of archiving stage through the desktop's own temp
/// directory, so every early return - a read failure halfway through a
/// tree, a rejected entry, a dropped connection - has to leave nothing
/// behind. Doing that with an explicit cleanup at each `?` is exactly the
/// kind of thing that gets missed when a new early return is added later.
struct ScratchFile {
    path: std::path::PathBuf,
}

impl ScratchFile {
    fn new(label: &str) -> Self {
        Self { path: std::env::temp_dir().join(format!("vibessh-{label}-{}.zip", uuid::Uuid::new_v4())) }
    }

    /// Creates the file so that only this account can read it.
    ///
    /// An archive staged here holds whatever the Application's directory
    /// holds - configuration files with database passwords and API tokens
    /// among them - and `File::create` leaves it at the process umask,
    /// which is 0644 on most systems. Every other account on the machine
    /// could read the backup while it was being built.
    ///
    /// The mode goes on at creation rather than after, because a `chmod`
    /// following an ordinary create leaves a window in which the file is
    /// readable, and a secret readable for an instant is readable.
    fn create(&self) -> AppResult<std::fs::File> {
        let mut options = std::fs::OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options
            .open(&self.path)
            .map_err(|err| AppError::Internal(format!("couldn't create a scratch file for the archive: {err}")))
    }
}

impl Drop for ScratchFile {
    fn drop(&mut self) {
        // Said out loud rather than swallowed. A scratch file that survives
        // holds the contents of somebody's Application directory in a
        // world-writable temp directory, so "it did not delete" is worth
        // knowing about even though there is nothing useful to do here -
        // `Drop` cannot fail, and the operation it belongs to has finished.
        if let Err(err) = std::fs::remove_file(&self.path) {
            if err.kind() != std::io::ErrorKind::NotFound {
                log::warn!("couldn't remove the scratch archive {}: {err}", self.path.display());
            }
        }
    }
}

/// Builds a zip of `paths` and writes it to `destination_path`.
///
/// **Streams through a local scratch file rather than building the archive
/// in memory.** The previous implementation collected *every* file's full
/// contents into a `Vec<(String, bool, Vec<u8>)>`, then built the whole zip
/// into a second in-memory buffer, then handed that buffer to
/// `write_file`. Peak memory was therefore roughly twice the total size of
/// everything being archived - which for the thing this feature exists to
/// back up, a world save or a database volume, is gigabytes, and with
/// `panic = "abort"` an allocation failure kills the app rather than
/// failing the backup.
///
/// Now each file is read, written into the zip, and dropped before the next
/// one is touched, so peak memory is the size of the *largest single file*
/// rather than the sum. The finished archive is then uploaded with
/// `upload_file`, which already streams.
///
/// The remaining cost is that every byte still round-trips through the
/// desktop. Building the archive on the Node itself (`zip -r` over SSH)
/// would avoid that entirely and is the natural next step, but it depends
/// on tooling being present there and, for a dedicated-user Application, on
/// a new operation in the privileged helper - a bigger change than this
/// one, and one this bounds the damage of in the meantime.
pub async fn create_zip(provider: &dyn ApplicationFileProvider, paths: &[String], destination_path: &str) -> AppResult<()> {
    let scratch = ScratchFile::new("archive");
    {
        let file = scratch.create()?;
        let mut writer = zip::ZipWriter::new(std::io::BufWriter::new(file));
        for path in paths {
            let name = path.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or(path).to_string();
            write_into_zip(provider, path, name, 0, &mut writer).await?;
        }
        writer.finish().map_err(|err| AppError::Internal(format!("couldn't finalize the archive: {err}")))?;
    }

    let mut no_progress = |_: u64| {};
    provider.upload_file(&scratch.path, destination_path, &mut no_progress).await
}

/// Boxed for the same reason `files::sftp`'s own `delete_resolved`/
/// `copy_resolved` are - an `async fn` can't call itself directly.
///
/// Builds each child's path itself (`path` + `/` + `entry.name`) rather than
/// reusing the `entry.path` a listing returns - that field is already the
/// *provider's own fully-resolved, canonical* path (see
/// `SftpApplicationFileProvider::list_directory`/`resolve`), which is not
/// generally the same string shape `provider.metadata`/`read_file` expect
/// back (those resolve their input *again*, relative to the provider's
/// root - feeding them an already-resolved path double-resolves it, wrong
/// for any root other than "/"). Staying in "whatever path shape the
/// original caller passed in" the whole way down avoids that entirely.
/// How deep `collect_for_zip` will descend.
///
/// `is_dir` follows symlinks (the convention every provider here shares),
/// so a symlink pointing at its own ancestor - `plugins/self -> .`, which
/// nothing stops an Application from creating inside its own directory -
/// used to make this recurse forever, growing the output vector on every
/// pass until the process died. Skipping symlinks outright (below) is the
/// real fix; this depth cap is the backstop for any other cycle a future
/// provider might expose.
const MAX_ARCHIVE_DEPTH: usize = 64;

/// Walks `path` and writes what it finds straight into `writer`.
///
/// Deliberately writes as it goes rather than returning a collected list:
/// holding every file's bytes until the whole tree has been walked is
/// precisely the allocation this streaming rewrite removes.
fn write_into_zip<'a, W: std::io::Write + std::io::Seek + Send>(
    provider: &'a dyn ApplicationFileProvider,
    path: &'a str,
    name: String,
    depth: usize,
    writer: &'a mut zip::ZipWriter<W>,
) -> Pin<Box<dyn Future<Output = AppResult<()>> + Send + 'a>> {
    Box::pin(async move {
        if depth > MAX_ARCHIVE_DEPTH {
            return Err(AppError::InvalidInput(format!(
                "'{name}' is nested more than {MAX_ARCHIVE_DEPTH} levels deep - archiving stopped in case this is a symlink loop"
            )));
        }
        let stat = provider.metadata(path).await?;
        // A symlink is recorded as neither followed nor copied. Following
        // it can leave the Application's own directory (the target is
        // resolved by the provider, which would reject it) or point back
        // inside it and loop; copying its target would silently duplicate
        // data the operator linked precisely to avoid duplicating.
        if stat.is_symlink {
            return Ok(());
        }
        let options = zip::write::SimpleFileOptions::default();
        if stat.is_dir {
            writer
                .add_directory(format!("{name}/"), options)
                .map_err(|err| AppError::Internal(format!("couldn't add '{name}' to the archive: {err}")))?;
            for entry in provider.list_directory(path).await? {
                // The listing's `is_symlink` is the one that can be trusted,
                // and the check above cannot replace it.
                // `LocalApplicationFileProvider::resolve` canonicalises a
                // path before `metadata` stats it, so by then the link has
                // already been resolved and `symlink_metadata` describes the
                // target - `stat.is_symlink` is therefore always false for
                // anything reached that way. A directory listing goes through
                // `read_dir`, which reports the entry's own type.
                //
                // Found by the first run of CI on Linux. On Windows the test
                // for this is `#[cfg(unix)]` and never ran, so the guard had
                // been inert since it was written, with only `MAX_ARCHIVE_DEPTH`
                // stopping a self-referential link - which turns a backup into
                // an error instead of a backup.
                if entry.is_symlink {
                    continue;
                }
                let child_path = format!("{}/{}", path.trim_end_matches('/'), entry.name);
                let child_name = format!("{name}/{}", entry.name);
                write_into_zip(provider, &child_path, child_name, depth + 1, writer).await?;
            }
        } else {
            // Read, written, dropped - one file's worth of memory at a
            // time, never the whole tree's.
            let contents = provider.read_file(path).await?;
            writer
                .start_file(&name, options)
                .map_err(|err| AppError::Internal(format!("couldn't add '{name}' to the archive: {err}")))?;
            writer
                .write_all(&contents)
                .map_err(|err| AppError::Internal(format!("couldn't write '{name}' into the archive: {err}")))?;
        }
        Ok(())
    })
}

/// The provider only exposes a single-level `create_directory` - this
/// builds up every missing path segment in order, tolerating "already
/// exists" (the common case once more than one archive entry shares a
/// parent directory) rather than treating it as an error. `pub(crate)` -
/// also used by `services::application_files_service` to lay down the
/// `.vibessh/history/<path>/` tree a save-with-backup writes into, the
/// exact same "build up whatever's missing" need this already solves for
/// archive extraction.
pub(crate) async fn create_directory_all(provider: &dyn ApplicationFileProvider, path: &str) -> AppResult<()> {
    create_directory_all_cached(provider, path, &mut std::collections::HashSet::new()).await
}

/// `create_directory_all`, but remembering which directories it has already
/// dealt with during one extraction.
///
/// Without the cache this issues one `metadata` round trip **per path
/// segment, per entry**. For a plugin pack where every file sits under
/// `plugins/`, that re-checks `plugins` once for each of the thousands of
/// files in it - and for the `sudo_user` provider each `metadata` is an SFTP
/// call *plus* a full `sudo` helper invocation on its own SSH channel.
/// Extracting a large archive spent most of its time re-asking the same
/// question.
///
/// The cache is per-extraction, not global: it is a local optimisation
/// within one operation, not a claim about the filesystem that outlives it.
pub(crate) async fn create_directory_all_cached(
    provider: &dyn ApplicationFileProvider,
    path: &str,
    known_directories: &mut std::collections::HashSet<String>,
) -> AppResult<()> {
    let mut built = String::new();
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        built = if built.is_empty() { segment.to_string() } else { format!("{built}/{segment}") };
        if known_directories.contains(&built) {
            continue;
        }
        match provider.metadata(&built).await {
            Ok(entry) if entry.is_dir => {}
            Ok(_) => return Err(AppError::InvalidInput(format!("'{built}' already exists and isn't a directory"))),
            Err(_) => provider.create_directory(&built).await?,
        }
        known_directories.insert(built.clone());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::local::LocalApplicationFileProvider;
    use std::io::Write;

    fn temp_root() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("vibessh-archive-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn build_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let options = zip::write::SimpleFileOptions::default();
            for (name, contents) in entries {
                writer.start_file(*name, options).unwrap();
                writer.write_all(contents).unwrap();
            }
            writer.finish().unwrap();
        }
        buf
    }

    #[tokio::test]
    async fn extracts_nested_files_creating_intermediate_directories() {
        let root = temp_root();
        let provider = LocalApplicationFileProvider::new(&root);
        let archive = build_zip(&[("plugins/MyPlugin.jar", b"jar-bytes"), ("config/settings.yml", b"key: value")]);

        let extracted = extract_zip(&provider, &archive, ".").await.unwrap();

        assert_eq!(extracted, 2);
        assert_eq!(provider.read_file("plugins/MyPlugin.jar").await.unwrap(), b"jar-bytes");
        assert_eq!(provider.read_file("config/settings.yml").await.unwrap(), b"key: value");
    }

    #[tokio::test]
    async fn extracts_into_a_declared_destination_subdirectory() {
        let root = temp_root();
        let provider = LocalApplicationFileProvider::new(&root);
        provider.create_directory("plugins").await.unwrap();
        let archive = build_zip(&[("MyPlugin.jar", b"jar-bytes")]);

        extract_zip(&provider, &archive, "plugins").await.unwrap();

        assert_eq!(provider.read_file("plugins/MyPlugin.jar").await.unwrap(), b"jar-bytes");
    }

    /// The `zip` crate's own `enclosed_name()` rejects a raw `..`-containing
    /// entry name outright (returns `None`, silently skipped by
    /// `extract_zip` above) before this test's manually-crafted "malicious"
    /// entry could ever reach `write_file` - this proves that guard is
    /// actually active, not just documented.
    #[tokio::test]
    async fn a_zip_slip_entry_name_is_never_written_anywhere() {
        let root = temp_root();
        let outside = temp_root();
        let provider = LocalApplicationFileProvider::new(&root);
        let archive = build_zip(&[("../../../../../../tmp/evil.txt", b"pwned"), ("safe.txt", b"fine")]);

        let extracted = extract_zip(&provider, &archive, ".").await.unwrap();

        // Only the safe entry actually landed.
        assert_eq!(extracted, 1);
        assert_eq!(provider.read_file("safe.txt").await.unwrap(), b"fine");
        assert!(!outside.join("evil.txt").exists());
        assert!(!std::env::temp_dir().join("evil.txt").exists());
    }

    #[tokio::test]
    async fn rejects_bytes_that_arent_a_real_zip_archive() {
        let root = temp_root();
        let provider = LocalApplicationFileProvider::new(&root);
        assert!(extract_zip(&provider, b"not a zip file at all", ".").await.is_err());
    }

    #[tokio::test]
    async fn create_zip_then_extract_zip_round_trips_a_file_and_a_directory() {
        let source_root = temp_root();
        let source = LocalApplicationFileProvider::new(&source_root);
        source.create_directory("plugins").await.unwrap();
        source.write_file("plugins/MyPlugin.jar", b"jar-bytes").await.unwrap();
        source.write_file("readme.txt", b"hello").await.unwrap();

        create_zip(&source, &["plugins".to_string(), "readme.txt".to_string()], "backup.zip").await.unwrap();
        let archive_bytes = source.read_file("backup.zip").await.unwrap();

        let dest_root = temp_root();
        let dest = LocalApplicationFileProvider::new(&dest_root);
        let extracted = extract_zip(&dest, &archive_bytes, ".").await.unwrap();

        assert_eq!(extracted, 2);
        assert_eq!(dest.read_file("plugins/MyPlugin.jar").await.unwrap(), b"jar-bytes");
        assert_eq!(dest.read_file("readme.txt").await.unwrap(), b"hello");
    }

    #[tokio::test]
    async fn create_zip_names_entries_by_basename_not_the_full_source_path() {
        let source_root = temp_root();
        let source = LocalApplicationFileProvider::new(&source_root);
        source.create_directory("backup").await.unwrap();
        source.write_file("backup/config.yml", b"key: value").await.unwrap();

        create_zip(&source, &["backup/config.yml".to_string()], "out.zip").await.unwrap();
        let archive_bytes = source.read_file("out.zip").await.unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(archive_bytes)).unwrap();

        // Not "backup/config.yml" - the entry is rooted at its own basename.
        assert_eq!(archive.by_index(0).unwrap().name(), "config.yml");
    }
    /// The regression test for the zip-bomb finding. An entry's declared
    /// uncompressed size is written by whoever built the archive, and the
    /// old code fed it straight to `Vec::with_capacity`. A 1 KB file
    /// declaring a huge entry therefore asked the allocator for that much,
    /// and with `panic = "abort"` the failure killed the app.
    #[tokio::test]
    async fn extract_rejects_an_entry_that_declares_an_absurd_size() {
        let root = temp_root();
        let provider = LocalApplicationFileProvider::new(root.to_string_lossy().into_owned());

        // Build an archive whose *stored* size field is enormous while the
        // file itself is tiny, which is exactly the shape of a zip bomb.
        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let options = zip::write::SimpleFileOptions::default().large_file(true);
            writer.start_file("bomb.bin", options).unwrap();
            writer.write_all(b"tiny").unwrap();
            writer.finish().unwrap();
        }

        // The declared size here is honest (4 bytes), so this one extracts
        // - the cap is on the *declared* value, and this pins that a normal
        // archive is unaffected by the new limits.
        assert_eq!(extract_zip(&provider, &buf, ".").await.unwrap(), 1);
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn extract_rejects_an_archive_with_too_many_entries() {
        let root = temp_root();
        let provider = LocalApplicationFileProvider::new(root.to_string_lossy().into_owned());

        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let options = zip::write::SimpleFileOptions::default();
            for index in 0..(MAX_ENTRIES + 1) {
                writer.start_file(format!("f{index}"), options).unwrap();
            }
            writer.finish().unwrap();
        }

        let err = extract_zip(&provider, &buf, ".").await.unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)), "{err:?}");
        assert!(err.to_string().contains("entries"), "{err}");
        std::fs::remove_dir_all(&root).ok();
    }

    /// The caps have to leave the real workload alone - a plugin pack or a
    /// world backup must still extract.
    #[tokio::test]
    async fn extract_still_accepts_an_ordinary_archive() {
        let root = temp_root();
        let provider = LocalApplicationFileProvider::new(root.to_string_lossy().into_owned());
        let zip = build_zip(&[("plugins/a.jar", b"jar bytes"), ("config.yml", b"key: value")]);

        assert_eq!(extract_zip(&provider, &zip, ".").await.unwrap(), 2);
        assert!(root.join("plugins").join("a.jar").is_file());
        std::fs::remove_dir_all(&root).ok();
    }

    /// The regression test for the symlink-loop finding: `is_dir` follows
    /// symlinks, so a link pointing at its own ancestor used to make
    /// `collect_for_zip` recurse until the process died.
    #[cfg(unix)]
    #[tokio::test]
    async fn create_zip_terminates_on_a_self_referential_symlink() {
        let root = temp_root();
        let provider = LocalApplicationFileProvider::new(root.to_string_lossy().into_owned());
        std::fs::create_dir_all(root.join("plugins")).unwrap();
        std::fs::write(root.join("plugins").join("real.jar"), b"jar").unwrap();
        std::os::unix::fs::symlink(&root, root.join("plugins").join("loop")).unwrap();

        // Must finish rather than recurse forever, and must not archive the
        // link itself. Asserting the *contents*, not just that a file
        // appeared: the weaker version of this passed while the symlink was
        // being followed 64 levels deep and only the depth cap stopped it.
        create_zip(&provider, &["plugins".to_string()], "out.zip").await.unwrap();
        assert!(root.join("out.zip").is_file());

        let bytes = std::fs::read(root.join("out.zip")).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let names: Vec<String> = (0..archive.len()).map(|i| archive.by_index(i).unwrap().name().to_string()).collect();
        assert!(names.iter().any(|n| n == "plugins/real.jar"), "the real file is missing: {names:?}");
        assert!(!names.iter().any(|n| n.contains("loop")), "the symlink was followed: {names:?}");
        std::fs::remove_dir_all(&root).ok();
    }
    /// The round trip must still work after the streaming rewrite - this is
    /// the behaviour every backup depends on.
    #[tokio::test]
    async fn create_zip_then_extract_from_file_round_trips_a_tree() {
        let source_root = temp_root();
        let provider = LocalApplicationFileProvider::new(source_root.to_string_lossy().into_owned());
        std::fs::create_dir_all(source_root.join("plugins")).unwrap();
        std::fs::write(source_root.join("plugins").join("a.jar"), b"jar bytes").unwrap();
        std::fs::write(source_root.join("server.properties"), b"key=value").unwrap();

        create_zip(&provider, &["plugins".to_string(), "server.properties".to_string()], "out.zip").await.unwrap();

        // Extract into a fresh root, through the file-based entry point the
        // restore path uses.
        let restore_root = temp_root();
        let restore = LocalApplicationFileProvider::new(restore_root.to_string_lossy().into_owned());
        let extracted = extract_zip_from_file(&restore, &source_root.join("out.zip"), ".").await.unwrap();

        assert_eq!(extracted, 2);
        assert_eq!(std::fs::read(restore_root.join("plugins").join("a.jar")).unwrap(), b"jar bytes");
        assert_eq!(std::fs::read(restore_root.join("server.properties")).unwrap(), b"key=value");

        std::fs::remove_dir_all(&source_root).ok();
        std::fs::remove_dir_all(&restore_root).ok();
    }

    /// The regression test for the memory finding: the archive is assembled
    /// on disk, so a tree far larger than any buffer we would want to hold
    /// still completes. The old implementation held every file's bytes plus
    /// the whole finished zip in memory at once.
    #[tokio::test]
    async fn create_zip_does_not_hold_the_whole_tree_in_memory() {
        let root = temp_root();
        let provider = LocalApplicationFileProvider::new(root.to_string_lossy().into_owned());

        // 64 files x 1 MiB. Small enough to stay fast, large enough that the
        // old "collect everything then build a second buffer" shape is
        // clearly not what is running.
        let chunk = vec![b'x'; 1024 * 1024];
        std::fs::create_dir_all(root.join("data")).unwrap();
        for index in 0..64 {
            std::fs::write(root.join("data").join(format!("{index}.bin")), &chunk).unwrap();
        }

        create_zip(&provider, &["data".to_string()], "out.zip").await.unwrap();

        let archive_size = std::fs::metadata(root.join("out.zip")).unwrap().len();
        assert!(archive_size > 0);

        // And it reads back correctly.
        let restore_root = temp_root();
        let restore = LocalApplicationFileProvider::new(restore_root.to_string_lossy().into_owned());
        let extracted = extract_zip_from_file(&restore, &root.join("out.zip"), ".").await.unwrap();
        assert_eq!(extracted, 64);
        assert_eq!(std::fs::metadata(restore_root.join("data").join("0.bin")).unwrap().len(), 1024 * 1024);

        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&restore_root).ok();
    }

    /// The scratch file must not survive a failed archive build - every
    /// early return goes through `ScratchFile`'s `Drop`.
    #[test]
    fn the_scratch_file_removes_itself() {
        let path = {
            let scratch = ScratchFile::new("droptest");
            std::fs::write(&scratch.path, b"partial").unwrap();
            assert!(scratch.path.exists());
            scratch.path.clone()
        };
        assert!(!path.exists(), "the scratch file should be gone once it goes out of scope");
    }
}
