//! What a member's role lets their account do on a Node, as sudo rules.
//!
//! Stage 3 of `docs/planning/team-access-design.md`. Until this, a member's
//! account carried `ALL=(ALL) NOPASSWD: ALL` and a role was a description of
//! what the *app* would offer them, enforced nowhere else - anybody who went
//! around the app had the owner's powers. This is where the Node starts
//! enforcing the same thing the interface claims.
//!
//! **The honest part first: several permissions cannot be narrowed, and this
//! module says so rather than pretending.** A permission is narrowable only
//! when the commands behind it cannot be turned into arbitrary code as root.
//! That rules out more than it first looks like:
//!
//! - `node.terminal` is a shell.
//! - `node.software` runs package maintainer scripts, which are root.
//! - `applications.create` and `applications.delete` need `useradd`,
//!   `install` and `chown` against a directory the user chooses, plus
//!   `docker run` - and `docker run -v /:/host` is the host.
//! - `node.services` is `systemctl` over arbitrary units, which includes
//!   replacing one and starting it.
//! - `node.network` is `wg-quick`, whose config files carry `PostUp`
//!   commands that run as root.
//!
//! Holding any of those means the account is root-equivalent, and the rule
//! written is the blanket one. Writing a careful-looking allowlist that a
//! `docker run` walks straight out of would be worse than the blanket rule,
//! because it would look like a restriction.
//!
//! **What narrows for real.** Starting and stopping a container that already
//! exists cannot mount anything. Reading status cannot write. File access
//! goes through the helper that already exists, run as the Application's own
//! account rather than as root. Those are the rules this can restrict, and
//! it does.
//!
//! **A member with no privileged permission gets no sudoers file at all** -
//! and an existing one is removed. That is the visible difference this stage
//! makes: "view only" now means the account cannot run `sudo` on that
//! machine, rather than meaning the app declines to show a button.

use std::collections::BTreeSet;

/// A permission that cannot be reduced to a command list without lying about
/// it - see this module's own doc comment for why each one is here.
const ROOT_EQUIVALENT: &[&str] = &[
    "node.terminal",
    "node.software",
    "node.services",
    "node.network",
    "applications.create",
    "applications.delete",
    "applications.config",
    "applications.databases",
    "applications.backups",
];

/// One command a rule may allow, as the binary to look for and the argument
/// pattern sudo should match.
///
/// The binary is a bare name because its path differs between
/// distributions - `/usr/bin/docker` on one, `/usr/local/bin/docker` on
/// another - and sudo will not match a rule that is not fully qualified. The
/// script resolves each name on the Node itself and drops the ones that are
/// not installed, which is also what keeps a Node without Docker from
/// getting a rule naming a binary that is not there.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AllowedCommand {
    pub binary: &'static str,
    /// Appended after the resolved path. Empty means the bare command.
    pub arguments: String,
}

/// The sudo rules one member's permissions earn them on a Node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Privilege {
    /// Nothing needs `sudo`. No file is written and any existing one goes.
    None,
    /// Exactly these commands, as root.
    Commands(Vec<AllowedCommand>),
    /// Everything, because at least one permission held cannot be narrowed.
    /// `reason` names the permission that decided it, so the interface can
    /// say which one rather than leaving somebody to guess.
    Root { reason: String },
}

fn command(binary: &'static str, arguments: impl Into<String>) -> AllowedCommand {
    AllowedCommand { binary, arguments: arguments.into() }
}

/// Turns a member's effective permissions into what their account may run.
///
/// Order matters: a root-equivalent permission wins over every narrow one,
/// because the account really is root and the rest of the list would be
/// decoration on top of that.
pub fn privilege_for(permissions: &[String]) -> Privilege {
    let held: BTreeSet<&str> = permissions.iter().map(String::as_str).collect();

    if let Some(reason) = ROOT_EQUIVALENT.iter().find(|key| held.contains(**key)) {
        return Privilege::Root { reason: (*reason).to_string() };
    }

    let mut commands: BTreeSet<AllowedCommand> = BTreeSet::new();

    if held.contains("applications.view") {
        // Read-only, and scoped to this app's own containers and units
        // wherever the command takes a name at all. `docker ps` takes none.
        commands.insert(command("docker", "ps"));
        commands.insert(command("docker", "ps *"));
        commands.insert(command("docker", "inspect vibessh-app-*"));
        commands.insert(command("docker", "logs vibessh-app-*"));
        commands.insert(command("systemctl", "status vibessh-app-*"));
    }

    if held.contains("applications.lifecycle") {
        // Starting a container that already exists runs it as it was
        // created - the escape in `docker` is `run`/`create`/`exec`, which
        // are not here.
        for verb in ["start", "stop", "restart", "kill"] {
            commands.insert(command("docker", format!("{verb} vibessh-app-*")));
        }
        for verb in ["start", "stop", "restart"] {
            commands.insert(command("systemctl", format!("{verb} vibessh-app-*")));
        }
    }

    if held.contains("node.firewall") {
        // `ufw` and `iptables` change what the machine accepts, which is the
        // whole point of the permission, and neither runs anything.
        commands.insert(command("ufw", "*"));
        commands.insert(command("iptables", "*"));
    }

    if commands.is_empty() {
        Privilege::None
    } else {
        Privilege::Commands(commands.into_iter().collect())
    }
}

/// Whether this member's files permissions earn the helper rule.
///
/// Separate from the command list because it is a different shape: it runs
/// as the Application's own account (`%vibessh-apps`) rather than as root,
/// which is the narrower thing and the reason `files::sudo_user` exists.
pub fn may_use_the_file_helper(permissions: &[String]) -> bool {
    permissions.iter().any(|key| key == "applications.files.read" || key == "applications.files.write")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn permissions(keys: &[&str]) -> Vec<String> {
        keys.iter().map(|key| (*key).to_string()).collect()
    }

    /// The change this stage is for: a role that grants nothing privileged
    /// leaves an account that cannot use `sudo` at all.
    #[test]
    fn a_member_with_nothing_privileged_gets_no_sudo() {
        assert_eq!(privilege_for(&permissions(&["team.view", "audit.view"])), Privilege::None);
        assert_eq!(privilege_for(&[]), Privilege::None);
    }

    #[test]
    fn viewing_earns_read_only_commands_scoped_to_this_apps_containers() {
        let Privilege::Commands(commands) = privilege_for(&permissions(&["applications.view"])) else {
            panic!("view should narrow");
        };
        assert!(commands.contains(&command("docker", "inspect vibessh-app-*")));
        assert!(commands.contains(&command("systemctl", "status vibessh-app-*")));
        // Nothing that writes.
        for allowed in &commands {
            assert!(!allowed.arguments.starts_with("stop"), "{allowed:?}");
            assert!(!allowed.arguments.starts_with("start"), "{allowed:?}");
        }
    }

    /// The escape hatches in `docker` are `run`, `create` and `exec` - a
    /// rule allowing any of them is a rule allowing root, whatever it looks
    /// like.
    #[test]
    fn no_narrow_rule_ever_allows_docker_to_make_a_new_container() {
        for keys in [
            vec!["applications.view"],
            vec!["applications.lifecycle"],
            vec!["applications.view", "applications.lifecycle", "node.firewall"],
        ] {
            let Privilege::Commands(commands) = privilege_for(&permissions(&keys)) else {
                panic!("{keys:?} should narrow");
            };
            for allowed in &commands {
                if allowed.binary != "docker" {
                    continue;
                }
                for escape in ["run", "create", "exec", "cp", "commit"] {
                    assert!(!allowed.arguments.starts_with(escape), "{keys:?} allows docker {escape}: {allowed:?}");
                }
            }
        }
    }

    /// Every permission that cannot be narrowed has to produce the blanket
    /// rule, and name itself while doing it. A new one added to the catalog
    /// and forgotten here would silently get a narrow rule it walks out of.
    #[test]
    fn each_root_equivalent_permission_is_reported_as_root_and_says_which() {
        for key in ROOT_EQUIVALENT {
            match privilege_for(&permissions(&[key])) {
                Privilege::Root { reason } => assert_eq!(&reason, key),
                other => panic!("{key} should be root-equivalent, got {other:?}"),
            }
        }
    }

    /// A narrow permission alongside a root-equivalent one does not dilute
    /// it. The account is root; a list of specific commands next to that
    /// would read as a restriction that is not there.
    #[test]
    fn one_root_equivalent_permission_decides_the_whole_rule() {
        let held = permissions(&["applications.view", "applications.lifecycle", "node.terminal"]);
        assert_eq!(privilege_for(&held), Privilege::Root { reason: "node.terminal".to_string() });
    }

    #[test]
    fn file_access_is_asked_for_separately_because_it_does_not_run_as_root() {
        assert!(may_use_the_file_helper(&permissions(&["applications.files.read"])));
        assert!(may_use_the_file_helper(&permissions(&["applications.files.write"])));
        assert!(!may_use_the_file_helper(&permissions(&["applications.view"])));
        // And it is not what decides whether there is any sudo at all.
        assert_eq!(privilege_for(&permissions(&["applications.files.read"])), Privilege::None);
    }
}
