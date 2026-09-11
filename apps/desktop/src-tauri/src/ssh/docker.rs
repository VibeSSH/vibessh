//! Docker container control over plain SSH. Every command is prefixed with
//! `sudo` unconditionally - the plug-and-play promise VibeSSH makes (connect
//! a fresh cloud server, everything just works) can't depend on the
//! connecting user already being in the host's `docker` group, which a
//! stock cloud image's default user never is even right after
//! `services::install_docker` runs (`usermod -aG` only takes effect on a
//! *new* login session, not the one already connected - not something worth
//! chasing when `sudo` sidesteps the whole problem). Same idiom
//! `firewall::ufw`'s `allow_command` already established: `sudo` is a
//! harmless no-op prefix when the connection already *is* root, and this
//! assumes the same passwordless sudo `install_docker`/
//! `ensure_working_directory_exists` already assume for a freshly connected
//! Node. What does need guarding is that a container name/ID reaches a
//! remote shell command at all - see `validate_container_ref`.

use vibessh_protocol::ContainerSummary;

use super::client::SshSession;
use crate::errors::{AppError, AppResult};

/// `|` as a field separator rather than a Go-template `\t` escape (which
/// isn't guaranteed to survive a shell round trip the same way a literal
/// character does): Docker's own name/ID/image syntax never allows `|`, so
/// it can't appear inside a field and be mistaken for a separator.
const LIST_COMMAND: &str = "sudo docker ps -a --format '{{.ID}}|{{.Names}}|{{.Image}}|{{.Status}}|{{.State}}'";

impl SshSession {
    pub async fn list_containers(&self) -> AppResult<Vec<ContainerSummary>> {
        let output = self.execute_command(LIST_COMMAND).await?;
        if output.exit_code != 0 {
            let detail = output.stderr.trim();
            let detail = if detail.is_empty() { "docker ps failed".to_string() } else { detail.to_string() };
            return Err(AppError::Connection(format!("couldn't list containers: {detail}")));
        }
        Ok(parse_docker_ps_output(&output.stdout))
    }

    pub async fn restart_container(&self, container: &str) -> AppResult<()> {
        self.run_docker("restart", container).await
    }

    pub async fn start_container(&self, container: &str) -> AppResult<()> {
        self.run_docker("start", container).await
    }

    pub async fn stop_container(&self, container: &str) -> AppResult<()> {
        self.run_docker("stop", container).await
    }

    /// A stopped container only, same as clicking "Delete" in Docker
    /// Desktop - a running one must be stopped first rather than silently
    /// force-killed, so a slow shutdown (a database flushing to disk, say)
    /// isn't cut short by a UI click.
    pub async fn remove_container(&self, container: &str) -> AppResult<()> {
        self.run_docker("rm", container).await
    }

    /// `2>&1` merges the container's stderr into the same stream as its
    /// stdout, in the order Docker wrote them - splitting them the way
    /// `execute_command`'s stdout/stderr fields normally would loses the
    /// actual interleaving, which is exactly what someone debugging a crash
    /// loop needs to see. `--timestamps` gives each line a real anchor, and
    /// `tail` is capped so a fat-fingered request for a million lines can't
    /// make an SSH round trip absurdly slow.
    pub async fn container_logs(&self, container: &str, tail: u32) -> AppResult<String> {
        validate_container_ref(container)?;
        let tail = tail.clamp(1, 5000);
        let output = self
            .execute_command(&format!("sudo docker logs --tail {tail} --timestamps {container} 2>&1"))
            .await?;
        if output.exit_code != 0 {
            let detail = output.stdout.trim();
            let detail = if detail.is_empty() { "docker logs failed".to_string() } else { detail.to_string() };
            return Err(AppError::Connection(format!("couldn't read logs for {container}: {detail}")));
        }
        Ok(output.stdout)
    }

    /// Streams a container's output as it is produced.
    ///
    /// `--tail` seeds the view with recent history so an operator opening
    /// the console does not stare at nothing until the next line arrives,
    /// and `-f` then keeps the channel open. No `--timestamps` here, unlike
    /// `container_logs`: that method has to reconstruct interleaving after
    /// the fact from a single collected blob, while a live stream arrives in
    /// order by construction and the prefix would only cost width.
    pub async fn follow_container_logs(
        &self,
        container: &str,
        tail: u32,
        mut on_line: impl FnMut(String) + Send + 'static,
        on_closed: impl FnOnce(Option<String>) + Send + 'static,
    ) -> AppResult<crate::ssh::client::FollowHandle> {
        validate_container_ref(container)?;
        // Zero is meaningful here, unlike in `container_logs`: a reconnect
        // already has the history on screen and wants only what comes next.
        // Replaying the window on every blink would repeat it each time.
        let tail = tail.min(5000);
        // `2>&1` is what lets a container's own stderr reach the console,
        // which is most of what a server writes - but it also delivers the
        // daemon's own complaints as though the container had said them. The
        // one that matters is "no such container": an Application that has
        // never been started has none, the console reconnects on a timer, and
        // each attempt appended the same error until the buffer was nothing
        // else. Dropped rather than rewritten, because the console already
        // has an honest empty state for "not running".
        let filtered = move |line: String| {
            if !is_daemon_missing_container_notice(&line) {
                on_line(line);
            }
        };
        self.follow_command(&format!("sudo docker logs --tail {tail} -f {container} 2>&1"), filtered, on_closed).await
    }

    /// `docker kill` sends SIGKILL immediately, bypassing the container's
    /// own stop timeout - the Docker equivalent of `kill -9`, for when
    /// `stop_container`'s normal graceful stop isn't what's wanted
    /// (Applications' "Kill" action - see `runtime::docker`).
    pub async fn kill_container(&self, container: &str) -> AppResult<()> {
        self.run_docker("kill", container).await
    }

    async fn run_docker(&self, action: &str, container: &str) -> AppResult<()> {
        validate_container_ref(container)?;
        let output = self.execute_command(&format!("sudo docker {action} {container}")).await?;
        if output.exit_code != 0 {
            let detail = output.stderr.trim();
            let detail = if detail.is_empty() { format!("docker {action} failed") } else { detail.to_string() };
            return Err(AppError::Connection(format!("couldn't {action} {container}: {detail}")));
        }
        Ok(())
    }
}

/// Whether a line is the Docker daemon saying there is no container, rather
/// than anything the container itself wrote.
///
/// Deliberately narrow. A broad "looks like an error" filter would swallow
/// real output - a Minecraft server printing a stack trace is exactly what
/// the console is for - so this matches only the daemon's own two ways of
/// saying "there is nothing here to read".
fn is_daemon_missing_container_notice(line: &str) -> bool {
    let line = line.trim();
    line.starts_with("Error response from daemon: No such container")
        || line.starts_with("Error: No such container")
        || line.starts_with("Error response from daemon: can not get logs from container which is dead or marked for removal")
}

/// Parses `LIST_COMMAND`'s `|`-delimited output into `ContainerSummary`s.
/// A line with an unexpected field count (e.g. truncated output) is
/// skipped rather than erroring the whole listing.
fn parse_docker_ps_output(stdout: &str) -> Vec<ContainerSummary> {
    let mut containers = Vec::new();
    for line in stdout.lines() {
        let fields: Vec<&str> = line.split('|').collect();
        let [id, name, image, status, state] = fields[..] else { continue };
        containers.push(ContainerSummary {
            id: id.to_string(),
            name: name.to_string(),
            image: image.to_string(),
            status: status.to_string(),
            running: state == "running",
        });
    }
    containers
}

/// Docker container names/IDs are restricted to `[a-zA-Z0-9][a-zA-Z0-9_.-]*`
/// (the daemon itself rejects anything else at creation time) - checking
/// against that same set before a name ever reaches a remote shell command
/// means there's no string this function accepts that could smuggle in a
/// second command. A generous but finite length cap guards against a
/// pathological input that's technically all-valid-characters but absurdly
/// long.
pub(crate) fn validate_container_ref(name: &str) -> AppResult<()> {
    let starts_ok = name.chars().next().is_some_and(|c| c.is_ascii_alphanumeric());
    let chars_ok = name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    if !starts_ok || !chars_ok || name.len() > 128 {
        return Err(AppError::InvalidInput(format!("'{name}' isn't a valid Docker container name or ID")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_shell_metacharacters_in_a_container_ref() {
        for bogus in ["web; rm -rf /", "web`whoami`", "web$(id)", "web && reboot", "", "-web", ".web"] {
            assert!(validate_container_ref(bogus).is_err(), "should have rejected {bogus:?}");
        }
    }

    #[test]
    fn accepts_realistic_container_refs() {
        for good in ["nginx", "web_app-1", "a1b2c3d4e5f6", "minecraft-server.1"] {
            assert!(validate_container_ref(good).is_ok(), "should have accepted {good:?}");
        }
    }

    /// The line that filled an unstarted Application's console with an
    /// error about nothing the operator did.
    #[test]
    fn the_daemons_missing_container_notice_is_not_console_output() {
        assert!(is_daemon_missing_container_notice(
            "Error response from daemon: No such container: vibessh-app-93150782-6d10-4210-acc1-22a9c77cbad2"
        ));
        assert!(is_daemon_missing_container_notice(
            "Error response from daemon: can not get logs from container which is dead or marked for removal"
        ));
        assert!(is_daemon_missing_container_notice("  Error: No such container: x  "), "leading whitespace must not defeat it");
    }

    /// The filter has to stay narrow. A server printing a stack trace is
    /// exactly what the console exists to show, and a broad "looks like an
    /// error" rule would eat it.
    #[test]
    fn a_containers_own_errors_still_reach_the_console() {
        for line in [
            "[Server thread/ERROR]: Error response from daemon: No such container",
            "java.lang.RuntimeException: No such container",
            "Error response from daemon: conflict: unable to remove repository reference",
            "[16:20:03 WARN]: Can't keep up! Is the server overloaded?",
            "",
        ] {
            assert!(!is_daemon_missing_container_notice(line), "{line:?} is not the daemon saying there is nothing to read");
        }
    }

    #[test]
    fn parses_a_realistic_docker_ps_listing() {
        let stdout = concat!(
            "a1b2c3d4e5f6|web|nginx:latest|Up 3 hours|running\n",
            "f6e5d4c3b2a1|db|mariadb:10.11|Exited (0) 2 days ago|exited\n",
        );
        let containers = parse_docker_ps_output(stdout);

        assert_eq!(containers.len(), 2);
        assert!(containers[0].running);
        assert_eq!(containers[0].name, "web");
        assert_eq!(containers[0].image, "nginx:latest");
        assert!(!containers[1].running);
        assert_eq!(containers[1].status, "Exited (0) 2 days ago");
    }
}
