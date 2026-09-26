//! Source restrictions for **published Docker ports**, which `ufw` alone
//! cannot enforce.
//!
//! **Why this exists.** When Docker publishes a port it writes its own
//! `nat/PREROUTING` DNAT rule and an ACCEPT into the `DOCKER` chain of
//! `filter/FORWARD`. Both are evaluated *before* ufw's `ufw-user-input`
//! chain ever sees the packet, so a `ufw allow from 10.77.0.0/16 to any
//! port 3306` has no effect whatsoever on a container published on 3306.
//! The operator reads a correct-looking `ufw status` and has an exposed
//! service. This is a well-known Docker/ufw interaction, not a bug in ufw.
//!
//! Docker provides exactly one supported hook for this: the `DOCKER-USER`
//! chain, which Docker creates and jumps to first from `FORWARD`, and which
//! Docker never rewrites. Rules placed there are evaluated before any of
//! Docker's own, so they can drop traffic that Docker would otherwise
//! accept.
//!
//! **This is defense in depth, not the primary control.** The primary
//! control is `services::application_service::resolve_bind_address`, which
//! binds a non-public port to a specific address so the kernel refuses the
//! connection at the socket. This module exists because that only covers
//! ports VibeSSH itself created: an operator's own `--network host`
//! container, a port published out of band, or a bind address that later
//! stops matching all still benefit from a filter-level rule. It also makes
//! the Firewall page honest - a source-scoped rule shown there now actually
//! restricts container traffic too.
//!
//! **Which port these rules match.** `DOCKER-USER` is reached from
//! `filter/FORWARD`, which runs *after* `nat/PREROUTING` has already
//! DNAT-ed a published port to the container. A packet arriving at the
//! published `8080` of `-p 8080:80` therefore carries destination port `80`
//! by the time this chain sees it, and a rule written against `8080` never
//! matches - leaving the port unrestricted while the Firewall page says
//! otherwise. So a rule is written for the container's own port. Where the
//! two differ the external one is written as well, because a container on
//! `--network host`, or a port published out of band, is not DNAT-ed and is
//! still seen on the number it was published as. An extra DROP for a port
//! nothing arrives on costs nothing; a missing one costs the guarantee.
//!
//! **Established flows are let back first.** A `--dport` DROP would also
//! catch the return path of a connection the container itself opened, if
//! its source port happened to collide with a restricted one. A single
//! `ESTABLISHED,RELATED -j RETURN` at the head of the chain removes that
//! class of surprise, and costs one conntrack lookup.
//!
//! **Additive and self-scoped**, matching `firewall::ufw`'s stance: every
//! rule carries an iptables comment of `MARKER`, only rules carrying it are
//! ever removed, and nothing here flushes a chain wholesale or touches a
//! rule some other tool put in `DOCKER-USER`.
//!
//! **Through a helper, not `iptables` itself.** Every change goes through
//! `HELPER_PATH`, a root-owned script that takes the rule as parts - add or
//! delete, protocol, port, source - checks each, and builds the one
//! `iptables` command this module needs. That is what lets a team member be
//! given the firewall without being given root: `iptables` accepts
//! `--modprobe=<program>` and runs that program as root, and a sudo rule
//! cannot keep it out, because `*` in sudoers matches spaces - any pattern
//! with a wildcard for the port or the source also matches one with
//! `--modprobe` in the middle. The helper has no way to pass it on.

use crate::errors::{AppError, AppResult};
use crate::models::PortProtocol;
use crate::ssh::command;
use crate::ssh::SshSession;


/// Stamped on every rule this module adds, and the only thing that makes a
/// rule eligible for removal. An operator's own `DOCKER-USER` rules, and
/// anything another tool added, never carry it.
const MARKER: &str = "vibessh";

/// The helper every `DOCKER-USER` change goes through - see the module doc.
pub const HELPER_PATH: &str = "/usr/local/lib/vibessh/docker-user";

/// The helper's whole source, compared against the Node's copy and
/// installed again when they differ.
///
/// Each argument is checked before it reaches `iptables`: the protocol is
/// `tcp` or `udp`, the port is 1 to 65535, the source is made of the
/// characters an address and a prefix length use - none of which is `-`,
/// so none of them can start an option. Arguments go to `iptables` as
/// separate words, never through a shell.
pub const HELPER_SCRIPT: &str = r#"#!/bin/sh
# Installed by VibeSSH. The changes to DOCKER-USER it makes, and no others:
#   docker-user check | list
#   docker-user drop add|del <tcp|udp> <port> <source>
#   docker-user established add|del
set -eu
marker=vibessh
refuse() { echo "docker-user: $1" >&2; exit 2; }
case "${1:-}" in
  check)
    [ $# -eq 1 ] || refuse "check takes no arguments"
    exec iptables -n -L DOCKER-USER ;;
  list)
    [ $# -eq 1 ] || refuse "list takes no arguments"
    exec iptables -S DOCKER-USER ;;
  drop)
    [ $# -eq 5 ] || refuse "usage: drop add|del <tcp|udp> <port> <source>"
    case "$2" in add) flag=-I ;; del) flag=-D ;; *) refuse "drop takes add or del" ;; esac
    case "$3" in tcp|udp) ;; *) refuse "the protocol must be tcp or udp" ;; esac
    case "$4" in ''|*[!0-9]*) refuse "the port must be a number" ;; esac
    [ ${#4} -le 5 ] && [ "$4" -ge 1 ] && [ "$4" -le 65535 ] || refuse "the port must be 1 to 65535"
    case "$5" in ''|*[!0-9a-fA-F:./]*) refuse "the source must be an address or a range" ;; esac
    [ ${#5} -le 43 ] || refuse "the source is too long"
    exec iptables "$flag" DOCKER-USER -p "$3" --dport "$4" ! -s "$5" -m comment --comment "$marker" -j DROP ;;
  established)
    [ $# -eq 2 ] || refuse "usage: established add|del"
    case "$2" in
      add) exec iptables -I DOCKER-USER 1 -m conntrack --ctstate ESTABLISHED,RELATED -m comment --comment "$marker" -j RETURN ;;
      del) exec iptables -D DOCKER-USER -m conntrack --ctstate ESTABLISHED,RELATED -m comment --comment "$marker" -j RETURN ;;
      *) refuse "established takes add or del" ;;
    esac ;;
  *) refuse "usage: check | list | drop add|del <tcp|udp> <port> <source> | established add|del" ;;
esac
"#;

/// Sessions whose Node already has the current helper, so a reconcile does
/// not compare it on every call. Keyed on the session: a reconnected Node is
/// checked again. Bounded by clearing, like the file helper's.
static HELPER_VERIFIED: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<u64>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

/// Puts the helper in place when it is missing or out of date.
///
/// Read without `sudo` - it is world-readable, and a team member's account,
/// which may run it but not `cat` as root, has to be able to tell it is
/// already there. Installing does need root, which the owner's account has
/// and the access sync uses; a member finding it missing gets the install's
/// own refusal.
pub async fn ensure_helper_installed(connection: &SshSession) -> AppResult<()> {
    if HELPER_VERIFIED.lock().expect("docker-user helper mutex poisoned").contains(&connection.id()) {
        return Ok(());
    }
    let helper = command::quote(HELPER_PATH);
    let deployed = connection.execute_command(&format!("cat {helper} 2>/dev/null")).await?;
    if deployed.stdout != HELPER_SCRIPT {
        // Staged in the admin's own home over SFTP, then moved into place as
        // root: nothing passes through a world-writable directory (AGENTS.md 4).
        let staging = format!(".vibessh-docker-user-{}", uuid::Uuid::new_v4());
        connection.write_file(&staging, HELPER_SCRIPT.as_bytes()).await?;
        let staging = command::quote(&staging);
        let output = connection
            .execute_command(&format!("sudo install -D -o root -g root -m 0755 {staging} {helper}; rc=$?; rm -f {staging}; exit $rc"))
            .await?;
        if output.exit_code != 0 {
            return Err(AppError::Connection(format!("couldn't install the firewall helper: {}", output.stderr.trim())));
        }
    }
    let mut verified = HELPER_VERIFIED.lock().expect("docker-user helper mutex poisoned");
    if verified.len() >= 512 {
        verified.clear();
    }
    verified.insert(connection.id());
    Ok(())
}

/// `sudo <helper> <arguments...>`, every argument quoted.
fn helper_command(arguments: &[&str]) -> String {
    let arguments: Vec<String> = arguments.iter().map(|argument| command::quote(argument)).collect();
    format!("sudo {} {}", command::quote(HELPER_PATH), arguments.join(" "))
}

fn protocol_str(protocol: PortProtocol) -> &'static str {
    match protocol {
        PortProtocol::Tcp => "tcp",
        PortProtocol::Udp => "udp",
    }
}

/// `true` once this Node has a Docker daemon with the `DOCKER-USER` chain.
/// Absent means either no Docker or a version old enough not to create the
/// chain - in both cases there is nothing to restrict and nothing to do.
pub async fn detect(connection: &SshSession) -> AppResult<bool> {
    ensure_helper_installed(connection).await?;
    let output = connection.execute_command(&format!("{} >/dev/null 2>&1 && echo yes || echo no", helper_command(&["check"]))).await?;
    Ok(output.stdout.trim() == "yes")
}

/// The rule that restricts `rule.port` to `rule.source_cidr`.
///
/// `-I` (insert at the top), not `-A`: `DOCKER-USER` usually ends with a
/// blanket `RETURN`, so an appended rule would sit after it and never be
/// reached.
///
/// `! -s <cidr> -j DROP` rather than an ACCEPT for the allowed range,
/// because `DOCKER-USER` is a filter chain that falls through to Docker's
/// own ACCEPT: expressing this as "drop everything that isn't from the
/// allowed source" is the only form that actually denies anything.
fn drop_command(rule: &ContainerRule) -> String {
    helper_command(&["drop", "add", protocol_str(rule.protocol), &rule.port.to_string(), &rule.source_cidr])
}

/// Let the return path of an already-accepted flow through before any of
/// the `--dport` drops below it. Inserted at position 1 so it stays ahead of
/// them however many times this reconciles.
fn conntrack_return_command() -> String {
    helper_command(&["established", "add"])
}

fn conntrack_delete_command() -> String {
    helper_command(&["established", "del"])
}

/// Same rule shape with `-D` - iptables deletes by exact specification, so
/// this has to match `drop_command` field for field.
fn delete_command(rule: &ContainerRule) -> String {
    helper_command(&["drop", "del", protocol_str(rule.protocol), &rule.port.to_string(), &rule.source_cidr])
}

/// One port this chain should restrict, and to what.
///
/// Separate from `FirewallRule` because the two answer different questions.
/// A `FirewallRule` is what ufw is told, and ufw sees the published port. A
/// `ContainerRule` is what iptables is told, and after DNAT iptables sees
/// the container's own - see the module doc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerRule {
    pub port: u16,
    pub protocol: PortProtocol,
    pub source_cidr: String,
}

/// Every port that has to be restricted for one published, source-scoped
/// port: the container's own, plus the published one when it differs.
pub fn rules_for_published_port(internal_port: u16, external_port: u16, protocol: PortProtocol, source_cidr: &str) -> Vec<ContainerRule> {
    let mut rules = vec![ContainerRule { port: internal_port, protocol, source_cidr: source_cidr.to_string() }];
    if external_port != internal_port {
        rules.push(ContainerRule { port: external_port, protocol, source_cidr: source_cidr.to_string() });
    }
    rules
}

/// Parses `iptables -S DOCKER-USER` output back into the rules this module
/// owns. Only lines carrying `MARKER` are considered - see the module doc.
fn parse_owned_rules(output: &str) -> Vec<ContainerRule> {
    output
        .lines()
        .filter(|line| line.contains(&format!("--comment {MARKER}")) || line.contains(&format!("--comment \"{MARKER}\"")))
        .filter_map(|line| {
            let tokens: Vec<&str> = line.split_whitespace().collect();
            let after = |flag: &str| tokens.iter().position(|t| *t == flag).and_then(|i| tokens.get(i + 1)).copied();
            let protocol = match after("-p")? {
                "tcp" => PortProtocol::Tcp,
                "udp" => PortProtocol::Udp,
                _ => return None,
            };
            let port: u16 = after("--dport")?.parse().ok()?;
            // `iptables -S` renders the negated source as `! -s <cidr>`.
            let source_index = tokens.iter().position(|t| *t == "-s")?;
            let cidr = (*tokens.get(source_index + 1)?).to_string();
            Some(ContainerRule { port, protocol, source_cidr: cidr })
        })
        .collect()
}

/// Brings `DOCKER-USER` in line with `desired`, and returns how many rules
/// were added and removed.
///
/// No-op (and not an error) on a Node with no Docker: a Node that only runs
/// systemd or bare-process Applications has nothing for this to restrict.
pub async fn reconcile(connection: &SshSession, desired: &[ContainerRule]) -> AppResult<(usize, usize)> {
    if !detect(connection).await? {
        return Ok((0, 0));
    }

    let owned = read_owned(connection).await?;
    let wanted = dedupe(desired);

    let mut removed = 0;
    for rule in &owned {
        if wanted.contains(rule) {
            continue;
        }
        let output = connection.execute_command(&delete_command(rule)).await?;
        if output.exit_code == 0 {
            removed += 1;
        } else {
            log::warn!("couldn't remove a stale DOCKER-USER rule for {}/{}: {}", rule.port, protocol_str(rule.protocol), output.stderr.trim());
        }
    }

    let mut added = 0;
    for rule in &wanted {
        if owned.contains(rule) {
            continue;
        }
        let output = connection.execute_command(&drop_command(rule)).await?;
        if output.exit_code != 0 {
            let detail = output.stderr.trim();
            let detail = if detail.is_empty() { "iptables failed".to_string() } else { detail.to_string() };
            return Err(AppError::Connection(format!(
                "couldn't restrict container traffic on {}/{}: {detail}",
                rule.port,
                protocol_str(rule.protocol)
            )));
        }
        added += 1;
    }

    // Last, so that inserting it at position 1 leaves it above every drop
    // just written. Removed and re-added rather than checked, because its
    // position matters and a rule already present somewhere lower down
    // would not protect anything.
    if !wanted.is_empty() {
        let _ = connection.execute_command(&conntrack_delete_command()).await;
        let output = connection.execute_command(&conntrack_return_command()).await?;
        if output.exit_code != 0 {
            let detail = output.stderr.trim();
            let detail = if detail.is_empty() { "iptables failed".to_string() } else { detail.to_string() };
            return Err(AppError::Connection(format!("couldn't allow established container traffic back: {detail}")));
        }
    }

    Ok((added, removed))
}

/// The restrictions this module has actually put on the Node, read back
/// from the Node itself.
///
/// The Firewall page used to call a port protected on the strength of the
/// rule set VibeSSH *intended*, which is a different claim: a reconcile
/// that failed, or a chain someone flushed, leaves the intention intact and
/// the port open. Anything that wants to tell a user "protected" reads this.
pub async fn read_owned(connection: &SshSession) -> AppResult<Vec<ContainerRule>> {
    ensure_helper_installed(connection).await?;
    let listing = connection.execute_command(&helper_command(&["list"])).await?;
    if listing.exit_code != 0 {
        return Err(AppError::Connection("couldn't read the DOCKER-USER chain".into()));
    }
    Ok(parse_owned_rules(&listing.stdout))
}

fn dedupe(rules: &[ContainerRule]) -> Vec<ContainerRule> {
    let mut out: Vec<ContainerRule> = Vec::new();
    for rule in rules {
        if !out.contains(rule) {
            out.push(rule.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(port: u16, protocol: PortProtocol, cidr: &str) -> ContainerRule {
        ContainerRule { port, protocol, source_cidr: cidr.to_string() }
    }

    /// Every change goes through the helper, as separate quoted words - never
    /// `iptables` itself, which a narrowed account must not be able to run.
    #[test]
    fn every_change_goes_through_the_helper() {
        let r = rule(3306, PortProtocol::Tcp, "10.77.0.0/16");
        assert_eq!(drop_command(&r), "sudo '/usr/local/lib/vibessh/docker-user' 'drop' 'add' 'tcp' '3306' '10.77.0.0/16'");
        assert_eq!(delete_command(&r), "sudo '/usr/local/lib/vibessh/docker-user' 'drop' 'del' 'tcp' '3306' '10.77.0.0/16'");
        for command in [drop_command(&r), delete_command(&r), conntrack_return_command(), conntrack_delete_command()] {
            assert!(!command.contains("iptables"), "{command}");
        }
    }

    /// The rules the helper writes, as before: `-I` at the top of the chain,
    /// a negated-source DROP, the marker, and `-D` the exact inverse.
    #[test]
    fn the_helper_writes_the_same_rules_this_module_always_did() {
        assert!(HELPER_SCRIPT.contains(r#"add) flag=-I ;; del) flag=-D"#));
        assert!(HELPER_SCRIPT.contains(r#"exec iptables "$flag" DOCKER-USER -p "$3" --dport "$4" ! -s "$5" -m comment --comment "$marker" -j DROP"#));
        assert!(HELPER_SCRIPT.contains("add) exec iptables -I DOCKER-USER 1 -m conntrack --ctstate ESTABLISHED,RELATED"));
        assert!(HELPER_SCRIPT.contains("marker=vibessh"));
        assert_eq!(MARKER, "vibessh");
    }

    /// The helper runs as root on whatever a narrowed account passes it, so it
    /// has to parse, and has to check every part before it is used.
    #[test]
    fn the_helper_parses_and_checks_what_it_is_given() {
        if let Ok(status) = std::process::Command::new("sh").arg("-n").arg("-c").arg(HELPER_SCRIPT).status() {
            assert!(status.success(), "the helper does not parse");
        }
        assert!(HELPER_SCRIPT.contains("tcp|udp)"), "the protocol is not checked");
        assert!(HELPER_SCRIPT.contains("*[!0-9]*) refuse"), "the port is not checked");
        assert!(HELPER_SCRIPT.contains("*[!0-9a-fA-F:./]*) refuse"), "the source is not checked");
        assert!(!HELPER_SCRIPT.contains("eval"), "nothing may be re-parsed by a shell");
    }

    /// The bug this module had: `DOCKER-USER` is reached after DNAT, so a
    /// rule written against the published port matches nothing on any
    /// mapping where the two differ - leaving the port open while the
    /// interface called it protected.
    #[test]
    fn a_remapped_port_is_restricted_on_the_number_the_packet_actually_carries() {
        let rules = rules_for_published_port(80, 8080, PortProtocol::Tcp, "10.77.0.0/16");
        assert!(rules.iter().any(|r| r.port == 80), "the container's own port has to be covered: {rules:?}");
        // And the published one too: a host-networked or out-of-band
        // container is not DNAT-ed and is seen on that number instead.
        assert!(rules.iter().any(|r| r.port == 8080), "{rules:?}");
    }

    /// The common case, where nothing is remapped, must not produce the
    /// same rule twice - iptables would happily insert both.
    #[test]
    fn an_unmapped_port_produces_exactly_one_rule() {
        let rules = rules_for_published_port(3306, 3306, PortProtocol::Tcp, "10.77.0.0/16");
        assert_eq!(rules.len(), 1, "{rules:?}");
    }

    /// The return path of a flow the container itself opened must not be
    /// caught by a `--dport` drop, and the accept has to sit above them.
    #[test]
    fn established_traffic_is_returned_from_the_head_of_the_chain() {
        assert_eq!(conntrack_return_command(), "sudo '/usr/local/lib/vibessh/docker-user' 'established' 'add'");
        assert_eq!(conntrack_delete_command(), "sudo '/usr/local/lib/vibessh/docker-user' 'established' 'del'");
    }

    #[test]
    fn parse_owned_rules_reads_back_what_drop_command_writes() {
        let output = concat!(
            "-N DOCKER-USER\n",
            "-A DOCKER-USER -p tcp -m tcp --dport 3306 ! -s 10.77.0.0/16 -m comment --comment vibessh -j DROP\n",
            "-A DOCKER-USER -p udp -m udp --dport 24454 ! -s 10.77.0.0/16 -m comment --comment vibessh -j DROP\n",
            "-A DOCKER-USER -j RETURN\n",
        );
        let owned = parse_owned_rules(output);
        assert_eq!(owned.len(), 2);
        assert_eq!(owned[0].port, 3306);
        assert_eq!(owned[0].protocol, PortProtocol::Tcp);
        assert_eq!(owned[0].source_cidr, "10.77.0.0/16");
        assert_eq!(owned[1].protocol, PortProtocol::Udp);
    }

    /// The whole safety property of this module: a rule somebody else put
    /// in `DOCKER-USER` must never be considered ours, and therefore never
    /// removed.
    #[test]
    fn parse_owned_rules_ignores_rules_this_module_didnt_add() {
        let output = concat!(
            "-N DOCKER-USER\n",
            "-A DOCKER-USER -p tcp -m tcp --dport 8080 ! -s 192.168.0.0/16 -j DROP\n",
            "-A DOCKER-USER -s 172.17.0.0/16 -j ACCEPT\n",
            "-A DOCKER-USER -j RETURN\n",
        );
        assert!(parse_owned_rules(output).is_empty());
    }

    #[test]
    fn parse_owned_rules_on_an_empty_chain_is_empty() {
        assert!(parse_owned_rules("-N DOCKER-USER\n-A DOCKER-USER -j RETURN\n").is_empty());
        assert!(parse_owned_rules("").is_empty());
    }
}
