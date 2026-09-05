//! Finding game servers that already exist on a machine.
//!
//! **One command, not a walk.** Over SSH each directory listing is its own
//! round trip, and a Pterodactyl host has a folder per server - so asking
//! about twenty of them one at a time is twenty round trips before anything
//! appears on screen. A single shell script does the whole scan and returns
//! one table.
//!
//! Nothing here writes. A scan looks at directories somebody is very likely
//! still running servers out of, so it opens no file it does not need and
//! changes none of them.

use std::path::Path;

use crate::errors::{AppError, AppResult};
use crate::models::{DiscoveredServer, DiscoveredServerKind};
use crate::ssh::command::quote as shell_quote;
use crate::ssh::SshSession;

/// What makes a directory a server rather than a folder that happens to
/// contain a jar.
///
/// A jar alone is not enough, which running this against a real desktop made
/// obvious: it offered to adopt a plugin download, an extracted zip, and a
/// source checkout - every one of them a directory with a `.jar` in it. So a
/// server also has to carry something a server *leaves behind* once it has
/// run.
///
/// Any one of them is enough, because no single file is common to all of
/// them: a proxy has no `server.properties`, a never-started server has no
/// `logs`, and a vanilla server has no `plugins`.
///
/// The cost is a directory holding nothing but a freshly downloaded jar,
/// which is not found. That is the better way round - a server that has never
/// run has nothing to adopt yet, and a list padded with things that are not
/// servers is worse than a short one.
const SERVER_MARKERS: [&str; 7] = ["server.properties", "eula.txt", "velocity.toml", "config.yml", "plugins", "logs", "world"];
const SCAN_SCRIPT: &str = r#"
cd %DIR% 2>/dev/null || exit 0
for d in */; do
  d="${d%/}"
  jar=""
  for f in "$d"/*.jar; do
    if [ -f "$f" ]; then jar="${f#"$d"/}"; break; fi
  done
  [ -n "$jar" ] || continue
  marker=""
  for m in %MARKERS%; do
    if [ -e "$d/$m" ]; then marker="$m"; break; fi
  done
  [ -n "$marker" ] || continue
  port=""
  if [ -f "$d/server.properties" ]; then
    port=$(sed -n 's/^server-port=//p' "$d/server.properties" | head -1 | tr -d '\r')
  fi
  printf '%s\t%s\t%s\n' "$d" "$jar" "$port"
done
"#;

/// Looks for servers wherever this Application would run.
///
/// `None` for the Node is a local scan, the same meaning `server_id` carries
/// everywhere else in the app - so pointing this at a folder on the desktop
/// works exactly as pointing it at `/home/container` on a Node does.
pub async fn scan_for_servers(
    server_repo: &crate::storage::server_repository::ServerRepository,
    sessions: &crate::state::SshSessionManager,
    server_id: Option<uuid::Uuid>,
    directory: &str,
) -> AppResult<Vec<DiscoveredServer>> {
    let directory = directory.trim();
    if directory.is_empty() {
        return Err(AppError::InvalidInput("a directory to look in is required".into()));
    }
    match server_id {
        None => scan_local(directory).await,
        Some(server_id) => {
            let connection = super::ssh_service::get_or_connect(server_repo, sessions, server_id).await?;
            scan_remote(&connection, directory).await
        }
    }
}

pub async fn scan_remote(connection: &SshSession, directory: &str) -> AppResult<Vec<DiscoveredServer>> {
    let markers = SERVER_MARKERS.iter().map(|marker| shell_quote(marker)).collect::<Vec<_>>().join(" ");
    let script = SCAN_SCRIPT.replace("%DIR%", &shell_quote(directory)).replace("%MARKERS%", &markers);
    let output = connection.execute_command(&script).await?;
    if output.exit_code != 0 {
        let detail = output.stderr.trim();
        let detail = if detail.is_empty() { "the directory could not be read".to_string() } else { detail.to_string() };
        return Err(AppError::InvalidInput(format!("couldn't look inside '{directory}': {detail}")));
    }
    Ok(parse_scan(&output.stdout, directory))
}

pub async fn scan_local(directory: &str) -> AppResult<Vec<DiscoveredServer>> {
    let root = Path::new(directory);
    let mut entries = tokio::fs::read_dir(root)
        .await
        .map_err(|err| AppError::InvalidInput(format!("couldn't look inside '{directory}': {err}")))?;

    let mut found = Vec::new();
    while let Some(entry) = entries.next_entry().await.map_err(|err| AppError::Storage(format!("couldn't read '{directory}': {err}")))? {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(jar) = first_jar_in(&path).await else { continue };
        if !looks_like_a_server(&path).await {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let port = read_port(&path.join("server.properties")).await;
        found.push(DiscoveredServer {
            kind: DiscoveredServerKind::from_jar_name(&jar),
            name,
            path: path.to_string_lossy().into_owned(),
            jar,
            port,
        });
    }

    // Sorted so two scans of the same directory read the same way - a
    // filesystem hands them back in whatever order it likes.
    found.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(found)
}

/// Whether anything a server leaves behind is in this directory - see
/// `SERVER_MARKERS`.
async fn looks_like_a_server(directory: &Path) -> bool {
    for marker in SERVER_MARKERS {
        if tokio::fs::metadata(directory.join(marker)).await.is_ok() {
            return true;
        }
    }
    false
}

async fn first_jar_in(directory: &Path) -> Option<String> {
    let mut entries = tokio::fs::read_dir(directory).await.ok()?;
    let mut jars = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.to_lowercase().ends_with(".jar") {
            jars.push(name);
        }
    }
    // Alphabetical so the answer does not depend on directory order, the same
    // reason the outer list is sorted.
    jars.sort();
    jars.into_iter().next()
}

async fn read_port(properties: &Path) -> Option<u16> {
    let text = tokio::fs::read_to_string(properties).await.ok()?;
    text.lines()
        .find_map(|line| line.strip_prefix("server-port="))
        .and_then(|value| value.trim().parse().ok())
}

/// Turns the script's tab-separated output into servers.
///
/// Split out so the parsing is testable without a host to run the script on -
/// which is where the fiddly parts are: an absent port, a name with a space
/// in it, a line the script never meant to emit.
pub fn parse_scan(stdout: &str, directory: &str) -> Vec<DiscoveredServer> {
    let base = directory.trim_end_matches('/');
    let mut found: Vec<DiscoveredServer> = stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let name = parts.next()?.trim();
            let jar = parts.next()?.trim();
            if name.is_empty() || jar.is_empty() {
                return None;
            }
            Some(DiscoveredServer {
                kind: DiscoveredServerKind::from_jar_name(jar),
                name: name.to_string(),
                path: format!("{base}/{name}"),
                jar: jar.to_string(),
                // An empty field is a server with no `server.properties`,
                // not a parse failure.
                port: parts.next().and_then(|value| value.trim().parse().ok()),
            })
        })
        .collect();
    found.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run against a real directory rather than a fixture. Ignored by
    /// default because it needs one: point it at a folder full of servers
    /// with `VIBESSH_SCAN_DIR=... cargo test -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn finds_real_servers_on_this_machine() {
        let Ok(dir) = std::env::var("VIBESSH_SCAN_DIR") else {
            eprintln!("set VIBESSH_SCAN_DIR to a directory holding server folders");
            return;
        };

        let found = scan_local(&dir).await.expect("the directory should be readable");

        for server in &found {
            eprintln!("{:<22} {:<34} port {:?}  {:?}", server.name, server.jar, server.port, server.kind);
        }
        assert!(!found.is_empty(), "nothing found in {dir}");
    }

    #[test]
    fn reads_a_server_out_of_each_line() {
        let out = "limbo\tlimbo-1.0.jar\t25567\noneblock\tpurpur-1.21.4-24.jar\t25568\n";

        let found = parse_scan(out, "/home/container");

        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "limbo");
        assert_eq!(found[0].path, "/home/container/limbo");
        assert_eq!(found[0].port, Some(25567));
        assert_eq!(found[1].kind, DiscoveredServerKind::Purpur);
    }

    /// A proxy keeps its port elsewhere, and that is not a reason to hide it -
    /// it is the one people would most like help setting up.
    #[test]
    fn a_server_with_no_properties_file_is_still_found() {
        let found = parse_scan("proxy\tvelocity-3.4.0.jar\t\n", "/home/container");

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].port, None);
        assert_eq!(found[0].kind, DiscoveredServerKind::Velocity);
    }

    #[test]
    fn a_trailing_slash_on_the_directory_does_not_double_up(){
        let found = parse_scan("limbo\tserver.jar\t\n", "/home/container/");

        assert_eq!(found[0].path, "/home/container/limbo");
    }

    #[test]
    fn a_name_with_a_space_survives() {
        // Tab-separated rather than space-separated for exactly this.
        let found = parse_scan("my server\tserver.jar\t25565\n", "/srv");

        assert_eq!(found[0].name, "my server");
        assert_eq!(found[0].path, "/srv/my server");
    }

    #[test]
    fn a_line_that_is_not_a_server_is_skipped_rather_than_failing() {
        let found = parse_scan("\n\nnoise\nlimbo\tserver.jar\t25565\n", "/srv");

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "limbo");
    }

    #[test]
    fn results_are_sorted_so_two_scans_read_the_same() {
        let found = parse_scan("zulu\ta.jar\t\nAlpha\tb.jar\t\n", "/srv");

        assert_eq!(found.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["Alpha", "zulu"]);
    }
}
