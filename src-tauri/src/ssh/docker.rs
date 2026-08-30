//! Docker container control over plain SSH, same reasoning as `systemd.rs`:
//! the SSH session runs as whatever user it authenticated as, so `docker
//! restart <container>` here has exactly the privileges that user (or their
//! membership in the `docker` group) would already have running the same
//! command by hand - no separate elevation mechanism to secure. What does
//! need guarding is that a container name/ID reaches a remote shell command
//! at all - see `validate_container_ref`.

use vibessh_protocol::ContainerSummary;

use super::client::SshSession;
use crate::errors::{AppError, AppResult};

/// `|` as a field separator rather than a Go-template `\t` escape (which
/// isn't guaranteed to survive a shell round trip the same way a literal
/// character does): Docker's own name/ID/image syntax never allows `|`, so
/// it can't appear inside a field and be mistaken for a separator.
const LIST_COMMAND: &str = "docker ps -a --format '{{.ID}}|{{.Names}}|{{.Image}}|{{.Status}}|{{.State}}'";

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
            .execute_command(&format!("docker logs --tail {tail} --timestamps {container} 2>&1"))
            .await?;
        if output.exit_code != 0 {
            let detail = output.stdout.trim();
            let detail = if detail.is_empty() { "docker logs failed".to_string() } else { detail.to_string() };
            return Err(AppError::Connection(format!("couldn't read logs for {container}: {detail}")));
        }
        Ok(output.stdout)
    }

    async fn run_docker(&self, action: &str, container: &str) -> AppResult<()> {
        validate_container_ref(container)?;
        let output = self.execute_command(&format!("docker {action} {container}")).await?;
        if output.exit_code != 0 {
            let detail = output.stderr.trim();
            let detail = if detail.is_empty() { format!("docker {action} failed") } else { detail.to_string() };
            return Err(AppError::Connection(format!("couldn't {action} {container}: {detail}")));
        }
        Ok(())
    }
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
fn validate_container_ref(name: &str) -> AppResult<()> {
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
