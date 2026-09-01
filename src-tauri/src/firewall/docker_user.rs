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
//! **Additive and self-scoped**, matching `firewall::ufw`'s stance: every
//! rule carries an iptables comment of `MARKER`, only rules carrying it are
//! ever removed, and nothing here flushes a chain wholesale or touches a
//! rule some other tool put in `DOCKER-USER`.

use crate::errors::{AppError, AppResult};
use crate::models::PortProtocol;
use crate::ssh::command;
use crate::ssh::SshSession;

use super::FirewallRule;

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
fn drop_command(rule: &FirewallRule, cidr: &str) -> String {
    format!(
        "sudo iptables -I DOCKER-USER -p {proto} --dport {port} ! -s {cidr} -m comment --comment {marker} -j DROP",
        proto = protocol_str(rule.protocol),
        port = rule.port,
        cidr = command::quote(cidr),
        marker = command::quote(MARKER),
    )
}

/// Same rule shape with `-D` - iptables deletes by exact specification, so
/// this has to match `drop_command` field for field.
fn delete_command(rule: &FirewallRule, cidr: &str) -> String {
    format!(
        "sudo iptables -D DOCKER-USER -p {proto} --dport {port} ! -s {cidr} -m comment --comment {marker} -j DROP",
        proto = protocol_str(rule.protocol),
        port = rule.port,
        cidr = command::quote(cidr),
        marker = command::quote(MARKER),
    )
}

/// Only source-scoped rules mean anything here. A rule with no
/// `source_cidr` is "reachable from anywhere", which is what Docker already
/// does - adding a rule for it would be a no-op that still has to be
/// reconciled later.
fn scoped_rules(desired: &[FirewallRule]) -> Vec<(&FirewallRule, &str)> {
    desired.iter().filter_map(|rule| rule.source_cidr.as_deref().map(|cidr| (rule, cidr))).collect()
}

/// Parses `iptables -S DOCKER-USER` output back into the rules this module
/// owns. Only lines carrying `MARKER` are considered - see the module doc.
fn parse_owned_rules(output: &str) -> Vec<(FirewallRule, String)> {
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
            Some((FirewallRule { port, protocol, source_cidr: Some(cidr.clone()) }, cidr))
        })
        .collect()
}

/// Brings `DOCKER-USER` in line with `desired`, and returns how many rules
/// were added and removed.
///
/// No-op (and not an error) on a Node with no Docker: a Node that only runs
/// systemd or bare-process Applications has nothing for this to restrict.
pub async fn reconcile(connection: &SshSession, desired: &[FirewallRule]) -> AppResult<(usize, usize)> {
    if !detect(connection).await? {
        return Ok((0, 0));
    }

    let listing = connection.execute_command("sudo iptables -S DOCKER-USER").await?;
    if listing.exit_code != 0 {
        return Err(AppError::Connection("couldn't read the DOCKER-USER chain".into()));
    }
    let owned = parse_owned_rules(&listing.stdout);
    let wanted = scoped_rules(desired);

    let mut removed = 0;
    for (rule, cidr) in &owned {
        if wanted.iter().any(|(w, wc)| w.port == rule.port && w.protocol == rule.protocol && *wc == cidr.as_str()) {
            continue;
        }
        let output = connection.execute_command(&delete_command(rule, cidr)).await?;
        if output.exit_code == 0 {
            removed += 1;
        } else {
            log::warn!("couldn't remove a stale DOCKER-USER rule for {}/{}: {}", rule.port, protocol_str(rule.protocol), output.stderr.trim());
        }
    }

    let mut added = 0;
    for (rule, cidr) in &wanted {
        if owned.iter().any(|(o, oc)| o.port == rule.port && o.protocol == rule.protocol && oc == cidr) {
            continue;
        }
        let output = connection.execute_command(&drop_command(rule, cidr)).await?;
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
    Ok((added, removed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(port: u16, protocol: PortProtocol, cidr: Option<&str>) -> FirewallRule {
        FirewallRule { port, protocol, source_cidr: cidr.map(str::to_string) }
    }

    /// `-I` not `-A`: `DOCKER-USER` normally ends in a blanket `RETURN`, so
    /// an appended rule would never be evaluated.
    #[test]
    fn drop_command_inserts_at_the_top_of_the_chain() {
        let command = drop_command(&rule(3306, PortProtocol::Tcp, None), "10.77.0.0/16");
        assert!(command.contains("-I DOCKER-USER"), "{command}");
        assert!(!command.contains("-A DOCKER-USER"), "{command}");
    }

    /// The rule has to be a negated-source DROP. An ACCEPT for the allowed
    /// range would deny nothing, because the chain falls through to
    /// Docker's own ACCEPT.
    #[test]
    fn drop_command_denies_everything_outside_the_allowed_source() {
        let command = drop_command(&rule(3306, PortProtocol::Tcp, None), "10.77.0.0/16");
        assert!(command.contains("! -s '10.77.0.0/16'"), "{command}");
        assert!(command.contains("-j DROP"), "{command}");
        assert!(command.contains("--dport 3306"), "{command}");
        assert!(command.contains("-p tcp"), "{command}");
    }

    /// iptables deletes by exact specification, so any drift between the
    /// two would leave rules that can never be removed.
    #[test]
    fn delete_command_matches_drop_command_exactly_apart_from_the_verb() {
        let r = rule(24454, PortProtocol::Udp, None);
        assert_eq!(drop_command(&r, "10.77.0.0/16").replace("-I", "-D"), delete_command(&r, "10.77.0.0/16"));
    }

    #[test]
    fn every_rule_carries_the_ownership_marker() {
        let r = rule(3306, PortProtocol::Tcp, None);
        assert!(drop_command(&r, "10.77.0.0/16").contains(&format!("--comment '{MARKER}'")));
        assert!(delete_command(&r, "10.77.0.0/16").contains(&format!("--comment '{MARKER}'")));
    }

    /// An unrestricted rule is what Docker already does - adding one would
    /// be a no-op that still needs reconciling forever.
    #[test]
    fn only_source_scoped_rules_produce_a_docker_user_rule() {
        let desired = vec![
            rule(25565, PortProtocol::Tcp, None),
            rule(3306, PortProtocol::Tcp, Some("10.77.0.0/16")),
            rule(24454, PortProtocol::Udp, Some("10.77.0.0/16")),
        ];
        let scoped = scoped_rules(&desired);
        assert_eq!(scoped.len(), 2);
        assert!(scoped.iter().all(|(r, _)| r.port != 25565));
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
        assert_eq!(owned[0].0.port, 3306);
        assert_eq!(owned[0].0.protocol, PortProtocol::Tcp);
        assert_eq!(owned[0].1, "10.77.0.0/16");
        assert_eq!(owned[1].0.protocol, PortProtocol::Udp);
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
