//! Getting a Java runtime onto this machine, without asking anybody to.
//!
//! **Why this exists.** A Minecraft server needs a JVM. Requiring one to be
//! installed first is exactly the step this feature set exists to remove -
//! "download a JDK, put it on PATH, come back" is the same kind of homework
//! as "install Docker Desktop, enable WSL2, reboot". So when no suitable Java
//! is already installed, one is fetched here and kept in the app's own data
//! directory, where it belongs to VibeSSH and touches nothing else on the
//! system: no PATH, no registry, no package manager, no administrator.
//!
//! An installed Java is still preferred - `java_service` finds those, and a
//! runtime somebody already trusts is better than a second copy of one.
//!
//! **Where the archive comes from.** Adoptium's Temurin builds, the same
//! source the Docker path already uses through `eclipse-temurin` images. The
//! API redirects to the right archive for the platform, so the URL is built
//! from the platform rather than pinned to a file that would go stale.

use std::path::{Path, PathBuf};

use crate::errors::{AppError, AppResult};

/// Where a downloaded runtime lives, under the app's data directory. One
/// directory per major version, so two Applications on different Java
/// versions do not fight over one.
fn install_dir(data_dir: &Path, major_version: &str) -> PathBuf {
    data_dir.join("java").join(major_version)
}

/// The binary inside an extracted Temurin archive.
fn java_binary(root: &Path) -> PathBuf {
    if cfg!(windows) {
        root.join("bin").join("java.exe")
    } else {
        root.join("bin").join("java")
    }
}

/// Adoptium's own names for this machine, or an error naming what it is.
///
/// Refusing loudly rather than guessing: an archive for the wrong
/// architecture downloads perfectly happily and then fails to execute with a
/// message about a format error, which is a long way from the cause.
fn platform() -> AppResult<(&'static str, &'static str)> {
    let os = match std::env::consts::OS {
        "windows" => "windows",
        "linux" => "linux",
        "macos" => "mac",
        other => return Err(AppError::InvalidInput(format!("no Temurin build is published for {other}"))),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "aarch64",
        other => return Err(AppError::InvalidInput(format!("no Temurin build is published for {other}"))),
    };
    Ok((os, arch))
}

pub fn download_url(major_version: &str, os: &str, arch: &str) -> String {
    // `jre` rather than `jdk`: a server runs, it does not compile, and the
    // JRE is roughly half the download.
    format!("https://api.adoptium.net/v3/binary/latest/{major_version}/ga/{os}/{arch}/jre/hotspot/normal/eclipse")
}

/// The path to a usable `java`, downloading one if this machine has none.
///
/// Idempotent: a runtime already downloaded is returned as it is, so this is
/// safe to call before every start rather than only at creation.
pub async fn ensure_java(data_dir: &Path, major_version: &str) -> AppResult<PathBuf> {
    let root = install_dir(data_dir, major_version);
    if let Some(existing) = find_java_under(&root) {
        return Ok(existing);
    }

    let (os, arch) = platform()?;
    let url = download_url(major_version, os, arch);
    log::info!("downloading a Java {major_version} runtime from {url}");

    let response = reqwest::get(&url)
        .await
        .map_err(|err| AppError::Connection(format!("couldn't reach Adoptium to download Java {major_version}: {err}")))?;
    if !response.status().is_success() {
        return Err(AppError::Connection(format!(
            "Adoptium has no Java {major_version} build for {os}/{arch} (HTTP {})",
            response.status()
        )));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|err| AppError::Connection(format!("the Java {major_version} download didn't finish: {err}")))?;

    // Extracted into a fresh directory rather than over whatever is there: a
    // half-extracted runtime from an interrupted attempt would otherwise be
    // indistinguishable from a complete one.
    let staging = root.with_extension("partial");
    let _ = tokio::fs::remove_dir_all(&staging).await;
    tokio::fs::create_dir_all(&staging)
        .await
        .map_err(|err| AppError::Storage(format!("couldn't create {}: {err}", staging.display())))?;

    extract(&bytes, &staging, os).await?;

    let _ = tokio::fs::remove_dir_all(&root).await;
    tokio::fs::rename(&staging, &root)
        .await
        .map_err(|err| AppError::Storage(format!("couldn't put the Java runtime in place: {err}")))?;

    find_java_under(&root).ok_or_else(|| AppError::Storage("the downloaded Java archive had no bin/java in it".into()))
}

/// Temurin archives contain a single top-level directory whose name carries
/// the exact build (`jdk-21.0.5+11-jre`), so the binary is looked for rather
/// than assumed - the name changes with every release.
fn find_java_under(root: &Path) -> Option<PathBuf> {
    let direct = java_binary(root);
    if direct.is_file() {
        return Some(direct);
    }
    for entry in std::fs::read_dir(root).ok()? {
        let candidate = java_binary(&entry.ok()?.path());
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Windows gets a zip, everything else a tar.gz.
///
/// `tar` is shelled out to rather than adding `tar` + `flate2` as
/// dependencies: it is present on every Linux and macOS this could run on,
/// and it preserves the executable bit on `bin/java`, which is the one thing
/// a naive extraction gets wrong and only discovers at first start.
async fn extract(bytes: &[u8], into: &Path, os: &str) -> AppResult<()> {
    let archive = into.join(if os == "windows" { "temurin.zip" } else { "temurin.tar.gz" });
    tokio::fs::write(&archive, bytes)
        .await
        .map_err(|err| AppError::Storage(format!("couldn't save the Java archive: {err}")))?;

    if os == "windows" {
        let file = std::fs::File::open(&archive).map_err(|err| AppError::Storage(format!("couldn't open the Java archive: {err}")))?;
        let mut zip = zip::ZipArchive::new(file).map_err(|err| AppError::Storage(format!("the Java archive is not readable: {err}")))?;
        zip.extract(into).map_err(|err| AppError::Storage(format!("couldn't unpack the Java archive: {err}")))?;
    } else {
        let status = tokio::process::Command::new("tar")
            .arg("-xzf")
            .arg(&archive)
            .arg("-C")
            .arg(into)
            .status()
            .await
            .map_err(|err| AppError::Storage(format!("couldn't run tar to unpack Java: {err}")))?;
        if !status.success() {
            return Err(AppError::Storage("tar could not unpack the Java archive".into()));
        }
    }

    let _ = tokio::fs::remove_file(&archive).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_url_asks_for_a_jre_for_this_platform() {
        let url = download_url("21", "linux", "x64");

        assert!(url.contains("/21/ga/linux/x64/"), "{url}");
        // A server runs rather than compiles, and the JRE is about half the
        // download - worth asserting, because the JDK URL differs by one word
        // and would work.
        assert!(url.contains("/jre/"), "{url}");
    }

    #[test]
    fn an_unsupported_architecture_is_named_rather_than_guessed() {
        // Not callable directly for another machine, so this asserts the
        // shape of the answer for the one running the tests: either a known
        // pair, or an error naming what was not recognised.
        match platform() {
            Ok((os, arch)) => {
                assert!(["windows", "linux", "mac"].contains(&os), "{os}");
                assert!(["x64", "aarch64"].contains(&arch), "{arch}");
            }
            Err(AppError::InvalidInput(message)) => assert!(message.contains("Temurin"), "{message}"),
            other => panic!("unexpected {other:?}"),
        }
    }

    /// The only test that proves the feature: it downloads about fifty
    /// megabytes, unpacks it, and runs the result. Ignored by default so a
    /// normal `cargo test` neither needs a network nor waits for it - run it
    /// with `cargo test -- --ignored` when this code changes.
    #[tokio::test]
    #[ignore]
    async fn a_real_runtime_is_downloaded_and_actually_runs() {
        let dir = std::env::temp_dir().join(format!("vibessh-java-live-{}", uuid::Uuid::new_v4()));

        let java = ensure_java(&dir, "21").await.expect("the runtime should download");

        let output = tokio::process::Command::new(&java).arg("-version").output().await.expect("it should run");
        let reported = String::from_utf8_lossy(&output.stderr).to_string();
        assert!(reported.contains("21."), "expected a Java 21, got: {reported}");

        // Asked for twice: the second answer must come from disk rather than
        // from another fifty megabytes.
        let again = ensure_java(&dir, "21").await.expect("the second call should reuse it");
        assert_eq!(again, java);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[test]
    fn a_missing_runtime_is_absent_rather_than_an_error() {
        let nowhere = std::env::temp_dir().join(format!("vibessh-java-test-{}", uuid::Uuid::new_v4()));

        assert!(find_java_under(&nowhere).is_none());
    }
}
