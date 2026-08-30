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

use std::io::Read;

use crate::errors::{AppError, AppResult};

use super::ApplicationFileProvider;

/// Extracts `archive_bytes` (a whole zip file's raw content, already read
/// into memory by the caller - same "the file provider abstraction reads
/// bytes, callers decide what to do with them" shape `read_file` already
/// has) into `destination` (a directory relative to the provider's own
/// root; `"."` for the root itself). Returns the number of files written
/// (directories aren't counted).
pub async fn extract_zip(provider: &dyn ApplicationFileProvider, archive_bytes: &[u8], destination: &str) -> AppResult<u32> {
    let cursor = std::io::Cursor::new(archive_bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|err| AppError::InvalidInput(format!("not a valid zip archive: {err}")))?;

    let mut extracted = 0u32;
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
                        let mut contents = Vec::with_capacity(entry.size() as usize);
                        entry
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
            create_directory_all(provider, &target).await?;
            continue;
        }
        if let Some(parent) = target.rsplit_once('/').map(|(parent, _)| parent) {
            create_directory_all(provider, parent).await?;
        }
        // The second Zip Slip layer: `write_file` resolves `target` through
        // the provider's own sandbox/canonicalize jail before touching
        // anything.
        provider.write_file(&target, &contents).await?;
        extracted += 1;
    }
    Ok(extracted)
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
    let mut built = String::new();
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        built = if built.is_empty() { segment.to_string() } else { format!("{built}/{segment}") };
        match provider.metadata(&built).await {
            Ok(entry) if entry.is_dir => continue,
            Ok(_) => return Err(AppError::InvalidInput(format!("'{built}' already exists and isn't a directory"))),
            Err(_) => provider.create_directory(&built).await?,
        }
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
}
