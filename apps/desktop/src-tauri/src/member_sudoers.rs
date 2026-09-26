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
        // With options first - `--tail 200 --timestamps`, the form
        // `runtime::member` reads them in. `docker logs` takes one container.
        commands.insert(command("docker", "logs * vibessh-app-*"));
        // `--no-pager` is not tidiness. Without it `systemctl status` on a
        // terminal opens `less` - as root, under this rule - and `!sh` in
        // `less` is a root shell.
        commands.insert(command("systemctl", "status --no-pager vibessh-app-*"));
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

    if held.contains("applications.console") {
        // Any Application's console, which is what a team-wide permission
        // means. The writer checks its argument is an Application id and
        // reads the line from stdin, so the wildcard reaches no further.
        commands.insert(command(CONSOLE_WRITER_PATH, "*"));
    }

    if held.contains("node.firewall") {
        // What the Firewall page runs, and nothing wider. This used to be
        // `ufw *` and `iptables *`, under a comment saying neither runs
        // anything - but `iptables --modprobe=<program>` runs that program
        // as root, so the rule was root. `iptables` is now reached only
        // through the DOCKER-USER helper, which takes the rule as checked
        // parts and has no way to pass an option on (`firewall::docker_user`).
        // `ufw` gets the forms `firewall::ufw` uses: reading, enabling, and
        // adding or deleting an allow rule, whose arguments ufw parses
        // itself and never runs.
        for arguments in ["status", "show added", "--force enable", "allow *", "delete allow *"] {
            commands.insert(command("ufw", arguments));
        }
        commands.insert(command(crate::firewall::docker_user::HELPER_PATH, "*"));
    }

    if commands.is_empty() {
        Privilege::None
    } else {
        Privilege::Commands(commands.into_iter().collect())
    }
}

// --- One shared Application --------------------------------------------
//
// A role's rules name `vibessh-app-*` - every Application on the Node at
// once, which is what a team-wide permission means. Permissions granted on
// one Application in its Users tab get rules that name that Application and
// nothing else: its container or unit by its exact name, its console by its
// id, its files through the helper run as its own account with its own
// directory fixed as the root. Exact names, never a wildcard where the name
// goes: `*` in sudoers matches spaces too, so `stop vibessh-app-*` would
// also match `stop vibessh-app-<mine> vibessh-app-<theirs>`, and `docker
// stop` takes several containers.

/// Writes one line to an Application's console, as root: the console's fifo
/// lives in the connecting admin's own 0700 directory, which a member's
/// account cannot open. Takes the Application's id as its only argument and
/// the line on stdin, so nothing typed into a console is ever part of a
/// command - see `CONSOLE_WRITER_SCRIPT`.
pub const CONSOLE_WRITER_PATH: &str = "/usr/local/lib/vibessh/console-write";

/// The console writer's whole source. Compared against what is on the Node
/// and installed again when it differs, like the file helper.
///
/// One line only (`read -r`), so input cannot smuggle a second command in on
/// a newline; `timeout`, because a fifo nobody is reading blocks a writer
/// forever; and the id checked to be a UUID before it becomes part of a path.
///
/// **It attaches the console again when nothing is reading it.** The owner's
/// start and restart attach `docker attach` to the fifo; a restart run by a
/// member is a bare `docker restart`, and the attach ends with the process
/// it was attached to. Without this, a member's restart would leave the
/// console deaf until the owner next started the server. So a write nobody
/// reads within two seconds attaches again - as the owner's start would,
/// the running container checked first - and writes once more.
pub const CONSOLE_WRITER_SCRIPT: &str = r#"#!/bin/sh
# Installed by VibeSSH. Writes one line from stdin to an application's
# console: console-write <application id>
set -eu
case "${1:-}" in
  ''|*[!0-9a-f-]*) echo "console-write: not an application id" >&2; exit 2 ;;
esac
fifo="/run/vibessh/console/$1.stdin"
name="vibessh-app-$1"
[ -p "$fifo" ] || { echo "console-write: the console is not attached - start the application first" >&2; exit 3; }
IFS= read -r line || [ -n "${line:-}" ] || exit 0
send() { timeout "$1" sh -c 'printf "%s\n" "$1" >> "$2"' console-write "$line" "$fifo"; }
send 2 && exit 0
# Nothing is reading: the container restarted since it was attached.
[ "$(docker inspect -f '{{.State.Running}}' "$name" 2>/dev/null)" = "true" ] || { echo "console-write: the application is not running" >&2; exit 4; }
exec 3<>"$fifo"
nohup docker attach --sig-proxy=false "$name" <&3 3<&- >/dev/null 2>&1 &
exec 3<&-
sleep 1
send 5
"#;

/// The file helper, which `files::sudo_user` installs and runs as an
/// Application's own account: `<helper> <root> <operation> <arguments...>`.
const FILE_HELPER_PATH: &str = "/usr/local/lib/vibessh/file-helper.sh";

/// The helper's operations that only read. Held separately so "read files"
/// can be granted without "change them" - the helper takes the operation as
/// its second argument, so a rule can name it.
const READ_ONLY_FILE_OPERATIONS: &[&str] = &["realpath", "list", "stat", "read", "readrange", "readsmall"];

/// How an Application runs, as far as its rules are concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantRuntime {
    Docker,
    Systemd,
}

impl GrantRuntime {
    /// The runtime as the shared projection names it - `docker`, `systemd`.
    /// Anything else has no container or unit a rule could name.
    pub fn from_projection(name: &str) -> Option<Self> {
        match name {
            "docker" => Some(Self::Docker),
            "systemd" => Some(Self::Systemd),
            _ => None,
        }
    }
}

/// One shared Application a member may act on.
#[derive(Debug, Clone)]
pub struct ApplicationGrant {
    pub application_id: uuid::Uuid,
    pub runtime: GrantRuntime,
    pub working_directory: String,
    pub permissions: Vec<String>,
}

/// Running the file helper as one Application's account, held to its
/// directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRule {
    /// The Application's own account - `dedicated_user::username`.
    pub account: String,
    /// Its working directory, fixed as the helper's first argument.
    pub root: String,
    /// Every operation, or only the ones in `READ_ONLY_FILE_OPERATIONS`.
    pub write: bool,
}

impl FileRule {
    /// The rule's command list, as sudoers wants it after `NOPASSWD:`.
    pub fn commands(&self) -> String {
        if self.write {
            return format!("{FILE_HELPER_PATH} {root} *", root = self.root);
        }
        READ_ONLY_FILE_OPERATIONS
            .iter()
            .flat_map(|op| [format!("{FILE_HELPER_PATH} {} {op}", self.root), format!("{FILE_HELPER_PATH} {} {op} *", self.root)])
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// What one grant earns on the Node.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApplicationRules {
    /// As root, resolved on the Node like a role's commands.
    pub commands: Vec<AllowedCommand>,
    /// `CONSOLE_WRITER_PATH <id>`, as root.
    pub console: Option<uuid::Uuid>,
    pub files: Option<FileRule>,
    /// What was asked for and could not be written, said rather than dropped.
    pub skipped: Vec<SkippedGrant>,
}

/// A permission granted on an Application that no rule could carry.
///
/// A code rather than a sentence, so the interface says it in the reader's
/// language.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedGrant {
    pub application_id: uuid::Uuid,
    pub reason: SkipReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// Its folder has characters a sudoers rule cannot hold literally.
    FolderUnnameable,
    /// A systemd service has no stdin to write a console line to.
    NoConsole,
    /// A runtime with no container or unit on the Node to name.
    NothingToName,
}

/// A working directory a sudoers rule can hold as a literal argument.
///
/// sudoers gives `,` `:` `=` `\` and whitespace meanings of their own in a
/// command's arguments, so rather than escaping them the directory is held to
/// the characters that mean nothing there - the same shape the disk-limit
/// check holds a cron line to. `/` alone, or any `..`, would make the root
/// the whole machine or a way out of it.
fn rule_safe_directory(directory: &str) -> Option<String> {
    let trimmed = directory.trim_end_matches('/');
    let safe = trimmed.starts_with('/')
        && trimmed.len() > 1
        && !trimmed.split('/').any(|part| part == "..")
        && trimmed.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'));
    safe.then(|| trimmed.to_string())
}

/// Turns one shared Application's grant into rules naming only it.
///
/// Being on the list at all earns viewing - its status and its logs - which
/// is what the Users tab promises beside the checkboxes.
pub fn application_rules(grant: &ApplicationGrant) -> ApplicationRules {
    let held: BTreeSet<&str> = grant.permissions.iter().map(String::as_str).collect();
    let id = grant.application_id;
    let mut rules = ApplicationRules::default();

    match grant.runtime {
        GrantRuntime::Docker => {
            let name = format!("vibessh-app-{id}");
            rules.commands.push(command("docker", format!("inspect {name}")));
            rules.commands.push(command("docker", format!("logs {name}")));
            // Options before the name - `--tail 200 --timestamps`. `docker
            // logs` takes exactly one container, so the wildcard cannot
            // reach a second one.
            rules.commands.push(command("docker", format!("logs * {name}")));
            if held.contains("applications.lifecycle") {
                for verb in ["start", "stop", "restart", "kill"] {
                    rules.commands.push(command("docker", format!("{verb} {name}")));
                }
            }
            if held.contains("applications.console") {
                rules.console = Some(id);
            }
        }
        GrantRuntime::Systemd => {
            let unit = format!("vibessh-app-{id}.service");
            rules.commands.push(command("systemctl", format!("status --no-pager {unit}")));
            if held.contains("applications.lifecycle") {
                for verb in ["start", "stop", "restart"] {
                    rules.commands.push(command("systemctl", format!("{verb} {unit}")));
                }
            }
            if held.contains("applications.console") {
                rules.skipped.push(SkippedGrant { application_id: id, reason: SkipReason::NoConsole });
            }
        }
    }

    let write = held.contains("applications.files.write");
    if write || held.contains("applications.files.read") {
        match rule_safe_directory(&grant.working_directory) {
            Some(root) => rules.files = Some(FileRule { account: crate::dedicated_user::username(id), root, write }),
            None => rules.skipped.push(SkippedGrant { application_id: id, reason: SkipReason::FolderUnnameable }),
        }
    }
    rules
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
        assert!(commands.contains(&command("systemctl", "status --no-pager vibessh-app-*")));
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

    /// `iptables --modprobe=<program>` runs that program as root, and a sudo
    /// wildcard cannot keep it out, so the firewall role must never reach
    /// `iptables` directly - only through the DOCKER-USER helper - and gets
    /// only the `ufw` forms the Firewall page runs.
    #[test]
    fn the_firewall_role_never_reaches_iptables_itself() {
        let Privilege::Commands(commands) = privilege_for(&permissions(&["node.firewall"])) else {
            panic!("the firewall should narrow");
        };
        assert!(!commands.iter().any(|allowed| allowed.binary == "iptables"), "{commands:?}");
        assert!(commands.contains(&command(crate::firewall::docker_user::HELPER_PATH, "*")), "{commands:?}");
        let ufw: Vec<&str> = commands.iter().filter(|allowed| allowed.binary == "ufw").map(|allowed| allowed.arguments.as_str()).collect();
        assert_eq!(ufw, vec!["--force enable", "allow *", "delete allow *", "show added", "status"]);
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

    fn grant(runtime: GrantRuntime, keys: &[&str]) -> ApplicationGrant {
        ApplicationGrant {
            application_id: uuid::Uuid::parse_str("3fe67742-ebd2-453d-b1ee-ae1bb75911dd").unwrap(),
            runtime,
            working_directory: "/srv/vibessh/oneblock".to_string(),
            permissions: permissions(keys),
        }
    }

    /// The point of per-application permissions: every rule names this one
    /// Application, never the `vibessh-app-*` a role gets - a wildcard where
    /// the name goes would also match a second container after the first.
    #[test]
    fn every_application_rule_names_exactly_one_application() {
        for runtime in [GrantRuntime::Docker, GrantRuntime::Systemd] {
            let rules = application_rules(&grant(runtime, &["applications.lifecycle", "applications.console"]));
            assert!(!rules.commands.is_empty());
            for allowed in &rules.commands {
                assert!(allowed.arguments.contains("vibessh-app-3fe67742-ebd2-453d-b1ee-ae1bb75911dd"), "{allowed:?}");
                assert!(!allowed.arguments.contains("vibessh-app-*"), "{allowed:?}");
                assert!(allowed.arguments.ends_with("ae1bb75911dd") || allowed.arguments.ends_with("ae1bb75911dd.service"), "{allowed:?}");
            }
        }
    }

    /// Being on the list is viewing: status and logs, and nothing that
    /// changes anything.
    #[test]
    fn being_on_the_list_earns_viewing_and_nothing_more() {
        let rules = application_rules(&grant(GrantRuntime::Docker, &[]));
        let verbs: Vec<&str> = rules.commands.iter().map(|allowed| allowed.arguments.split(' ').next().unwrap()).collect();
        assert!(verbs.iter().all(|verb| matches!(*verb, "inspect" | "logs")), "{verbs:?}");
        assert_eq!(rules.console, None);
        assert_eq!(rules.files, None);
    }

    #[test]
    fn lifecycle_earns_start_stop_restart_and_kill_of_that_container() {
        let rules = application_rules(&grant(GrantRuntime::Docker, &["applications.lifecycle"]));
        for verb in ["start", "stop", "restart", "kill"] {
            assert!(rules.commands.contains(&command("docker", format!("{verb} vibessh-app-3fe67742-ebd2-453d-b1ee-ae1bb75911dd"))), "{verb}");
        }
        for allowed in &rules.commands {
            for escape in ["run", "create", "exec", "cp", "commit"] {
                assert!(!allowed.arguments.starts_with(escape), "{allowed:?}");
            }
        }
    }

    /// `systemctl status` without `--no-pager` opens `less` as root, and
    /// `!sh` in `less` is a root shell - for the role rule and this one.
    #[test]
    fn systemctl_status_never_opens_a_pager() {
        let rules = application_rules(&grant(GrantRuntime::Systemd, &[]));
        let role = privilege_for(&permissions(&["applications.view"]));
        let Privilege::Commands(role) = role else { panic!("view should narrow") };
        for allowed in rules.commands.iter().chain(role.iter()).filter(|allowed| allowed.binary == "systemctl") {
            if allowed.arguments.starts_with("status") {
                assert!(allowed.arguments.starts_with("status --no-pager "), "{allowed:?}");
            }
        }
    }

    #[test]
    fn the_console_is_granted_through_the_writer_by_id_and_only_for_docker() {
        assert!(application_rules(&grant(GrantRuntime::Docker, &["applications.console"])).console.is_some());
        let service = application_rules(&grant(GrantRuntime::Systemd, &["applications.console"]));
        assert_eq!(service.console, None);
        assert_eq!(service.skipped.len(), 1, "a console that cannot be given is said, not dropped");
    }

    /// Reading files is the read-only operations of the helper, run as the
    /// Application's own account with its folder fixed - not every
    /// operation, which is what writing is.
    #[test]
    fn reading_files_names_only_the_helpers_read_operations() {
        let rules = application_rules(&grant(GrantRuntime::Docker, &["applications.files.read"]));
        let files = rules.files.expect("reading should earn a file rule");
        assert_eq!(files.account, "vibessh-app-3fe67742ebd2");
        assert_eq!(files.root, "/srv/vibessh/oneblock");
        let commands = files.commands();
        for op in ["write", "writein", "delete", "rename", "mkdir", "chmod", "copy", "fetchurl"] {
            assert!(!commands.contains(&format!(" {op}")), "read-only allows {op}: {commands}");
        }
        assert!(!commands.contains("oneblock *"), "read-only must not allow every operation: {commands}");
        assert!(commands.contains("/srv/vibessh/oneblock readsmall *"), "{commands}");

        let write = application_rules(&grant(GrantRuntime::Docker, &["applications.files.write"])).files.unwrap();
        assert!(write.write);
        assert_eq!(write.commands(), "/usr/local/lib/vibessh/file-helper.sh /srv/vibessh/oneblock *");
    }

    /// A folder a sudoers rule cannot hold as a literal - a comma, a space, a
    /// way up and out, or the whole machine - earns no file rule, and says so.
    #[test]
    fn a_folder_a_rule_cannot_name_earns_no_file_access() {
        for directory in ["/", "/srv/a,b", "/srv/a b", "/srv/../etc", "relative/path", "/srv/a=b", "/srv/a:b"] {
            let mut unsafe_grant = grant(GrantRuntime::Docker, &["applications.files.write"]);
            unsafe_grant.working_directory = directory.to_string();
            let rules = application_rules(&unsafe_grant);
            assert_eq!(rules.files, None, "{directory}");
            assert_eq!(rules.skipped.len(), 1, "{directory}");
        }
    }

    #[test]
    fn only_docker_and_systemd_have_something_to_name() {
        assert_eq!(GrantRuntime::from_projection("docker"), Some(GrantRuntime::Docker));
        assert_eq!(GrantRuntime::from_projection("systemd"), Some(GrantRuntime::Systemd));
        assert_eq!(GrantRuntime::from_projection("remoteprocess"), None);
        assert_eq!(GrantRuntime::from_projection("localprocess"), None);
    }

    /// The writer is root-run shell reading another person's input: it has
    /// to parse, and the id has to be checked before it becomes a path.
    #[test]
    fn the_console_writer_parses_and_checks_its_argument() {
        let checked = std::process::Command::new("sh").arg("-n").arg("-c").arg(CONSOLE_WRITER_SCRIPT).status();
        if let Ok(status) = checked {
            assert!(status.success(), "the console writer does not parse");
        }
        assert!(CONSOLE_WRITER_SCRIPT.contains("*[!0-9a-f-]*"), "the id is not checked");
        assert!(CONSOLE_WRITER_SCRIPT.contains("read -r line"), "input should be read as one line");
        assert!(CONSOLE_WRITER_SCRIPT.contains("timeout"), "a fifo nobody reads would block forever");
        // A member's restart ends the owner's attach; the writer has to be
        // able to attach again, and only to a running container.
        assert!(CONSOLE_WRITER_SCRIPT.contains("docker attach --sig-proxy=false \"$name\""), "no re-attach");
        assert!(CONSOLE_WRITER_SCRIPT.contains("{{.State.Running}}"), "re-attach without checking the container runs");
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
