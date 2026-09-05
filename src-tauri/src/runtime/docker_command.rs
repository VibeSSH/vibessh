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
}

#[async_trait::async_trait]
impl DockerCommandRunner for SshSession {
    async fn docker(&self, args: &[&str]) -> AppResult<CommandOutput> {
        // `sudo` because a managed Node's admin is assumed to have passwordless
        // sudo rather than to be in the `docker` group - see
        // `docs/agent-privileges.md`.
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
                AppError::InvalidInput("docker isn't installed on this machine, or isn't on PATH".into())
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
}

/// The remote rendering is worth pinning because it is the half that cannot
/// be checked by running it: an argument that loses its quoting becomes two
/// arguments on the far side of an SSH channel, silently.
#[cfg(test)]
mod tests {
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
