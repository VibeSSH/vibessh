//! Running `docker` somewhere.
//!
//! **Why this is one method and not a transport interface.** This project
//! already had a twenty-method `ServerConnection` trait meant to abstract
//! "SSH or something else", and removed it: only one implementation was ever
//! written and not one of its methods was ever called. `transport::mod`'s own
//! doc comment records the lesson - shape the seam around what the two
//! implementations actually turn out to share, once there are two.
//!
//! There are now two, and they share exactly this: run `docker` with these
//! arguments, tell me what it printed and whether it worked. Everything else
//! the Docker runtime does over SSH - `chown` for the dedicated-user
//! isolation, the console FIFO - has no local counterpart and is deliberately
//! not behind this.
//!
//! **Arguments, not a command line.** The remote path renders them into a
//! POSIX shell string, because that is what an SSH exec channel takes. The
//! local path hands them to the process directly, because Windows has no such
//! shell and quoting for one that isn't there is how a path with a space
//! becomes two arguments.

use crate::errors::{AppError, AppResult};
use crate::ssh::command::quote as shell_quote;
use crate::ssh::SshSession;
use crate::transport::CommandOutput;

#[async_trait::async_trait]
pub trait DockerCommandRunner: Send + Sync {
    /// Runs `docker` with `args`, exactly as given - no shell, no globbing,
    /// no word splitting, whatever the implementation does underneath.
    async fn docker(&self, args: &[&str]) -> AppResult<CommandOutput>;

    /// Whether this can offer the per-Application dedicated user.
    ///
    /// The isolation is built out of POSIX users, groups and `chown`. A local
    /// Windows daemon has none of those, and the honest answer there is that
    /// the feature is unavailable - not that it quietly did nothing, which
    /// would leave an Application claiming an isolation it does not have.
    fn supports_dedicated_user(&self) -> bool;

    /// Writes `contents` somewhere only this account can read, and returns
    /// the path, **without the contents ever appearing in a command line**.
    ///
    /// This exists for one reason. `docker run -e KEY=VALUE` puts the value
    /// in argv, and argv is world-readable on Linux through
    /// `/proc/<pid>/cmdline`: any local account could read a database
    /// password out of `ps` while the container was being created, and a
    /// process-accounting or audit daemon would write it to disk. Passing
    /// the same values through `--env-file` keeps them out of both.
    async fn write_private_file(&self, contents: &str) -> AppResult<PrivateFile>;

    /// Removes what `write_private_file` made, directory and all.
    async fn remove_private_directory(&self, directory: &str) -> AppResult<()>;
}

/// A file written by `write_private_file`, and the directory holding it.
///
/// Both are returned because removing the file is not enough: the directory
/// is created per call, and leaving thousands of empty ones behind on a
/// long-lived Node is its own small fault.
pub struct PrivateFile {
    pub path: String,
    pub directory: String,
}

#[async_trait::async_trait]
impl DockerCommandRunner for SshSession {
    async fn docker(&self, args: &[&str]) -> AppResult<CommandOutput> {
        // `sudo` because a managed Node's admin is assumed to have passwordless
        // sudo rather than to be in the `docker` group - see
        // `docs/security/agent-privileges.md`.
        let mut line = String::from("sudo docker");
        for arg in args {
            line.push(' ');
            line.push_str(&shell_quote(arg));
        }
        self.execute_command(&line).await
    }

    fn supports_dedicated_user(&self) -> bool {
        true
    }

    async fn write_private_file(&self, contents: &str) -> AppResult<PrivateFile> {
        // `mktemp -d` in one step: an unguessable name, mode 0700, and the
        // creation is atomic. A fixed path under /tmp would let any local
        // account pre-create it, or plant a symlink at it and have the write
        // land somewhere else entirely. Because the directory itself is
        // private and already ours, nothing can be planted inside it
        // afterwards either.
        let output = self.execute_command("mktemp -d /tmp/vibessh.XXXXXXXXXX").await?;
        if output.exit_code != 0 {
            return Err(AppError::Connection(format!("couldn't create a private directory on the Node: {}", output.stderr.trim())));
        }
        let directory = output.stdout.trim().to_string();
        if directory.is_empty() {
            return Err(AppError::Connection("mktemp returned no path".into()));
        }
        let path = format!("{directory}/env");
        // Created with its mode already set rather than chmod-ed afterwards:
        // the gap between the two is a window in which the file exists
        // readable, and the whole point is that it never is.
        let prepare = format!("install -T -m 600 /dev/null {}", shell_quote(&path));
        let output = self.execute_command(&prepare).await?;
        if output.exit_code != 0 {
            return Err(AppError::Connection(format!("couldn't create a private file on the Node: {}", output.stderr.trim())));
        }
        // Over SFTP, so the contents travel as file data rather than as part
        // of a command line. Writing to the existing file keeps its mode.
        self.write_file(&path, contents.as_bytes()).await?;
        Ok(PrivateFile { path, directory })
    }

    async fn remove_private_directory(&self, directory: &str) -> AppResult<()> {
        let output = self.execute_command(&format!("rm -rf {}", shell_quote(directory))).await?;
        if output.exit_code != 0 {
            return Err(AppError::Connection(format!("couldn't remove {directory}: {}", output.stderr.trim())));
        }
        Ok(())
    }
}

/// What to say when `docker` is not there.
///
/// **Named rather than repeated.** Three places spawn `docker` on this
/// machine - the command runner, the console attach and the registry login -
/// and each carried its own copy of a message that said only that it was
/// missing. Somebody reading it still had to go and find out what to
/// install, which on Windows is two things and not one.
///
/// The Windows wording names both because Docker Desktop will not run
/// without WSL2 on Windows 10 Home, where Hyper-V is not available - so
/// "install Docker Desktop" alone sends people to an installer that stops
/// and asks for something else.
/// Whether this failure is "the daemon is not running", and what to say if so.
///
/// **Why the CLI's own words are not enough.** Docker is installed, on PATH
/// and runs - so `docker_missing` never fires - and then it fails with
/// `failed to connect to the docker API at npipe:////./pipe/dockerDesktop
/// LinuxEngine ... The system cannot find the file specified`. That is an
/// accurate description of a missing named pipe and a useless description of
/// the situation, which is that Docker Desktop is not started. Somebody
/// reading it goes looking for a path.
///
/// Matched on the daemon-connection wording rather than on an exit code,
/// because the CLI uses the same code for everything, and on both the Windows
/// pipe and the Unix socket, because the same app manages Nodes over SSH
/// where the message is about `/var/run/docker.sock`.
pub(crate) fn daemon_unreachable(stderr: &str) -> Option<String> {
    let lowered = stderr.to_lowercase();
    let looks_like_it = lowered.contains("cannot connect to the docker daemon")
        || lowered.contains("failed to connect to the docker api")
        || lowered.contains("dockerdesktoplinuxengine")
        || lowered.contains("docker_engine")
        || (lowered.contains("docker.sock") && lowered.contains("connect"));
    if !looks_like_it {
        return None;
    }

    #[cfg(windows)]
    let advice = "Docker isn't running. Start Docker Desktop, wait for the whale in the tray to stop animating, then try again.";
    #[cfg(target_os = "macos")]
    let advice = "Docker isn't running. Start Docker Desktop and wait for it to finish starting, then try again.";
    #[cfg(all(unix, not(target_os = "macos")))]
    let advice = "Docker isn't running. Start it with `sudo systemctl start docker`, then try again.";

    // The original is kept on the end: the sentence above is what somebody
    // needs, and the CLI's own text is what they would paste into a search if
    // it turns out to be something else.
    Some(format!("{advice} (docker said: {})", stderr.trim()))
}

pub(crate) fn docker_missing() -> AppError {
    #[cfg(windows)]
    let detail = concat!(
        "docker isn't installed on this machine, or isn't on PATH. ",
        "Local containers need Docker Desktop, and Docker Desktop needs WSL2: ",
        "run `wsl --install` in an administrator PowerShell, restart the computer, ",
        "then install Docker Desktop from docker.com and restart VibeSSH so it picks up the new PATH.",
    );
    #[cfg(target_os = "macos")]
    let detail = concat!(
        "docker isn't installed on this machine, or isn't on PATH. ",
        "Local containers need Docker Desktop - install it from docker.com, start it, ",
        "then restart VibeSSH so it picks up the new PATH.",
    );
    #[cfg(all(unix, not(target_os = "macos")))]
    let detail = concat!(
        "docker isn't installed on this machine, or isn't on PATH. ",
        "Install it with your package manager (`sudo apt install docker.io`, or Docker's own ",
        "repository), add yourself to the `docker` group, then restart VibeSSH so it picks up ",
        "the new group membership.",
    );
    AppError::InvalidInput(detail.into())
}

/// The Docker daemon on this machine.
pub struct LocalDocker;

#[async_trait::async_trait]
impl DockerCommandRunner for LocalDocker {
    async fn docker(&self, args: &[&str]) -> AppResult<CommandOutput> {
        let mut command = tokio::process::Command::new("docker");
        command.args(args);

        #[cfg(windows)]
        {
            // Same reason a launched application gets it: a packaged build is
            // a GUI process, so every console program it starts would raise a
            // console window of its own - here one per docker invocation.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let output = command.output().await.map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                docker_missing()
            } else {
                AppError::Internal(format!("couldn't run docker: {err}"))
            }
        })?;

        Ok(CommandOutput {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    fn supports_dedicated_user(&self) -> bool {
        // True on a local Linux daemon too, in principle - but the dedicated
        // user is created and chowned over the same connection that runs
        // docker, and that path is SSH-only. Claiming support without it would
        // be the silent no-op this exists to avoid.
        false
    }

    async fn write_private_file(&self, contents: &str) -> AppResult<PrivateFile> {
        // The user's own temp directory, which on Windows is already
        // per-account, plus a random name. The argv exposure this avoids is
        // a Linux `/proc` one and does not exist here, but the two runtimes
        // taking different paths through the same code is how one of them
        // ends up untested - and a local daemon on Linux has exactly the
        // same `/proc` to read.
        let directory = std::env::temp_dir().join(format!("vibessh-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&directory)
            .await
            .map_err(|err| AppError::Internal(format!("couldn't create a private directory: {err}")))?;
        let path = directory.join("env");
        write_owner_only(&path, contents).await?;
        Ok(PrivateFile {
            path: path.to_string_lossy().into_owned(),
            directory: directory.to_string_lossy().into_owned(),
        })
    }

    async fn remove_private_directory(&self, directory: &str) -> AppResult<()> {
        tokio::fs::remove_dir_all(directory)
            .await
            .map_err(|err| AppError::Internal(format!("couldn't remove {directory}: {err}")))
    }
}

/// The remote rendering is worth pinning because it is the half that cannot
/// be checked by running it: an argument that loses its quoting becomes two
/// arguments on the far side of an SSH channel, silently.
/// Writes a file the rest of the machine cannot read.
///
/// On Unix the mode goes on at creation rather than afterwards: a `chmod`
/// following an ordinary create leaves the file briefly world-readable, and
/// a secret that is readable for an instant is readable. Windows has no
/// mode; a file in the user's own temp directory is already confined to that
/// account by the directory's ACL.
async fn write_owner_only(path: &std::path::Path, contents: &str) -> AppResult<()> {
    let mut options = tokio::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).await.map_err(|err| AppError::Internal(format!("couldn't create {}: {err}", path.display())))?;
    tokio::io::AsyncWriteExt::write_all(&mut file, contents.as_bytes())
        .await
        .map_err(|err| AppError::Internal(format!("couldn't write {}: {err}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact text a user reported, from Docker Desktop on Windows with
    /// the engine stopped. It has to be recognised, and the answer has to
    /// come before the CLI's own words about a named pipe.
    #[test]
    fn a_stopped_docker_desktop_is_recognised_from_its_own_wording() {
        let reported = "failed to connect to the docker API at npipe:////./pipe/dockerDesktopLinuxEngine;                         check if the path is correct and if the daemon is running:                         open //./pipe/dockerDesktopLinuxEngine: The system cannot find the file specified.";

        let message = daemon_unreachable(reported).expect("this is the daemon being down");

        assert!(message.starts_with("Docker isn't running."), "{message}");
        assert!(message.contains(reported.trim()), "the original should still be there to search for: {message}");
    }

    /// The wording the CLI uses against a Node over SSH, where the daemon is
    /// behind a Unix socket rather than a Windows pipe.
    #[test]
    fn the_unix_socket_wording_is_recognised_too() {
        assert!(daemon_unreachable("Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?").is_some());
    }

    /// Anything else has to fall through, or a real failure would be
    /// reported as the daemon being down and somebody would restart Docker
    /// for no reason.
    #[test]
    fn an_unrelated_failure_is_left_alone() {
        assert!(daemon_unreachable("Error response from daemon: No such container: vibessh-abc").is_none());
        assert!(daemon_unreachable("docker: invalid reference format").is_none());
        assert!(daemon_unreachable("").is_none());
    }

    /// The message is written across several source lines with backslash
    /// continuations, which strip the newline *and* the following
    /// indentation - get one wrong and the text reaches the user with a gap
    /// in the middle of a sentence, or two words run together.
    #[test]
    fn the_missing_docker_message_reads_as_one_sentence() {
        let AppError::InvalidInput(message) = docker_missing() else {
            panic!("a missing docker is bad input, not an internal failure");
        };

        assert!(!message.contains('\n'), "the message wrapped onto a second line: {message}");
        assert!(!message.contains("  "), "a line continuation left a double space: {message}");
        assert!(message.starts_with("docker isn't installed"), "{message}");
        // Whatever the platform, it has to say what to do next rather than
        // only what is wrong.
        assert!(message.contains("restart VibeSSH"), "the message never says to restart: {message}");
    }

    /// Windows needs both, and naming only Docker Desktop sends somebody to
    /// an installer that stops and asks for the other one.
    #[cfg(windows)]
    #[test]
    fn on_windows_it_names_wsl_as_well_as_docker_desktop() {
        let AppError::InvalidInput(message) = docker_missing() else { unreachable!() };

        assert!(message.contains("Docker Desktop"), "{message}");
        assert!(message.contains("WSL2"), "{message}");
        assert!(message.contains("wsl --install"), "{message}");
    }

    use crate::ssh::command::quote as shell_quote;

    /// The same rendering `SshSession::docker` does, without needing a
    /// session to call it on.
    fn render(args: &[&str]) -> String {
        let mut line = String::from("sudo docker");
        for arg in args {
            line.push(' ');
            line.push_str(&shell_quote(arg));
        }
        line
    }

    /// The POSIX escape for a single quote inside single quotes, built from
    /// characters so no layer of quoting in between can eat it.
    fn escaped() -> String {
        [QUOTE, BACKSLASH, QUOTE, QUOTE].iter().collect()
    }

    const QUOTE: char = '\'';
    const BACKSLASH: char = '\\';

    #[test]
    fn renders_a_plain_invocation() {
        assert_eq!(render(&["ps", "-a"]), "sudo docker 'ps' '-a'");
    }

    #[test]
    fn a_path_with_a_space_stays_one_argument() {
        // The whole reason arguments are passed as a list rather than as a
        // command line: a working directory like `C:\Program Files\x`
        // becomes two arguments the moment anything joins them with spaces.
        let rendered = render(&["run", "-v", "/srv/my server:/data", "paper"]);

        assert!(rendered.contains("'/srv/my server:/data'"), "{rendered}");
    }

    #[test]
    fn an_argument_carrying_a_quote_cannot_end_the_quoting() {
        // A container name is derived from user input. The dangerous text is
        // still *present* in the rendered line - that is not the question -
        // the question is whether it is still inside the quotes, which is
        // what the `'` escape decides.
        let rendered = render(&["rm", "-f", "evil'; rm -rf /; echo x"]);

        assert!(rendered.contains(&escaped()), "the embedded quote was not escaped: {rendered}");
        // With the escaped ones taken out, what is left must pair up: an odd
        // number would mean an argument ran off the end of its own quoting
        // and the rest of the line became shell syntax. Counting them all
        // would not show that - the escape sequence contributes three,
        // because the middle one is deliberately outside the quotes.
        let structural = rendered.replace(&escaped(), "");

        assert_eq!(structural.matches(QUOTE).count() % 2, 0, "unbalanced quoting: {rendered}");
    }
}
