//! A team member's own login account on a shared Node.
//!
//! **Why a separate account rather than sharing one credential.** The
//! alternative was to hand a member the owner's key. Then nothing in the
//! Node's own auth log distinguishes who did what, and revoking one person
//! means rotating a key everywhere and re-issuing it to everyone who stays.
//! With an account each, the log names a person and revoking is one key on
//! one Node. See `docs/planning/team-access-design.md`.
//!
//! **How much privilege the account gets.** As much as the member's role
//! earns and no more - see `member_sudoers`, which turns their effective
//! permissions into the sudo rules written here. A role granting nothing
//! privileged leaves an account that cannot run `sudo` at all; a role
//! granting something that cannot be narrowed without lying about it
//! (a shell, package installation, `docker run`) gets the blanket rule, and
//! the interface is told which permission decided that.
//!
//! **Why the authorized_keys file is written whole.** Appending is how a
//! revoked key survives: the next reconcile adds what should be there and
//! leaves behind what should not. The desired set is written in full, every
//! time, which makes removing a key the same operation as adding one.

use crate::errors::{AppError, AppResult};
use crate::member_sudoers;
use crate::ssh::command::{quote as shell_quote, validate_linux_username};
use crate::ssh::SshSession;

/// Every member account belongs to this group, which is what makes them
/// findable later - a Node with accounts nobody can enumerate is a Node
/// nobody can audit.
const GROUP: &str = "vibessh-members";

/// The group every per-Application account belongs to, and the helper the
/// file rules run. Both are `files::sudo_user`'s, repeated here rather than
/// imported so that this module builds a rule for the mechanism that exists
/// instead of inventing a second one.
const APPLICATION_GROUP: &str = "vibessh-apps";
const FILE_HELPER_PATH: &str = "/usr/local/lib/vibessh/file-helper.sh";

/// Where the member's sudo rules go. One file per member, named after the
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
///
/// **Nothing here writes into the member's home as root.** That home, and
/// everything in it, belongs to somebody who can log in and replace any of
/// it with a symlink between two commands - and root following that link is
/// root writing wherever the member chose. This used to run `sudo install
/// -d -o <member> ~/.ssh`, which chowns through a symlinked `.ssh`, so a
/// member could have been handed `/etc`. The account and its group are
/// root's business; its `.ssh` is made by the member's own account
/// (`authorized_keys_script`), with no more power than the member has.
///
/// Also lifts an expiry left by an earlier revoke, so re-adding somebody
/// who was removed lets them in again.
pub fn provision_script(username: &str) -> AppResult<String> {
    validate_linux_username(username, "the member's account name")?;
    let user = shell_quote(username);
    Ok(format!(
        "set -e; \
         getent group {group} >/dev/null 2>&1 || sudo groupadd {group}; \
         id -u {user} >/dev/null 2>&1 || sudo useradd --create-home --shell /bin/bash --gid {group} {user}; \
         sudo passwd --lock {user} >/dev/null; \
         sudo usermod --expiredate '' {user}",
        group = shell_quote(GROUP),
    ))
}

/// Replaces the account's `authorized_keys` with exactly `keys`.
///
/// Written through a temporary file in the same directory and moved into
/// place, so a connection that drops halfway cannot leave a member locked
/// out of a Node with a half-written file - `mv` within one filesystem is
/// atomic, a partial write is not.
///
/// **Written as the member, not as root.** `sudo -u <member>`: the directory
/// is the member's, so whatever they may have put there - a symlink where
/// `authorized_keys.new` goes, a `.ssh` that points elsewhere - can only
/// lead this write somewhere the member could already write. As root, the
/// same `tee`, `chown` and `chmod` wrote and handed over whatever the link
/// named, `/etc/passwd` included; any member with a login could take root at
/// the next sync. The keys travel on stdin, the home directory as an
/// argument - neither is ever part of the script.
pub fn authorized_keys_script(username: &str, keys: &[String]) -> AppResult<String> {
    validate_linux_username(username, "the member's account name")?;
    for key in keys {
        if key.contains('\n') || key.contains('\r') {
            return Err(AppError::InvalidInput("a public key can't contain a line break".into()));
        }
    }
    let user = shell_quote(username);
    let home = shell_quote(&home_directory(username));
    // Each key on its own line, quoted as one shell word so nothing in a
    // comment field can start a second command.
    let write = keys.iter().map(|key| format!("printf '%s\\n' {}", shell_quote(key))).collect::<Vec<_>>().join("; ");
    let write = if write.is_empty() { "true".to_string() } else { write };
    Ok(format!(
        "set -e; \
         ({write}) | sudo -n -u {user} sh -c {as_member} vibessh-keys {home}",
        as_member = shell_quote(MEMBER_KEYS_WRITE),
    ))
}

/// What the member's own account runs to replace its keys: the home
/// directory is `$1`, the keys arrive on stdin.
const MEMBER_KEYS_WRITE: &str = r#"umask 077 && mkdir -p "$1/.ssh" && chmod 700 "$1/.ssh" && cat > "$1/.ssh/authorized_keys.new" && mv -f "$1/.ssh/authorized_keys.new" "$1/.ssh/authorized_keys""#;

/// The sudo rules this member's role earns them, written as one file.
///
/// Installed through a temporary file that `visudo -c` checks before it is
/// moved into place. A syntactically broken file in `/etc/sudoers.d` takes
/// `sudo` down for every account on the machine, including the one that
/// would have to fix it - which on a remote Node means somebody driving to
/// it. The check is not optional politeness.
///
/// **A member who has earned nothing gets the file removed, not emptied.**
/// An empty file is a file, and one left behind after a role is narrowed
/// would be a rule nobody meant to keep. Removing it is also what makes the
/// narrowing observable from the Node rather than only from the app.
///
/// **Command paths are resolved on the Node.** sudo will not match a rule
/// whose command is not fully qualified, and `docker` is not in the same
/// place on every distribution. So the script looks each one up with
/// `command -v` and drops the rules for anything that is not installed -
/// which is also how a Node without Docker avoids a rule naming a binary
/// that is not there.
///
/// **Per-Application rules** (`applications`) sit beside the role's: the
/// member's role decides what they may do with every Application, each
/// grant what they may do with one. A role that is root already covers them,
/// so they are only written when it is not.
pub fn sudoers_script(username: &str, permissions: &[String], applications: &[member_sudoers::ApplicationRules]) -> AppResult<String> {
    validate_linux_username(username, "the member's account name")?;
    let path = shell_quote(&sudoers_path(username));
    let temporary = shell_quote(&format!("{}.new", sudoers_path(username)));

    let privilege = member_sudoers::privilege_for(permissions);
    let helper = member_sudoers::may_use_the_file_helper(permissions);
    let is_root = matches!(privilege, member_sudoers::Privilege::Root { .. });
    let applications: &[member_sudoers::ApplicationRules] = if is_root { &[] } else { applications };

    // The account name goes into a rule unquoted, because sudoers is not a
    // shell and quotes would become part of the name. That is safe only
    // because `validate_linux_username` above has already restricted it to
    // lowercase letters, digits, underscore and hyphen.
    let mut lines: Vec<String> = Vec::new();
    match &privilege {
        member_sudoers::Privilege::None => {}
        member_sudoers::Privilege::Root { reason } => {
            lines.push(format!("# {reason} cannot be narrowed - see member_sudoers.rs"));
            lines.push(format!("{username} ALL=(ALL) NOPASSWD: ALL"));
        }
        member_sudoers::Privilege::Commands(_) => {}
    }
    if helper {
        // Not root: the Application's own account, through the helper that
        // `files::sudo_user` already installs.
        lines.push(format!("{username} ALL=(%{group}) NOPASSWD: {helper_path}", group = APPLICATION_GROUP, helper_path = FILE_HELPER_PATH));
    }
    for application in applications {
        // The writer's path is fixed, and the id is a UUID, so this needs
        // nothing resolved on the far side.
        if let Some(id) = application.console {
            lines.push(format!("{username} ALL=(root) NOPASSWD: {writer} {id}", writer = member_sudoers::CONSOLE_WRITER_PATH));
        }
    }

    // Everything above is known here. The command rules are not, because
    // their paths only exist on the far side, so they are appended by the
    // script itself.
    let mut commands: std::collections::BTreeSet<member_sudoers::AllowedCommand> = std::collections::BTreeSet::new();
    if let member_sudoers::Privilege::Commands(role_commands) = &privilege {
        commands.extend(role_commands.iter().cloned());
    }
    for application in applications {
        commands.extend(application.commands.iter().cloned());
    }
    // Appended through `sudo tee -a`, not `>>`: the file was created by
    // `sudo tee` and belongs to root, and a `>>` is opened by the admin's own
    // shell. Connecting as root hid that; as any other admin the append was
    // refused, and `set -e` ended the sync on the first rule.
    let mut resolvers = String::new();
    for allowed in &commands {
        let arguments = if allowed.arguments.is_empty() { String::new() } else { format!(" {}", allowed.arguments) };
        resolvers.push_str(&format!(
            "p=$(command -v {binary} 2>/dev/null) && printf '%s ALL=(root) NOPASSWD: %s{arguments}\\n' {user} \"$p\" | sudo tee -a {temporary} >/dev/null; ",
            binary = shell_quote(allowed.binary),
            user = shell_quote(username),
        ));
    }
    // An Application's account exists only once it has been started with
    // one, and a rule naming a missing account is a rule for nobody - so,
    // like a command's path, whether to write it is decided on the Node.
    for application in applications {
        if let Some(files) = &application.files {
            let rule = format!("{username} ALL=({account}) NOPASSWD: {commands}", account = files.account, commands = files.commands());
            resolvers.push_str(&format!(
                "id -u {account} >/dev/null 2>&1 && printf '%s\\n' {rule} | sudo tee -a {temporary} >/dev/null; ",
                account = shell_quote(&files.account),
                rule = shell_quote(&rule),
            ));
        }
    }

    if lines.is_empty() && resolvers.is_empty() {
        // Nothing earned: take away whatever was there.
        return Ok(format!("set -e; sudo rm -f {path} {temporary}"));
    }

    let header = lines.iter().map(|line| format!("printf '%s\\n' {}", shell_quote(line))).collect::<Vec<_>>().join("; ");
    let header = if header.is_empty() { "true".to_string() } else { header };

    Ok(format!(
        "set -e; \
         ({header}) | sudo tee {temporary} >/dev/null; \
         {resolvers}\
         sudo chmod 440 {temporary}; \
         sudo visudo -c -f {temporary} >/dev/null || {{ sudo rm -f {temporary}; exit 9; }}; \
         sudo mv {temporary} {path}"
    ))
}

/// Removes the account's access without removing the account.
///
/// Deliberately two different things. Deleting the account would also
/// delete files it owns, and a member who wrote something in a shared
/// directory should not have it vanish because their access was withdrawn.
///
/// **What actually takes the access away is root's, not the member's.** The
/// sudo rule is removed; the account is expired, which sshd refuses a login
/// for whatever key is offered; and whatever the member is still running is
/// ended. Their `authorized_keys` is emptied too, but only as a courtesy:
/// it lives in a directory the member controls, so it could not be the
/// thing a revocation rests on - a member expecting removal could have made
/// it impossible to rewrite. Re-adding them lifts the expiry
/// (`provision_script`).
pub fn revoke_script(username: &str) -> AppResult<String> {
    validate_linux_username(username, "the member's account name")?;
    let path = shell_quote(&sudoers_path(username));
    let user = shell_quote(username);
    Ok(format!(
        "set -e; \
         sudo rm -f {path}; \
         id -u {user} >/dev/null 2>&1 || exit 0; \
         sudo usermod --expiredate 1 {user}; \
         {{ {keys}; }} || echo 'vibessh: the keys file could not be emptied - the account is expired, which is what refuses the login' >&2; \
         sudo pkill -KILL -u {user} || true",
        keys = authorized_keys_script(username, &[])?,
    ))
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
            assert!(sudoers_script(name, &admin(), &[]).is_err(), "{name} was accepted");
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
        // A revoked member who is added back gets in again.
        assert!(script.contains("usermod --expiredate ''"), "{script}");
    }

    /// The privilege bug: root wrote into the member's home, through any
    /// symlink the member left there. No step that touches the home may run
    /// as root - not `tee`, `chown`, `chmod`, `install` or `mv` - and the keys
    /// are written by the member's own account.
    #[test]
    fn nothing_in_the_members_home_is_written_as_root() {
        let home = "/home/vibessh-m-0123456789ab";
        let scripts = [
            provision_script("vibessh-m-0123456789ab").unwrap(),
            authorized_keys_script("vibessh-m-0123456789ab", &[KEY.to_string()]).unwrap(),
            revoke_script("vibessh-m-0123456789ab").unwrap(),
        ];
        for script in &scripts {
            for step in script.split(';') {
                let step = step.trim();
                if step.contains(home) {
                    assert!(step.contains("sudo -n -u 'vibessh-m-0123456789ab'"), "a step touching the home runs as something else: {step}");
                }
                for as_root in ["sudo tee", "sudo chown", "sudo chmod", "sudo install", "sudo mv"] {
                    assert!(!(step.contains(as_root) && step.contains(".ssh")), "{as_root} on the member's .ssh: {step}");
                }
            }
        }
        let keys = authorized_keys_script("vibessh-m-0123456789ab", &[KEY.to_string()]).unwrap();
        assert!(keys.contains("| sudo -n -u 'vibessh-m-0123456789ab' sh -c"), "{keys}");
    }

    /// Every script the sync runs has to parse - the revoke one nests the
    /// keys script in a group.
    #[test]
    fn the_account_scripts_parse() {
        for script in [
            provision_script("vibessh-m-0123456789ab").unwrap(),
            authorized_keys_script("vibessh-m-0123456789ab", &[KEY.to_string()]).unwrap(),
            revoke_script("vibessh-m-0123456789ab").unwrap(),
        ] {
            if let Ok(status) = std::process::Command::new("sh").arg("-n").arg("-c").arg(&script).status() {
                assert!(status.success(), "{script}");
            }
        }
    }

    /// The member's own step parses and does what it says: run as whoever
    /// runs the test, against a temporary home.
    #[cfg(unix)]
    #[test]
    fn the_members_own_keys_write_replaces_the_file() {
        let home = std::env::temp_dir().join(format!("vibessh-member-home-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(home.join(".ssh")).unwrap();
        std::fs::write(home.join(".ssh/authorized_keys"), "old-key\n").unwrap();
        let mut child = std::process::Command::new("sh")
            .arg("-c")
            .arg(MEMBER_KEYS_WRITE)
            .arg("vibessh-keys")
            .arg(&home)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        std::io::Write::write_all(child.stdin.as_mut().unwrap(), b"new-key\n").unwrap();
        assert!(child.wait().unwrap().success());
        assert_eq!(std::fs::read_to_string(home.join(".ssh/authorized_keys")).unwrap(), "new-key\n");
        assert!(!home.join(".ssh/authorized_keys.new").exists());
        let _ = std::fs::remove_dir_all(&home);
    }

    /// Written whole, not appended. Appending is how a revoked key survives
    /// a reconcile that was meant to remove it.
    #[test]
    fn authorized_keys_is_replaced_rather_than_appended_to() {
        let script = authorized_keys_script("vibessh-m-0123456789ab", &[KEY.to_string()]).unwrap();
        assert!(!script.contains(">>"), "appending leaves revoked keys in place: {script}");
        assert!(script.contains("mv -f"), "the file has to be moved into place, not written in place: {script}");
        assert!(script.contains("umask 077"), "{script}");
    }

    /// Revoking a device produces an empty file rather than no command at
    /// all - "there are no keys" has to be written down, or the old ones
    /// stay.
    #[test]
    fn revoking_writes_an_empty_key_file_rather_than_doing_nothing() {
        let script = revoke_script("vibessh-m-0123456789ab").unwrap();
        assert!(script.contains("authorized_keys"), "{script}");
        assert!(script.contains("/etc/sudoers.d/vibessh-m-0123456789ab"), "{script}");
        // What the revocation rests on is root's: the expiry, and ending
        // what they still run - not a file in their own home.
        assert!(script.contains("usermod --expiredate 1 'vibessh-m-0123456789ab'"), "{script}");
        assert!(script.contains("pkill -KILL -u 'vibessh-m-0123456789ab'"), "{script}");
    }

    /// A role that cannot be narrowed, which is the case that still writes
    /// the blanket rule.
    fn admin() -> Vec<String> {
        vec!["node.terminal".to_string()]
    }

    /// A broken file in /etc/sudoers.d takes sudo down for every account on
    /// the machine - including the one that would have to repair it, which
    /// on a remote Node means physical access.
    #[test]
    fn the_sudoers_file_is_checked_before_it_is_installed() {
        let script = sudoers_script("vibessh-m-0123456789ab", &admin(), &[]).unwrap();
        let check = script.find("visudo -c").expect("no syntax check at all");
        let install = script.rfind("mv").expect("nothing is moved into place");
        assert!(check < install, "the check has to happen before the move: {script}");
        assert!(script.contains("chmod 440"), "sudo refuses a file with looser permissions: {script}");
    }

    /// The point of stage 3, seen from the Node: a role that earns nothing
    /// privileged does not leave a rule behind, it takes one away.
    #[test]
    fn a_role_earning_nothing_removes_the_file_rather_than_writing_an_empty_one() {
        let script = sudoers_script("vibessh-m-0123456789ab", &["team.view".to_string()], &[]).unwrap();
        assert!(script.contains("rm -f"), "{script}");
        assert!(!script.contains("NOPASSWD"), "nothing should be granted: {script}");
    }

    /// A narrow role writes named commands and never the blanket rule.
    #[test]
    fn a_narrow_role_writes_specific_commands_and_no_blanket_rule() {
        let held = vec!["applications.view".to_string(), "applications.lifecycle".to_string()];
        let script = sudoers_script("vibessh-m-0123456789ab", &held, &[]).unwrap();
        assert!(!script.contains("NOPASSWD: ALL"), "a narrow role must not get the blanket rule: {script}");
        // Resolved on the far side, because sudo needs a full path and
        // `docker` is not in the same place on every distribution.
        assert!(script.contains("command -v"), "{script}");
        assert!(script.contains("start vibessh-app-*"), "{script}");
        assert!(script.contains("visudo -c"), "still checked before it is installed: {script}");
    }

    /// The blanket rule is still written when the role has earned it, and it
    /// says which permission decided so - the file is what somebody reads on
    /// the machine, months later, wondering why this account is root.
    #[test]
    fn a_root_equivalent_role_writes_the_blanket_rule_and_names_the_reason() {
        let script = sudoers_script("vibessh-m-0123456789ab", &admin(), &[]).unwrap();
        assert!(script.contains("NOPASSWD: ALL"), "{script}");
        assert!(script.contains("node.terminal"), "the file should say what decided this: {script}");
    }

    /// File access is the one rule that is not root: it runs as the
    /// Application's own account, through the helper that already exists.
    #[test]
    fn file_access_runs_as_the_applications_account_and_not_as_root() {
        let script = sudoers_script("vibessh-m-0123456789ab", &["applications.files.write".to_string()], &[]).unwrap();
        assert!(script.contains("(%vibessh-apps)"), "{script}");
        assert!(script.contains("/usr/local/lib/vibessh/file-helper.sh"), "{script}");
        assert!(!script.contains("NOPASSWD: ALL"), "{script}");
    }

    /// A failed check must not leave the half-written file behind for the
    /// next run to move into place.
    #[test]
    fn a_file_that_fails_its_check_is_removed_rather_than_left() {
        let script = sudoers_script("vibessh-m-0123456789ab", &admin(), &[]).unwrap();
        let check = script.find("visudo -c").expect("no syntax check at all");
        let cleanup = script[check..].find("rm -f").expect("nothing cleans up a rejected file");
        let install = script[check..].find("mv").expect("nothing is moved into place");
        assert!(cleanup < install, "the rejected file has to go before the move: {script}");
    }

    fn oneblock(keys: &[&str]) -> member_sudoers::ApplicationRules {
        member_sudoers::application_rules(&member_sudoers::ApplicationGrant {
            application_id: uuid::Uuid::parse_str("3fe67742-ebd2-453d-b1ee-ae1bb75911dd").unwrap(),
            runtime: member_sudoers::GrantRuntime::Docker,
            working_directory: "/srv/vibessh/oneblock".to_string(),
            permissions: keys.iter().map(|key| (*key).to_string()).collect(),
        })
    }

    /// A member with no role beyond seeing the team, and restart, console
    /// and files on one Application: a file with exactly that, checked
    /// before it is installed, and never the blanket rule.
    #[test]
    fn per_application_grants_are_written_beside_a_role_that_earns_nothing() {
        let applications = [oneblock(&["applications.lifecycle", "applications.console", "applications.files.write"])];
        let script = sudoers_script("vibessh-m-0123456789ab", &["team.view".to_string()], &applications).unwrap();
        assert!(!script.contains("NOPASSWD: ALL"), "{script}");
        assert!(script.contains("restart vibessh-app-3fe67742-ebd2-453d-b1ee-ae1bb75911dd"), "{script}");
        assert!(script.contains("/usr/local/lib/vibessh/console-write 3fe67742-ebd2-453d-b1ee-ae1bb75911dd"), "{script}");
        // The Application's own account, and only if it exists on the Node.
        assert!(script.contains("id -u 'vibessh-app-3fe67742ebd2'"), "{script}");
        assert!(script.contains("ALL=(vibessh-app-3fe67742ebd2) NOPASSWD: /usr/local/lib/vibessh/file-helper.sh /srv/vibessh/oneblock *"), "{script}");
        assert!(script.contains("visudo -c"), "{script}");
    }

    /// A role that is already root makes per-application rules decoration -
    /// a list of specific commands beside `ALL` reads as a limit that is
    /// not there.
    #[test]
    fn a_root_role_is_not_dressed_up_with_per_application_rules() {
        let script = sudoers_script("vibessh-m-0123456789ab", &admin(), &[oneblock(&["applications.lifecycle"])]).unwrap();
        assert!(script.contains("NOPASSWD: ALL"), "{script}");
        assert!(!script.contains("restart vibessh-app-3fe67742"), "{script}");
    }

    /// The rules file belongs to root from the moment `sudo tee` creates it.
    /// A `>>` is opened by the admin's own shell, which works as root and
    /// fails as anybody else - checked in an Ubuntu container as a non-root
    /// admin, where it ended the sync on the first rule.
    #[test]
    fn rules_are_appended_through_sudo_and_never_by_the_admins_shell() {
        let applications = [oneblock(&["applications.lifecycle", "applications.files.read"])];
        let script = sudoers_script("vibessh-m-0123456789ab", &["applications.view".to_string()], &applications).unwrap();
        assert!(!script.contains(">> '/etc/sudoers.d/"), "{script}");
        assert!(script.contains("| sudo tee -a '/etc/sudoers.d/vibessh-m-0123456789ab.new'"), "{script}");
    }

    /// The generated script is shell run as the admin over SSH; it has to
    /// parse, per-application rules and all.
    #[test]
    fn the_script_with_per_application_rules_parses() {
        let applications = [oneblock(&["applications.lifecycle", "applications.console", "applications.files.read"])];
        let script = sudoers_script("vibessh-m-0123456789ab", &["applications.view".to_string()], &applications).unwrap();
        if let Ok(status) = std::process::Command::new("sh").arg("-n").arg("-c").arg(&script).status() {
            assert!(status.success(), "{script}");
        }
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
