//! A team member's own login account on a shared Node.
//!
//! **Why a separate account rather than sharing one credential.** The
//! alternative was to hand a member the owner's key. Then nothing in the
//! Node's own auth log distinguishes who did what, and revoking one person
//! means rotating a key everywhere and re-issuing it to everyone who stays.
//! With an account each, the log names a person and revoking is one key on
//! one Node. See `docs/planning/team-access-design.md`.
//!
//! **What this does not buy, stated here so nobody reads it as more.** The
//! desktop's Node operations need `sudo` for `docker`, `ufw`, `iptables`,
//! `wg`, `apt-get` and `systemctl`. An account that can do what the app does
//! is root in all but name, so at this stage a member's account is *as
//! privileged as the owner's*. What it adds is accountability and
//! revocability. Narrowing the privilege to what a member's role allows is a
//! later stage, and until it lands nothing in the interface may describe a
//! role as a restriction.
//!
//! **Why the authorized_keys file is written whole.** Appending is how a
//! revoked key survives: the next reconcile adds what should be there and
//! leaves behind what should not. The desired set is written in full, every
//! time, which makes removing a key the same operation as adding one.

use crate::errors::{AppError, AppResult};
use crate::ssh::command::{quote as shell_quote, validate_linux_username};
use crate::ssh::SshSession;

/// Every member account belongs to this group, which is what makes them
/// findable later - a Node with accounts nobody can enumerate is a Node
/// nobody can audit.
const GROUP: &str = "vibessh-members";

/// Where the blanket sudo rule goes. One file per member, named after the
/// account, so removing a member is removing one file rather than editing a
/// shared one - an edit that goes wrong takes `sudo` down for everybody.
fn sudoers_path(username: &str) -> String {
    format!("/etc/sudoers.d/{username}")
}

fn home_directory(username: &str) -> String {
    format!("/home/{username}")
}

/// Creates the account if it is not there, and leaves it alone if it is.
///
/// Unlike an Application's dedicated account this one is a real login: it
/// needs a home directory for `authorized_keys` and a shell, because a
/// member connects to it over SSH. It has no password and none can be set -
/// the only way in is a key, which is the point.
pub fn provision_script(username: &str) -> AppResult<String> {
    validate_linux_username(username, "the member's account name")?;
    let user = shell_quote(username);
    let home = shell_quote(&home_directory(username));
    Ok(format!(
        "set -e; \
         getent group {group} >/dev/null 2>&1 || sudo groupadd {group}; \
         id -u {user} >/dev/null 2>&1 || sudo useradd --create-home --shell /bin/bash --gid {group} {user}; \
         sudo passwd --lock {user} >/dev/null; \
         sudo install -d -m 700 -o {user} -g {group} {home}/.ssh",
        group = shell_quote(GROUP),
    ))
}

/// Replaces the account's `authorized_keys` with exactly `keys`.
///
/// Written through a temporary file in the same directory and moved into
/// place, so a connection that drops halfway cannot leave a member locked
/// out of a Node with a half-written file - `mv` within one filesystem is
/// atomic, a partial write is not.
pub fn authorized_keys_script(username: &str, keys: &[String]) -> AppResult<String> {
    validate_linux_username(username, "the member's account name")?;
    for key in keys {
        if key.contains('\n') || key.contains('\r') {
            return Err(AppError::InvalidInput("a public key can't contain a line break".into()));
        }
    }
    let user = shell_quote(username);
    let home = home_directory(username);
    let path = shell_quote(&format!("{home}/.ssh/authorized_keys"));
    let temporary = shell_quote(&format!("{home}/.ssh/authorized_keys.new"));
    // Each key on its own line, quoted as one shell word so nothing in a
    // comment field can start a second command.
    let write = keys.iter().map(|key| format!("printf '%s\\n' {}", shell_quote(key))).collect::<Vec<_>>().join("; ");
    let write = if write.is_empty() { "true".to_string() } else { write };
    Ok(format!(
        "set -e; \
         ({write}) | sudo tee {temporary} >/dev/null; \
         sudo chown {user}:{group} {temporary}; \
         sudo chmod 600 {temporary}; \
         sudo mv {temporary} {path}",
        group = shell_quote(GROUP),
    ))
}

/// The blanket sudo rule for stage 1.
///
/// Installed through a temporary file that `visudo -c` checks before it is
/// moved into place. A syntactically broken file in `/etc/sudoers.d` takes
/// `sudo` down for every account on the machine, including the one that
/// would have to fix it - which on a remote Node means somebody driving to
/// it. The check is not optional politeness.
pub fn sudoers_script(username: &str) -> AppResult<String> {
    validate_linux_username(username, "the member's account name")?;
    let path = shell_quote(&sudoers_path(username));
    let temporary = shell_quote(&format!("{}.new", sudoers_path(username)));
    // The account name goes into the rule unquoted, because sudoers is not a
    // shell and quotes would become part of the name. That is safe only
    // because `validate_linux_username` above has already restricted it to
    // lowercase letters, digits, underscore and hyphen.
    let rule = shell_quote(&format!("{username} ALL=(ALL) NOPASSWD: ALL"));
    Ok(format!(
        "set -e; \
         printf '%s\\n' {rule} | sudo tee {temporary} >/dev/null; \
         sudo chmod 440 {temporary}; \
         sudo visudo -c -f {temporary} >/dev/null; \
         sudo mv {temporary} {path}"
    ))
}

/// Removes the account's access without removing the account.
///
/// Deliberately two different things. Emptying `authorized_keys` and taking
/// away the sudo rule stops the member doing anything; deleting the account
/// would also delete files it owns, and a member who wrote something in a
/// shared directory should not have it vanish because their access was
/// withdrawn.
pub fn revoke_script(username: &str) -> AppResult<String> {
    validate_linux_username(username, "the member's account name")?;
    let path = shell_quote(&sudoers_path(username));
    Ok(format!("sudo rm -f {path}; {keys}", keys = authorized_keys_script(username, &[])?))
}

/// Runs one of the scripts above and turns a non-zero exit into the Node's
/// own words rather than a bare failure.
pub async fn run(connection: &SshSession, script: &str, what: &str) -> AppResult<()> {
    let output = connection.execute_command(script).await?;
    if output.exit_code != 0 {
        let detail = output.stderr.trim();
        let detail = if detail.is_empty() { what.to_string() } else { format!("{what}: {detail}") };
        return Err(AppError::Connection(format!("couldn't {detail}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICz0PKtIFpwteAQuKKR6Efa20YOAhyRrvRXT3qkVb7he laptop@example";

    #[test]
    fn an_invalid_account_name_never_reaches_a_command() {
        for name in ["../root", "a b", "root; rm -rf /", "UPPER", ""] {
            assert!(provision_script(name).is_err(), "{name} was accepted");
            assert!(authorized_keys_script(name, &[]).is_err(), "{name} was accepted");
            assert!(sudoers_script(name).is_err(), "{name} was accepted");
        }
    }

    /// A real login, unlike an Application's account: a member connects to
    /// it, so it needs a home for authorized_keys and a shell to run in.
    #[test]
    fn the_account_is_a_login_with_no_password() {
        let script = provision_script("vibessh-m-0123456789ab").unwrap();
        assert!(script.contains("--create-home"), "{script}");
        assert!(script.contains("--shell /bin/bash"), "{script}");
        // The only way in is a key. A locked password is not a password
        // somebody can guess.
        assert!(script.contains("passwd --lock"), "{script}");
        assert!(script.contains("-m 700"), "the .ssh directory has to be private: {script}");
    }

    /// Written whole, not appended. Appending is how a revoked key survives
    /// a reconcile that was meant to remove it.
    #[test]
    fn authorized_keys_is_replaced_rather_than_appended_to() {
        let script = authorized_keys_script("vibessh-m-0123456789ab", &[KEY.to_string()]).unwrap();
        assert!(!script.contains("tee -a"), "appending leaves revoked keys in place: {script}");
        assert!(script.contains("mv"), "the file has to be moved into place, not written in place: {script}");
        assert!(script.contains("chmod 600"), "{script}");
    }

    /// Revoking a device produces an empty file rather than no command at
    /// all - "there are no keys" has to be written down, or the old ones
    /// stay.
    #[test]
    fn revoking_writes_an_empty_key_file_rather_than_doing_nothing() {
        let script = revoke_script("vibessh-m-0123456789ab").unwrap();
        assert!(script.contains("authorized_keys"), "{script}");
        assert!(script.contains("/etc/sudoers.d/vibessh-m-0123456789ab"), "{script}");
    }

    /// A broken file in /etc/sudoers.d takes sudo down for every account on
    /// the machine - including the one that would have to repair it, which
    /// on a remote Node means physical access.
    #[test]
    fn the_sudoers_file_is_checked_before_it_is_installed() {
        let script = sudoers_script("vibessh-m-0123456789ab").unwrap();
        let check = script.find("visudo -c").expect("no syntax check at all");
        let install = script.rfind("mv").expect("nothing is moved into place");
        assert!(check < install, "the check has to happen before the move: {script}");
        assert!(script.contains("chmod 440"), "sudo refuses a file with looser permissions: {script}");
    }

    /// A key's comment field is free text and arrives from another person's
    /// device. It must be a value, never part of the command.
    #[test]
    fn a_key_is_one_shell_word_however_its_comment_reads() {
        let hostile = format!("{KEY}; rm -rf /");
        let script = authorized_keys_script("vibessh-m-0123456789ab", std::slice::from_ref(&hostile)).unwrap();
        // The whole value, `; rm -rf /` included, has to sit inside one
        // quoted word. Asserting that the dangerous text is absent would be
        // wrong - it is present, and harmlessly so, which is the point.
        assert!(script.contains(&format!("'{hostile}'")), "the key is not one quoted word: {script}");
    }
}
