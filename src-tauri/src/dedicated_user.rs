//! A per-Application, unprivileged Linux system account - the identity a
//! Docker Application's own container runs as
//! (`runtime::docker::DockerConfig::run_as_dedicated_user`) and the identity
//! `files::sudo_user::SudoUserApplicationFileProvider` acts as for every
//! file operation, replacing the earlier "chown everything to the
//! connecting SSH admin" approach: that meant every Application on a Node
//! shared one identity, so a bug in this codebase (or a compromised
//! Application process reaching outside its own container) could touch
//! every *other* Application's files too, not just its own. A dedicated
//! account per Application - member of the shared `GROUP` below, nothing
//! else, no login shell, no `sudo` rights of its own - keeps the blast
//! radius of any single Application confined to that Application's own
//! `working_directory`, enforced by the OS's own file permissions rather
//! than this codebase's own bookkeeping alone.
//!
//! **Deliberately not the same identity `runtime::docker`'s connecting SSH
//! session authenticates as.** That account keeps its existing broad `sudo`
//! (Docker, mkdir, chown, apt-get - see every other module's own
//! `sudo`-prefixed commands) for genuine host-management actions, a much
//! wider trust level than "owns one Application's files" - this module's
//! whole point is giving each Application a *narrower* identity than that.

use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::ssh::SshSession;
// The one shared implementation - every module that builds a remote
// command used to carry its own byte-identical copy of this.
use crate::ssh::command::quote as shell_quote;

/// Every dedicated per-Application account's shared group - also what
/// `files::sudo_user`'s installed sudoers rule scopes its `RunAs` list to
/// (`ALL=(%vibessh-apps) NOPASSWD: ...`), so a newly created Application's
/// account is automatically covered without ever touching the sudoers file
/// again.
pub(crate) const GROUP: &str = "vibessh-apps";

/// Deterministic from `application_id` alone - no new stored field needed,
/// matching `runtime::docker::container_name`'s own "derive it, don't store
/// it" convention. Well under the ~32-character `useradd` system username
/// limit (12-char fixed prefix + 12 hex chars = 24).
pub(crate) fn username(application_id: Uuid) -> String {
    format!("vibessh-app-{}", &application_id.simple().to_string()[..12])
}

/// Idempotent: safe to call on every start/restart, not just the first time
/// an Application opts into this - cheap (`getent`/`id` probes before ever
/// touching `groupadd`/`useradd`), and self-healing for an Application that
/// started using this feature before its dedicated account existed, same
/// "don't require the user to remember a manual fix-up step" bar
/// `runtime::docker`'s own working-directory-ownership fix already meets.
///
/// System account (`--system`), no home directory (`--no-create-home` - a
/// game server's own files live in its `working_directory`, not a home
/// directory), no login shell (`--shell /usr/sbin/nologin` - this account
/// is a file/process identity boundary only, never meant for interactive or
/// SSH login).
pub(crate) async fn ensure_provisioned(connection: &SshSession, username: &str) -> AppResult<()> {
    let script = format!(
        "getent group {group} >/dev/null 2>&1 || sudo groupadd --system {group}; \
         id -u {user} >/dev/null 2>&1 || sudo useradd --system --no-create-home --shell /usr/sbin/nologin --gid {group} {user}",
        group = GROUP,
        user = shell_quote(username),
    );
    let output = connection.execute_command(&script).await?;
    if output.exit_code != 0 {
        let detail = output.stderr.trim();
        let detail = if detail.is_empty() { "couldn't provision the Application's dedicated account".to_string() } else { detail.to_string() };
        return Err(AppError::Connection(detail));
    }
    Ok(())
}

/// The dedicated account's own `uid:gid`, as Docker's `--user` flag expects
/// it. Validated against a strict `digits:digits` shape before ever
/// reaching a shell command, same stance
/// `runtime::docker::connecting_user_id` (this module's predecessor, before
/// per-Application accounts existed) already took.
pub(crate) async fn user_id(connection: &SshSession, username: &str) -> AppResult<String> {
    let output = connection.execute_command(&format!("id -u {u} 2>/dev/null && id -g {u} 2>/dev/null", u = shell_quote(username))).await?;
    let mut lines = output.stdout.lines();
    let uid = lines.next().unwrap_or("").trim();
    let gid = lines.next().unwrap_or("").trim();
    let is_numeric = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !is_numeric(uid) || !is_numeric(gid) {
        return Err(AppError::Connection(format!("couldn't determine {username}'s uid:gid")));
    }
    Ok(format!("{uid}:{gid}"))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn username_is_deterministic_stable_and_a_valid_short_system_account_name() {
        let id = Uuid::new_v4();
        let a = username(id);
        let b = username(id);
        assert_eq!(a, b);
        assert!(a.len() <= 32, "{a}");
        assert!(a.starts_with("vibessh-app-"));
        assert!(a.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
    }

    #[test]
    fn username_differs_across_applications() {
        assert_ne!(username(Uuid::new_v4()), username(Uuid::new_v4()));
    }
}
