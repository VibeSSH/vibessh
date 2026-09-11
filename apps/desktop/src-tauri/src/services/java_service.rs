//! Detects real, actually-installed Java runtimes - so the Create
//! Application wizard's "Java binary" field can offer a picker (the user's
//! own reference point: Pterodactyl lets you pick a Java version from a
//! list instead of typing a path) rather than a free-text field nobody but
//! a Java developer would know how to fill in correctly. Local detection
//! scans common install locations on this machine; remote detection runs
//! the equivalent scan over SSH. Both are best-effort: an empty result
//! just means the wizard's free-text fallback stays available, not an
//! error - a host with an unusual Java install layout shouldn't block
//! creating the application, only miss the convenience.

use std::collections::HashSet;

use serde::Serialize;
use uuid::Uuid;

use crate::errors::AppResult;
use crate::services::ssh_service::get_or_connect;
use crate::ssh::SshSession;
use crate::state::SshSessionManager;
use crate::storage::server_repository::ServerRepository;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaInstallation {
    pub path: String,
    /// A human-readable label built from the binary's own `-version`
    /// output (e.g. `"17.0.9"`), never guessed from the install path alone -
    /// directory naming conventions vary too much across vendors/OSes to
    /// trust ("jdk-17", "temurin-17-jdk", "zulu17.44.53-ca-jdk17.0.8-...")
    /// for something shown to the user as the actual version.
    pub label: String,
    /// The major version number (`"21"`, `"8"`, ...) parsed from `label` -
    /// so the wizard can offer a clean "Java 21" / "Java 17" picker (the
    /// user's own reference point: Pterodactyl's Docker-image picker reads
    /// the same way), the same simple style the Minecraft version picker
    /// already has, instead of every individual install's exact patch
    /// version and filesystem path cluttering the list.
    pub major_version: String,
}

/// Java's own versioning split: `"1.8.0_392"`-style strings (Java 8 and
/// earlier) report their major version as the *second* dot-separated
/// component; `"9.x.x"` onward (including `"21.0.9"`, `"25"`) reports it as
/// the first.
fn major_version(label: &str) -> String {
    let mut parts = label.split('.');
    match (parts.next(), parts.next()) {
        (Some("1"), Some(second)) => second.to_string(),
        (Some(first), _) => first.to_string(),
        (None, _) => label.to_string(),
    }
}

/// Keeps only the first installation found for each major version -
/// several JDK vendors installed side by side often share one, and a
/// picker showing "Java 21" three times over (once per vendor) defeats the
/// point of simplifying it in the first place. Order is preserved, so the
/// `$PATH`-found installation (always collected first, in both the local
/// and remote scans) wins over a directory-scan match when both share a
/// major version.
fn dedupe_by_major_version(installations: Vec<JavaInstallation>) -> Vec<JavaInstallation> {
    let mut seen_majors = HashSet::new();
    installations.into_iter().filter(|installation| seen_majors.insert(installation.major_version.clone())).collect()
}

/// `Some(server_id)` detects on that Remote server over SSH; `None`
/// detects on this machine.
pub async fn detect_java_installations(
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Option<Uuid>,
) -> AppResult<Vec<JavaInstallation>> {
    match server_id {
        None => Ok(detect_local_java().await),
        Some(server_id) => {
            let connection = get_or_connect(server_repo, sessions, server_id).await?;
            Ok(detect_remote_java(&connection).await)
        }
    }
}

/// The bare, portable name - not a directory-scan match - so a chosen "on
/// PATH" installation round-trips as the exact same value
/// `GenericJavaBlueprint`'s own `javaBinary` default already uses. Works
/// unmodified on Windows too: `CreateProcess`/`tokio::process::Command`
/// resolve a bare name via `PATH` + `PATHEXT`, the same way a bare `sh -c
/// java` would on Unix.
const JAVA_ON_PATH: &str = "java";

#[cfg(windows)]
const CANDIDATE_ROOTS: &[&str] =
    &["C:\\Program Files\\Java", "C:\\Program Files\\Eclipse Adoptium", "C:\\Program Files\\Zulu", "C:\\Program Files\\Microsoft", "C:\\Program Files\\Amazon Corretto"];
#[cfg(windows)]
const JAVA_BINARY_NAME: &str = "java.exe";

#[cfg(unix)]
const CANDIDATE_ROOTS: &[&str] = &["/usr/lib/jvm", "/usr/java", "/opt/java"];
#[cfg(unix)]
const JAVA_BINARY_NAME: &str = "java";

async fn detect_local_java() -> Vec<JavaInstallation> {
    let mut installations = Vec::new();

    if let Some(label) = probe_java_version(JAVA_ON_PATH).await {
        installations.push(JavaInstallation { path: JAVA_ON_PATH.to_string(), major_version: major_version(&label), label });
    }

    for root in CANDIDATE_ROOTS {
        let Ok(mut entries) = tokio::fs::read_dir(root).await else { continue };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let candidate = entry.path().join("bin").join(JAVA_BINARY_NAME);
            let Some(candidate_str) = candidate.to_str() else { continue };
            if installations.iter().any(|found: &JavaInstallation| found.path == candidate_str) {
                continue;
            }
            if let Some(label) = probe_java_version(candidate_str).await {
                installations.push(JavaInstallation { path: candidate_str.to_string(), major_version: major_version(&label), label });
            }
        }
    }

    dedupe_by_major_version(installations)
}

async fn probe_java_version(binary: &str) -> Option<String> {
    let output = tokio::process::Command::new(binary).arg("-version").output().await.ok()?;
    parse_java_version_output(&String::from_utf8_lossy(&output.stderr)).or_else(|| parse_java_version_output(&String::from_utf8_lossy(&output.stdout)))
}

/// `java -version`'s first line looks like `openjdk version "17.0.9"
/// 2023-10-17` (historically printed to stderr; some
/// distributions/wrappers use stdout instead, so both are checked by the
/// caller) - this extracts the quoted version string.
fn parse_java_version_output(output: &str) -> Option<String> {
    let first_line = output.lines().next()?;
    let start = first_line.find('"')? + 1;
    let rest = &first_line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// One remote shell round trip: probes `java` on `$PATH`, then every
/// `bin/java` under the common per-distro JVM install roots (globs that
/// match nothing expand to a literal, harmless string the `[ -x ... ]`
/// guard filters back out - no `nullglob` dependency). `===JAVA===` is a
/// marker line preceding each found binary's path, then its own
/// `-version` output - the same "delimited sections in one combined
/// command" shape `ssh::monitor`'s `METRICS_COMMAND` already uses.
const REMOTE_DETECT_SCRIPT: &str = r#"for candidate in $(command -v java) /usr/lib/jvm/*/bin/java /usr/java/*/bin/java /opt/java/*/bin/java; do
  [ -x "$candidate" ] || continue
  echo "===JAVA==="
  echo "$candidate"
  "$candidate" -version 2>&1
done"#;

async fn detect_remote_java(connection: &SshSession) -> Vec<JavaInstallation> {
    let Ok(output) = connection.execute_command(REMOTE_DETECT_SCRIPT).await else {
        return Vec::new();
    };
    parse_remote_detect_output(&output.stdout)
}

fn parse_remote_detect_output(stdout: &str) -> Vec<JavaInstallation> {
    let mut installations = Vec::new();
    let mut seen = HashSet::new();

    for block in stdout.split("===JAVA===").skip(1) {
        let mut lines = block.lines().filter(|line| !line.trim().is_empty());
        let Some(path) = lines.next() else { continue };
        let path = path.trim().to_string();
        if !seen.insert(path.clone()) {
            continue;
        }
        let version_output = lines.collect::<Vec<_>>().join("\n");
        if let Some(label) = parse_java_version_output(&version_output) {
            installations.push(JavaInstallation { path, major_version: major_version(&label), label });
        }
    }

    dedupe_by_major_version(installations)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_java_version_output_extracts_the_quoted_version() {
        assert_eq!(parse_java_version_output("openjdk version \"17.0.9\" 2023-10-17\nOpenJDK Runtime Environment"), Some("17.0.9".to_string()));
        assert_eq!(parse_java_version_output("java version \"1.8.0_392\"\n"), Some("1.8.0_392".to_string()));
        assert_eq!(parse_java_version_output("not java output"), None);
        assert_eq!(parse_java_version_output(""), None);
    }

    #[test]
    fn major_version_handles_both_java_versioning_schemes() {
        assert_eq!(major_version("21.0.11"), "21");
        assert_eq!(major_version("25"), "25");
        assert_eq!(major_version("1.8.0_481"), "8");
        assert_eq!(major_version("1.7.0_80"), "7");
    }

    #[test]
    fn parse_remote_detect_output_collapses_two_vendors_sharing_a_major_version() {
        let stdout = concat!(
            "===JAVA===\n",
            "/usr/bin/java\n",
            "openjdk version \"21.0.1\" 2023-10-17\n",
            "===JAVA===\n",
            "/usr/lib/jvm/temurin-21-jdk/bin/java\n",
            "openjdk version \"21.0.4\" 2024-01-01\n",
        );

        let installations = parse_remote_detect_output(stdout);

        // Two different real paths, same major version - the PATH-found
        // one (listed first) wins, the vendor-specific duplicate is
        // dropped rather than showing "Java 21" twice.
        assert_eq!(installations.len(), 1);
        assert_eq!(installations[0].path, "/usr/bin/java");
        assert_eq!(installations[0].major_version, "21");
    }

    #[test]
    fn parse_remote_detect_output_reads_multiple_installations_and_dedupes() {
        let stdout = concat!(
            "===JAVA===\n",
            "/usr/lib/jvm/java-17-openjdk/bin/java\n",
            "openjdk version \"17.0.9\" 2023-10-17\n",
            "OpenJDK Runtime Environment\n",
            "===JAVA===\n",
            "/usr/lib/jvm/java-21-openjdk/bin/java\n",
            "openjdk version \"21.0.1\" 2023-10-17\n",
            "===JAVA===\n",
            "/usr/lib/jvm/java-17-openjdk/bin/java\n",
            "openjdk version \"17.0.9\" 2023-10-17\n",
        );

        let installations = parse_remote_detect_output(stdout);

        assert_eq!(installations.len(), 2);
        assert_eq!(installations[0].path, "/usr/lib/jvm/java-17-openjdk/bin/java");
        assert_eq!(installations[0].label, "17.0.9");
        assert_eq!(installations[1].path, "/usr/lib/jvm/java-21-openjdk/bin/java");
        assert_eq!(installations[1].label, "21.0.1");
    }

    #[test]
    fn parse_remote_detect_output_skips_a_block_with_unparseable_version_output() {
        let stdout = "===JAVA===\n/usr/bin/java\nsome unexpected garbage\n";
        assert!(parse_remote_detect_output(stdout).is_empty());
    }

    #[tokio::test]
    async fn detect_local_java_never_panics_and_dedupes_the_path_entry() {
        // Not asserting a specific result - this environment may or may not
        // have a real `java` on PATH - only that the scan completes cleanly
        // and never registers the same path twice.
        let installations = detect_local_java().await;
        let mut seen = HashSet::new();
        for installation in &installations {
            assert!(seen.insert(installation.path.clone()), "duplicate path: {}", installation.path);
        }
    }
}
