//! systemd service control over plain SSH. Unlike the agent's own systemd
//! support (Etap G), this needs no polkit rule or unit allowlist: the SSH
//! session already runs as whatever user the connection authenticated as,
//! so `systemctl restart <unit>` here has exactly the privileges that user
//! would have typing the same command by hand - there's no separate
//! elevation mechanism sitting in front of it to secure. What *does* need
//! guarding is that a service name reaches a remote shell command at all -
//! see `validate_unit_name`.

use vibessh_protocol::ServiceSummary;

use super::client::SshSession;
use crate::errors::{AppError, AppResult};

const LIST_COMMAND: &str = r#"echo ===UNITS===
systemctl list-units --type=service --all --no-legend --no-pager --plain
echo ===FILES===
systemctl list-unit-files --type=service --no-legend --no-pager --plain"#;

impl SshSession {
    pub async fn list_services(&self) -> AppResult<Vec<ServiceSummary>> {
        let output = self.execute_command(LIST_COMMAND).await?;
        let (units_section, files_section) = split_two_sections(&output.stdout);
        let enabled = parse_unit_files(files_section);
        Ok(parse_units(units_section, &enabled))
    }

    pub async fn restart_service(&self, service_name: &str) -> AppResult<()> {
        validate_unit_name(service_name)?;
        let output = self.execute_command(&format!("systemctl restart {service_name}")).await?;
        if output.exit_code != 0 {
            let detail = output.stderr.trim();
            let detail = if detail.is_empty() { "systemctl exited with an error".to_string() } else { detail.to_string() };
            return Err(AppError::Connection(format!("couldn't restart {service_name}: {detail}")));
        }
        Ok(())
    }
}

/// systemd unit names are restricted to a known character set (see
/// systemd.unit(5)) - checking against it before the name ever reaches a
/// remote shell command means there's no string this function accepts that
/// could smuggle in a second command (`;`, backticks, `$(...)`, quotes,
/// whitespace are all rejected). Scoped to `.service` units specifically,
/// matching what this module lists and restarts.
fn validate_unit_name(name: &str) -> AppResult<()> {
    let chars_ok = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '-' | '_' | '.' | '@'));
    if !chars_ok || !name.ends_with(".service") {
        return Err(AppError::InvalidInput(format!("'{name}' isn't a valid systemd service unit name")));
    }
    Ok(())
}

fn split_two_sections(output: &str) -> (&str, &str) {
    let after_units = output.split_once("===UNITS===").map(|(_, rest)| rest).unwrap_or(output);
    match after_units.split_once("===FILES===") {
        Some((units, files)) => (units, files),
        None => (after_units, ""),
    }
}

/// `systemctl list-unit-files --type=service --no-legend --plain` lines:
/// `unit.service   enabled` / `disabled` / `static` / `masked` / etc. Only
/// `enabled` (and `enabled-runtime`) count as "will start on boot" -
/// everything else, `static` included, is not something a human would call
/// "enabled".
fn parse_unit_files(section: &str) -> std::collections::HashMap<String, bool> {
    let mut enabled = std::collections::HashMap::new();
    for line in section.lines() {
        let mut fields = line.split_whitespace();
        let (Some(name), Some(state)) = (fields.next(), fields.next()) else { continue };
        enabled.insert(name.to_string(), state.starts_with("enabled"));
    }
    enabled
}

/// `systemctl list-units --type=service --all --no-legend --plain` lines:
/// `unit.service loaded active running Some description text`. The
/// description is free text with spaces, so it's everything after the
/// fourth field, not a fifth `split_whitespace` token.
fn parse_units(section: &str, enabled: &std::collections::HashMap<String, bool>) -> Vec<ServiceSummary> {
    let mut services = Vec::new();
    for line in section.lines() {
        let mut fields = line.split_whitespace();
        let Some(name) = fields.next() else { continue };
        let Some(_load_state) = fields.next() else { continue };
        let Some(active_state) = fields.next() else { continue };
        let Some(_sub_state) = fields.next() else { continue };
        let description = fields.collect::<Vec<_>>().join(" ");

        services.push(ServiceSummary {
            name: name.to_string(),
            active: active_state == "active",
            enabled: enabled.get(name).copied().unwrap_or(false),
            description,
        });
    }
    services
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_shell_metacharacters_in_a_unit_name() {
        for bogus in ["nginx; rm -rf /", "nginx`whoami`", "nginx$(id)", "nginx && reboot", "", "nginx.socket"] {
            assert!(validate_unit_name(bogus).is_err(), "should have rejected {bogus:?}");
        }
    }

    #[test]
    fn accepts_realistic_unit_names() {
        for good in ["nginx.service", "mariadb.service", "getty@tty1.service", "docker.service"] {
            assert!(validate_unit_name(good).is_ok(), "should have accepted {good:?}");
        }
    }

    #[test]
    fn parses_a_realistic_combined_listing() {
        let output = concat!(
            "===UNITS===\n",
            "nginx.service           loaded active   running Nginx web server\n",
            "mariadb.service         loaded inactive dead    MariaDB database server\n",
            "getty@tty1.service      loaded active   running Getty on tty1\n",
            "===FILES===\n",
            "nginx.service                              enabled\n",
            "mariadb.service                             disabled\n",
            "getty@.service                               static\n",
        );
        let (units_section, files_section) = split_two_sections(output);
        let enabled = parse_unit_files(files_section);
        let services = parse_units(units_section, &enabled);

        assert_eq!(services.len(), 3);

        let nginx = services.iter().find(|s| s.name == "nginx.service").unwrap();
        assert!(nginx.active);
        assert!(nginx.enabled);
        assert_eq!(nginx.description, "Nginx web server");

        let mariadb = services.iter().find(|s| s.name == "mariadb.service").unwrap();
        assert!(!mariadb.active);
        assert!(!mariadb.enabled);

        // getty@tty1.service isn't itself in the unit-files listing (only
        // its template getty@.service is) - it should fall back to "not
        // enabled" rather than erroring or panicking on a missing key.
        let getty = services.iter().find(|s| s.name == "getty@tty1.service").unwrap();
        assert!(getty.active);
        assert!(!getty.enabled);
    }
}
