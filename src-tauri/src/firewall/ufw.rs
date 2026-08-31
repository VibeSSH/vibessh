//! `UfwProvider` - the only real `FirewallProvider` backend so far (Etap
//! M2), chosen first because it's the default/most common firewall
//! frontend on the Debian/Ubuntu-family VPS images this project's own
//! blueprints target. `firewalld`/`nftables` are later, additive backends
//! behind the same trait - nothing here assumes ufw is the only one that
//! will ever exist.
//!
//! Every rule this module adds carries `comment 'vibessh'` - not read back
//! anywhere yet (`ufw status` doesn't surface comments, see `firewall::mod`'s
//! own doc comment on why removal isn't attempted this phase), but it's
//! cheap, harmless, and exactly the kind of marker a future removal story
//! would need, so it's written from day one rather than retrofitted later.

use crate::errors::{AppError, AppResult};
use crate::models::PortProtocol;
use crate::ssh::SshSession;

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
        Some(cidr) => format!("sudo ufw allow from {cidr} to any port {} proto {} comment 'vibessh'", rule.port, protocol_str(rule.protocol)),
    }
}

#[async_trait::async_trait]
impl FirewallProvider for UfwProvider {
    fn name(&self) -> &'static str {
        "ufw"
    }

    async fn apply_rules(&self, connection: &SshSession, desired: &[FirewallRule]) -> AppResult<()> {
        for rule in desired {
            let output = connection.execute_command(&allow_command(rule)).await?;
            if output.exit_code != 0 {
                let detail = output.stderr.trim();
                let detail = if detail.is_empty() { "ufw allow failed".to_string() } else { detail.to_string() };
                return Err(AppError::Connection(format!(
                    "couldn't allow {}/{} through ufw: {detail}",
                    rule.port,
                    protocol_str(rule.protocol)
                )));
            }
        }
        Ok(())
    }

    async fn current_rules(&self, connection: &SshSession) -> AppResult<Vec<FirewallRule>> {
        let output = connection.execute_command("sudo ufw status").await?;
        Ok(parse_status_rules(&output.stdout))
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
        assert_eq!(allow_command(&scoped), "sudo ufw allow from 10.77.0.0/16 to any port 25565 proto tcp comment 'vibessh'");
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
}
