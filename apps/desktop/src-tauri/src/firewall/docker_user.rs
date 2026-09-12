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

use crate::errors::{AppError, AppResult};
use crate::models::PortProtocol;
use crate::ssh::command;
use crate::ssh::SshSession;


/// Stamped on every rule this module adds, and the only thing that makes a
/// rule eligible for removal. An operator's own `DOCKER-USER` rules, and
/// anything another tool added, never carry it.
const MARKER: &str = "vibessh";

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
    let output = connection.execute_command("sudo iptables -n -L DOCKER-USER >/dev/null 2>&1 && echo yes || echo no").await?;
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
    format!(
        "sudo iptables -I DOCKER-USER -p {proto} --dport {port} ! -s {cidr} -m comment --comment {marker} -j DROP",
        proto = protocol_str(rule.protocol),
        port = rule.port,
        cidr = command::quote(&rule.source_cidr),
        marker = command::quote(MARKER),
    )
}

/// Let the return path of an already-accepted flow through before any of
/// the `--dport` drops below it. Inserted at position 1 so it stays ahead of
/// them however many times this reconciles.
fn conntrack_return_command() -> String {
    format!(
        "sudo iptables -I DOCKER-USER 1 -m conntrack --ctstate ESTABLISHED,RELATED -m comment --comment {marker} -j RETURN",
        marker = command::quote(MARKER),
    )
}

fn conntrack_delete_command() -> String {
    format!(
        "sudo iptables -D DOCKER-USER -m conntrack --ctstate ESTABLISHED,RELATED -m comment --comment {marker} -j RETURN",
        marker = command::quote(MARKER),
    )
}

/// Same rule shape with `-D` - iptables deletes by exact specification, so
/// this has to match `drop_command` field for field.
fn delete_command(rule: &ContainerRule) -> String {
    format!(
        "sudo iptables -D DOCKER-USER -p {proto} --dport {port} ! -s {cidr} -m comment --comment {marker} -j DROP",
        proto = protocol_str(rule.protocol),
        port = rule.port,
        cidr = command::quote(&rule.source_cidr),
        marker = command::quote(MARKER),
    )
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
    let listing = connection.execute_command("sudo iptables -S DOCKER-USER").await?;
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

    /// `-I` not `-A`: `DOCKER-USER` normally ends in a blanket `RETURN`, so
    /// an appended rule would never be evaluated.
    #[test]
    fn drop_command_inserts_at_the_top_of_the_chain() {
        let command = drop_command(&rule(3306, PortProtocol::Tcp, "10.77.0.0/16"));
        assert!(command.contains("-I DOCKER-USER"), "{command}");
        assert!(!command.contains("-A DOCKER-USER"), "{command}");
    }

    /// The rule has to be a negated-source DROP. An ACCEPT for the allowed
    /// range would deny nothing, because the chain falls through to
    /// Docker's own ACCEPT.
    #[test]
    fn drop_command_denies_everything_outside_the_allowed_source() {
        let command = drop_command(&rule(3306, PortProtocol::Tcp, "10.77.0.0/16"));
        assert!(command.contains("! -s '10.77.0.0/16'"), "{command}");
        assert!(command.contains("-j DROP"), "{command}");
        assert!(command.contains("--dport 3306"), "{command}");
        assert!(command.contains("-p tcp"), "{command}");
    }

    /// iptables deletes by exact specification, so any drift between the
    /// two would leave rules that can never be removed.
    #[test]
    fn delete_command_matches_drop_command_exactly_apart_from_the_verb() {
        let r = rule(24454, PortProtocol::Udp, "10.77.0.0/16");
        assert_eq!(drop_command(&r).replace("-I", "-D"), delete_command(&r));
    }

    #[test]
    fn every_rule_carries_the_ownership_marker() {
        let r = rule(3306, PortProtocol::Tcp, "10.77.0.0/16");
        assert!(drop_command(&r).contains(&format!("--comment '{MARKER}'")));
        assert!(delete_command(&r).contains(&format!("--comment '{MARKER}'")));
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
        let command = conntrack_return_command();
        assert!(command.contains("-I DOCKER-USER 1"), "{command}");
        assert!(command.contains("ESTABLISHED,RELATED"), "{command}");
        assert!(command.contains("-j RETURN"), "{command}");
        assert_eq!(command.replace("-I DOCKER-USER 1", "-D DOCKER-USER"), conntrack_delete_command());
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
