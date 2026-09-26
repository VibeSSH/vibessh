//! `UfwProvider` - the only real `FirewallProvider` backend so far (Etap
//! M2), chosen first because it's the default/most common firewall
//! frontend on the Debian/Ubuntu-family VPS images this project's own
//! blueprints target. `firewalld`/`nftables` are later, additive backends
//! behind the same trait - nothing here assumes ufw is the only one that
//! will ever exist.
//!
//! Every rule this module adds carries `comment 'vibessh'` - written from
//! day one as the marker `vibessh_owned_rules` needs to tell "a rule this
//! code added" apart from "a rule the host's own admin added by hand" (see
//! `firewall::mod`'s own doc comment on why that distinction is the whole
//! safety story behind removal). Read back through `sudo ufw show added`,
//! not `sudo ufw status` - `status` prints the comment too on this ufw
//! version (confirmed live: `# vibessh` trails each rule's row), but that's
//! not guaranteed across every ufw version this project might run against,
//! while `show added` reprints the exact `ufw allow ...` invocation
//! (comment included) that created each rule - the same shape
//! `allow_command` builds, so parsing it back is a near-mirror of building
//! it, not a second format to keep in sync by hand.

use crate::errors::{AppError, AppResult};
use crate::models::PortProtocol;
use crate::ssh::{command, SshSession};

use super::{FirewallProvider, FirewallRule};

pub struct UfwProvider;

impl UfwProvider {
    pub async fn detect(connection: &SshSession) -> AppResult<bool> {
        let output = connection.execute_command("command -v ufw >/dev/null 2>&1 && echo yes || echo no").await?;
        Ok(output.stdout.trim() == "yes")
    }
}

fn protocol_str(protocol: PortProtocol) -> &'static str {
    match protocol {
        PortProtocol::Tcp => "tcp",
        PortProtocol::Udp => "udp",
    }
}

/// `sudo ufw allow <port>/<proto>` for an anywhere rule, or `sudo ufw allow
/// from <cidr> to any port <port> proto <proto>` when `source_cidr` scopes
/// it (Etap M4 - a "Vibe Network only" Application port, or the mesh's own
/// WireGuard port). `sudo` unconditionally - a real run against an SSH-mode
/// Node connecting as a non-root user with passwordless sudo (rather than
/// root directly) proved this was missing: `ufw` refuses to run at all
/// without it ("ERROR: You need to be root to run this script"), and `sudo`
/// is a harmless no-op prefix when the connection already *is* root.
fn allow_command(rule: &FirewallRule) -> String {
    match &rule.source_cidr {
        None => format!("sudo ufw allow {}/{} comment 'vibessh'", rule.port, protocol_str(rule.protocol)),
        // Quoted: a custom rule's source is typed by the operator, and until
        // it was validated and quoted it reached this line as raw shell.
        Some(cidr) => format!("sudo ufw allow from {} to any port {} proto {} comment 'vibessh'", command::quote(cidr), rule.port, protocol_str(rule.protocol)),
    }
}

/// The exact inverse of `allow_command`, minus the comment - `ufw delete
/// <rule spec>` matches against the port/protocol/source shape a rule was
/// created with, not its comment, so the comment plays no part in deletion.
fn revoke_command(rule: &FirewallRule) -> String {
    match &rule.source_cidr {
        None => format!("sudo ufw delete allow {}/{}", rule.port, protocol_str(rule.protocol)),
        Some(cidr) => format!("sudo ufw delete allow from {} to any port {} proto {}", command::quote(cidr), rule.port, protocol_str(rule.protocol)),
    }
}

#[async_trait::async_trait]
impl FirewallProvider for UfwProvider {
    fn name(&self) -> &'static str {
        "ufw"
    }

    /// Applies every rule, then reports whatever failed.
    ///
    /// **Deliberately does not stop at the first failure.** It used to, and
    /// that was the wrong shape twice over: every rule after the failing
    /// one was silently never applied, and the caller only ever saw the
    /// first error, so an operator fixing one problem at a time had no idea
    /// how many remained.
    ///
    /// Continuing is safe here specifically because every rule is an
    /// `allow`. Failing to apply one leaves the Node *more* restricted, not
    /// less - the opposite of the usual partial-application hazard - so
    /// there is nothing to roll back, and getting the rest applied is
    /// strictly better than abandoning them.
    async fn apply_rules(&self, connection: &SshSession, desired: &[FirewallRule]) -> AppResult<()> {
        let mut failures = Vec::new();
        for rule in desired {
            match connection.execute_command(&allow_command(rule)).await {
                Ok(output) if output.exit_code == 0 => {}
                Ok(output) => {
                    let detail = output.stderr.trim();
                    let detail = if detail.is_empty() { "ufw allow failed".to_string() } else { detail.to_string() };
                    failures.push(format!("{}/{}: {detail}", rule.port, protocol_str(rule.protocol)));
                }
                // A transport failure means the remaining rules cannot be
                // attempted either - unlike a rejected rule, there is no
                // point continuing.
                Err(err) => return Err(err),
            }
        }
        if !failures.is_empty() {
            return Err(AppError::Connection(format!("couldn't apply {} firewall rule(s) - {}", failures.len(), failures.join("; "))));
        }
        Ok(())
    }

    async fn current_rules(&self, connection: &SshSession) -> AppResult<Vec<FirewallRule>> {
        let output = connection.execute_command("sudo ufw status").await?;
        Ok(parse_status_rules(&output.stdout))
    }

    async fn vibessh_owned_rules(&self, connection: &SshSession) -> AppResult<Vec<FirewallRule>> {
        let output = connection.execute_command("sudo ufw show added").await?;
        Ok(parse_added_rules(&output.stdout))
    }

    async fn revoke_rules(&self, connection: &SshSession, obsolete: &[FirewallRule]) -> AppResult<()> {
        for rule in obsolete {
            let output = connection.execute_command(&revoke_command(rule)).await?;
            if output.exit_code != 0 {
                let detail = output.stderr.trim();
                let detail = if detail.is_empty() { "ufw delete failed".to_string() } else { detail.to_string() };
                return Err(AppError::Connection(format!(
                    "couldn't remove the {}/{} ufw rule: {detail}",
                    rule.port,
                    protocol_str(rule.protocol)
                )));
            }
        }
        Ok(())
    }

    async fn is_active(&self, connection: &SshSession) -> AppResult<bool> {
        let output = connection.execute_command("sudo ufw status").await?;
        Ok(parse_is_active(&output.stdout))
    }

    /// Applies `desired` first, every time - never a separate step a caller
    /// could reorder or skip, which is what makes the "SSH port allowed
    /// before enforcement turns on" invariant hold by construction rather
    /// than by caller discipline. `--force` skips ufw's own interactive
    /// `Proceed with operation (y|n)?` prompt, which would otherwise hang
    /// forever with no terminal attached to answer it.
    async fn enable(&self, connection: &SshSession, desired: &[FirewallRule]) -> AppResult<()> {
        // Turning on a default-deny firewall over the very SSH session that
        // manages the Node is the one operation here that can end with
        // nobody able to reach the machine again. The desired rule set
        // always contains a rule for `Server::ssh_port` - but that is
        // VibeSSH's *stored* value, and it goes stale: an operator who
        // moved sshd to another port, or who reaches the Node through a
        // jump host or a forwarded port, has a live connection on a port
        // the rule set doesn't mention. Enabling then locks them out
        // permanently, with no undo and no way back in.
        //
        // So ask the Node which port this session actually arrived on and
        // refuse if nothing covers it. `$SSH_CONNECTION`'s fourth field is
        // the server-side port; when it is unset (an exec channel that
        // didn't inherit it), we refuse rather than guess.
        let live_port = live_ssh_port(connection).await?;
        let covered = desired
            .iter()
            .any(|rule| rule.port == live_port && rule.protocol == PortProtocol::Tcp && rule.source_cidr.is_none());
        if !covered {
            return Err(AppError::InvalidInput(format!(
                "refusing to enable the firewall: this SSH session is connected on port {live_port}, and no rule allows it.                  Enabling now would lock VibeSSH out of this Node. Update the Node's SSH port, or add a custom rule for {live_port}/tcp first."
            )));
        }

        self.apply_rules(connection, desired).await?;
        let output = connection.execute_command("sudo ufw --force enable").await?;
        if output.exit_code != 0 {
            let detail = output.stderr.trim();
            let detail = if detail.is_empty() { "ufw enable failed".to_string() } else { detail.to_string() };
            return Err(AppError::Connection(format!("couldn't enable ufw: {detail}")));
        }
        Ok(())
    }
}

/// `ufw status`'s first line is `Status: active` or `Status: inactive` -
/// nothing else in the output starts with `Status:`.
/// The server-side port this SSH session actually arrived on, straight
/// from the Node rather than from anything VibeSSH stored.
///
/// `$SSH_CONNECTION` is `<client ip> <client port> <server ip> <server
/// port>`. Erroring on an unparseable/absent value is deliberate: this only
/// feeds a safety check, and a check that silently degrades to "allow" is
/// worse than no check.
async fn live_ssh_port(connection: &SshSession) -> AppResult<u16> {
    let output = connection.execute_command("printf '%s' \"$SSH_CONNECTION\"").await?;
    parse_live_ssh_port(&output.stdout).ok_or_else(|| {
        AppError::Connection(
            "couldn't determine which port this SSH session is connected on, so enabling the firewall can't be done safely -              enable it manually on the Node once you've confirmed your SSH port is allowed"
                .into(),
        )
    })
}

fn parse_live_ssh_port(ssh_connection: &str) -> Option<u16> {
    ssh_connection.split_whitespace().nth(3)?.parse().ok()
}

fn parse_is_active(output: &str) -> bool {
    output.lines().next().map(str::trim) == Some("Status: active")
}

/// Parses the `To`/`Action`/`From` table `ufw status` prints, e.g.:
/// ```text
/// Status: active
///
/// To                         Action      From
/// --                         ------      ----
/// 22/tcp                     ALLOW       Anywhere
/// 25565/tcp                  ALLOW       Anywhere
/// ```
/// Only rows whose `To` column is a bare `<port>/<proto>` are recognized -
/// ufw also accepts named services (`OpenSSH`), port ranges, and IP-scoped
/// rules, none of which this reconcile path ever creates itself, so a row
/// that doesn't parse is silently skipped rather than treated as an error.
/// Source-scoped rules (Etap M4) are not distinguished here yet - this is
/// only used by a future "show current rules" view, never by the
/// reconcile path itself (which is purely additive, see `firewall::mod`'s
/// own doc comment), so under-reporting scope is not a correctness risk.
fn parse_status_rules(output: &str) -> Vec<FirewallRule> {
    output
        .lines()
        .filter_map(|line| {
            let to_column = line.split_whitespace().next()?;
            let (port, proto) = to_column.split_once('/')?;
            let port: u16 = port.parse().ok()?;
            let protocol = match proto {
                "tcp" => PortProtocol::Tcp,
                "udp" => PortProtocol::Udp,
                _ => return None,
            };
            Some(FirewallRule { port, protocol, source_cidr: None })
        })
        .collect()
}

/// Parses `sudo ufw show added` output, e.g.:
/// ```text
/// Added user rules (see 'ufw status' for running firewall):
/// ufw allow 22/tcp comment 'vibessh'
/// ufw allow from 10.77.0.0/16 to any port 8080 proto tcp comment 'vibessh'
/// ufw allow in on docker0 to any port 3306 proto tcp
/// ```
/// Only lines ending in `comment 'vibessh'` are considered - see
/// `firewall::mod`'s own doc comment on why that's the entire safety
/// property `revoke_rules` depends on. The last line above (a manually
/// added rule with no comment, e.g. `database_service`'s own docker0 MySQL
/// rule) is correctly never returned - it doesn't carry the marker, so this
/// module has no way to know it's safe to touch and must not try.
fn parse_added_rules(output: &str) -> Vec<FirewallRule> {
    output
        .lines()
        .filter(|line| line.trim_end().ends_with("comment 'vibessh'"))
        .filter_map(|line| {
            let tokens: Vec<&str> = line.split_whitespace().collect();
            if tokens.first() != Some(&"ufw") || tokens.get(1) != Some(&"allow") {
                return None;
            }
            if tokens.get(2) == Some(&"from") {
                let cidr = (*tokens.get(3)?).to_string();
                if tokens.get(4) != Some(&"to") || tokens.get(5) != Some(&"any") || tokens.get(6) != Some(&"port") || tokens.get(8) != Some(&"proto") {
                    return None;
                }
                let port: u16 = tokens.get(7)?.parse().ok()?;
                let protocol = match *tokens.get(9)? {
                    "tcp" => PortProtocol::Tcp,
                    "udp" => PortProtocol::Udp,
                    _ => return None,
                };
                Some(FirewallRule { port, protocol, source_cidr: Some(cidr) })
            } else {
                let (port, proto) = tokens.get(2)?.split_once('/')?;
                let port: u16 = port.parse().ok()?;
                let protocol = match proto {
                    "tcp" => PortProtocol::Tcp,
                    "udp" => PortProtocol::Udp,
                    _ => return None,
                };
                Some(FirewallRule { port, protocol, source_cidr: None })
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(port: u16, protocol: PortProtocol) -> FirewallRule {
        FirewallRule { port, protocol, source_cidr: None }
    }

    #[test]
    fn allow_command_includes_port_protocol_and_a_vibessh_comment() {
        let command = allow_command(&rule(25565, PortProtocol::Tcp));
        assert_eq!(command, "sudo ufw allow 25565/tcp comment 'vibessh'");
    }

    #[test]
    fn allow_command_scopes_to_a_source_cidr_when_set() {
        let scoped = FirewallRule { port: 25565, protocol: PortProtocol::Tcp, source_cidr: Some("10.77.0.0/16".to_string()) };
        assert_eq!(allow_command(&scoped), "sudo ufw allow from '10.77.0.0/16' to any port 25565 proto tcp comment 'vibessh'");
    }

    #[test]
    fn parse_is_active_reads_the_first_line_only() {
        assert!(parse_is_active("Status: active\n\nTo  Action  From"));
        assert!(!parse_is_active("Status: inactive\n"));
        assert!(!parse_is_active(""));
    }

    #[test]
    fn parse_status_rules_reads_realistic_ufw_status_output() {
        let output = "Status: active\n\nTo                         Action      From\n--                         ------      ----\n22/tcp                     ALLOW       Anywhere\n25565/tcp                  ALLOW       Anywhere\n24454/udp                  ALLOW       Anywhere\n";
        let rules = parse_status_rules(output);
        assert_eq!(rules, vec![rule(22, PortProtocol::Tcp), rule(25565, PortProtocol::Tcp), rule(24454, PortProtocol::Udp)]);
    }

    #[test]
    fn parse_status_rules_skips_rows_that_arent_a_bare_port_slash_protocol() {
        let output = "Status: active\n\nTo                         Action      From\nOpenSSH                    ALLOW       Anywhere\n80,443/tcp                 ALLOW       Anywhere\n";
        assert!(parse_status_rules(output).is_empty());
    }

    #[test]
    fn parse_status_rules_on_an_empty_or_inactive_status_is_empty() {
        assert!(parse_status_rules("Status: inactive\n").is_empty());
        assert!(parse_status_rules("").is_empty());
    }

    #[test]
    fn revoke_command_mirrors_allow_command_without_the_comment() {
        assert_eq!(revoke_command(&rule(25565, PortProtocol::Tcp)), "sudo ufw delete allow 25565/tcp");
        let scoped = FirewallRule { port: 8080, protocol: PortProtocol::Tcp, source_cidr: Some("10.77.0.0/16".to_string()) };
        assert_eq!(revoke_command(&scoped), "sudo ufw delete allow from '10.77.0.0/16' to any port 8080 proto tcp");
    }

    #[test]
    fn parse_added_rules_reads_a_realistic_show_added_listing() {
        let output = "Added user rules (see 'ufw status' for running firewall):\n\
ufw allow 22/tcp comment 'vibessh'\n\
ufw allow 54221/udp comment 'vibessh'\n\
ufw allow from 10.77.0.0/16 to any port 8080 proto tcp comment 'vibessh'\n\
ufw allow in on docker0 to any port 3306 proto tcp\n";
        let rules = parse_added_rules(output);
        assert_eq!(
            rules,
            vec![
                rule(22, PortProtocol::Tcp),
                FirewallRule { port: 54221, protocol: PortProtocol::Udp, source_cidr: None },
                FirewallRule { port: 8080, protocol: PortProtocol::Tcp, source_cidr: Some("10.77.0.0/16".to_string()) },
            ]
        );
    }

    #[test]
    fn parse_added_rules_never_includes_a_rule_without_the_vibessh_comment() {
        let output = "Added user rules (see 'ufw status' for running firewall):\nufw allow in on docker0 to any port 3306 proto tcp\nufw allow 9000/tcp\n";
        assert!(parse_added_rules(output).is_empty());
    }

    #[test]
    fn parse_added_rules_on_an_empty_listing_is_empty() {
        assert!(parse_added_rules("Added user rules (see 'ufw status' for running firewall):\n").is_empty());
        assert!(parse_added_rules("").is_empty());
    }

    /// `$SSH_CONNECTION`'s fourth field is the server-side port. Getting
    /// this wrong in either direction is dangerous: a false negative blocks
    /// a legitimate enable, a false positive locks the operator out.
    #[test]
    fn parse_live_ssh_port_reads_the_server_side_port() {
        assert_eq!(parse_live_ssh_port("203.0.113.5 51234 10.0.0.7 22"), Some(22));
        assert_eq!(parse_live_ssh_port("203.0.113.5 51234 10.0.0.7 2222
"), Some(2222));
        // IPv6 endpoints keep the same four-field shape.
        assert_eq!(parse_live_ssh_port("2001:db8::1 51234 2001:db8::2 22"), Some(22));
    }

    /// Anything it can't read must come back `None` so `live_ssh_port`
    /// errors - a safety check that silently degrades to "allow" is worse
    /// than no check.
    #[test]
    fn parse_live_ssh_port_refuses_to_guess() {
        for bad in ["", "   ", "203.0.113.5 51234 10.0.0.7", "203.0.113.5 51234 10.0.0.7 notaport", "203.0.113.5 51234 10.0.0.7 99999"] {
            assert_eq!(parse_live_ssh_port(bad), None, "{bad:?}");
        }
    }

    /// `parse_added_rules` reads output from a remote command, and what it
    /// returns is the *only* thing `revoke_obsolete_rules` will delete. A
    /// parser that invents a rule from a malformed line would delete a rule
    /// VibeSSH does not own; one that panics takes the firewall sync with it.
    mod parser_properties {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// Arbitrary bytes, including the shapes a hostile or simply
            /// broken `ufw` might emit.
            #[test]
            fn never_panics_on_arbitrary_output(output in "\\PC{0,300}") {
                let _ = parse_added_rules(&output);
            }

            /// The safety property `firewall::mod` documents: a line without
            /// the marker is never returned, so a rule the operator added by
            /// hand can never be revoked as "obsolete".
            #[test]
            fn a_line_without_the_marker_is_never_returned(port in 1u16..=65535, cidr in "[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}/[0-9]{1,2}") {
                let unmarked = format!("ufw allow {port}/tcp\nufw allow from {cidr} to any port {port} proto tcp");
                prop_assert!(parse_added_rules(&unmarked).is_empty(), "claimed ownership of {unmarked:?}");
            }

            /// Round trip against the command builder: whatever `allow_command`
            /// writes, this must read back as the same rule. These two are
            /// the write and read halves of one format, and nothing else
            /// checks that they agree.
            #[test]
            fn round_trips_with_the_command_this_module_writes(
                port in 1u16..=65535,
                udp in any::<bool>(),
                cidr in prop::option::of("(10|172|192)\\.[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}/[0-9]{1,2}"),
            ) {
                let protocol = if udp { PortProtocol::Udp } else { PortProtocol::Tcp };
                let original = FirewallRule { port, protocol, source_cidr: cidr };
                // `ufw show added` prints the command that was run, without
                // the `sudo` the module adds when it runs it - and without
                // the shell quoting, which the shell removed before ufw ever
                // saw its arguments.
                let mut line = allow_command(&original).replace("sudo ", "");
                if let Some(cidr) = &original.source_cidr {
                    line = line.replace(&command::quote(cidr), cidr);
                }
                let parsed = parse_added_rules(&line);
                prop_assert_eq!(parsed.len(), 1, "did not read back: {:?}", line);
                prop_assert_eq!(parsed[0].port, original.port);
                prop_assert_eq!(parsed[0].protocol, original.protocol);
                prop_assert_eq!(&parsed[0].source_cidr, &original.source_cidr);
            }
        }
    }

}
