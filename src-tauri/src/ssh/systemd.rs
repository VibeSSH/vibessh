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
        Ok(parse_units(units_section, files_section))
    }

    pub async fn restart_service(&self, service_name: &str) -> AppResult<()> {
        self.run_systemctl("restart", service_name).await
    }

    pub async fn start_service(&self, service_name: &str) -> AppResult<()> {
        self.run_systemctl("start", service_name).await
    }

    pub async fn stop_service(&self, service_name: &str) -> AppResult<()> {
        self.run_systemctl("stop", service_name).await
    }

    /// `--now` starts it in the same round trip - "enable" alone would leave
    /// the unit stopped until the next boot, which isn't what a user
    /// clicking "Enable" on a currently-inspected unit expects.
    pub async fn enable_service(&self, service_name: &str) -> AppResult<()> {
        self.run_systemctl("enable --now", service_name).await
    }

    pub async fn disable_service(&self, service_name: &str) -> AppResult<()> {
        self.run_systemctl("disable --now", service_name).await
    }

    async fn run_systemctl(&self, action: &str, service_name: &str) -> AppResult<()> {
        validate_unit_name(service_name)?;
        let verb = action.split_whitespace().next().unwrap_or(action);
        let output = self.execute_command(&format!("systemctl {action} {service_name}")).await?;
        if output.exit_code != 0 {
            let detail = output.stderr.trim();
            let detail = if detail.is_empty() { "systemctl exited with an error".to_string() } else { detail.to_string() };
            return Err(AppError::Connection(format!("couldn't {verb} {service_name}: {detail}")));
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

/// `systemctl list-units --all` only shows units systemd currently has
/// *loaded* - an inactive oneshot unit (or really any unit systemd decides
/// to garbage-collect once nothing references it) drops out of that list
/// entirely, `--all` notwithstanding, even though its unit file is still
/// right there on disk. `list-unit-files` is the complete, load-state-
/// independent universe of every unit systemd knows about from a file, so
/// this builds the result from *that* list and enriches each entry with
/// load/active state and description where `list-units` happens to have a
/// live entry for it - never the other way around, or an unloaded-but-real
/// unit would silently vanish from what the UI shows.
fn parse_units(units_section: &str, files_section: &str) -> Vec<ServiceSummary> {
    let loaded = parse_loaded_units(units_section);

    let mut services = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for line in files_section.lines() {
        let mut fields = line.split_whitespace();
        let (Some(name), Some(state)) = (fields.next(), fields.next()) else { continue };
        if !seen.insert(name.to_string()) {
            continue;
        }
        let (active, description) = loaded.get(name).cloned().unwrap_or((false, String::new()));
        services.push(ServiceSummary {
            name: name.to_string(),
            active,
            enabled: state.starts_with("enabled"),
            description,
        });
    }

    // A loaded unit with no backing file at all (transient units, some
    // generator-produced ones) still deserves to be listed - "not enabled"
    // is the honest fallback rather than dropping it.
    for (name, (active, description)) in loaded {
        if seen.insert(name.clone()) {
            services.push(ServiceSummary { name, active, enabled: false, description });
        }
    }

    services
}

/// `systemctl list-units --type=service --all --no-legend --plain` lines:
/// `unit.service loaded active running Some description text`. The
/// description is free text with spaces, so it's everything after the
/// fourth field, not a fifth `split_whitespace` token. Returns (active,
/// description) per unit name.
fn parse_loaded_units(section: &str) -> std::collections::HashMap<String, (bool, String)> {
    let mut loaded = std::collections::HashMap::new();
    for line in section.lines() {
        let mut fields = line.split_whitespace();
        let Some(name) = fields.next() else { continue };
        let Some(_load_state) = fields.next() else { continue };
        let Some(active_state) = fields.next() else { continue };
        let Some(_sub_state) = fields.next() else { continue };
        let description = fields.collect::<Vec<_>>().join(" ");
        loaded.insert(name.to_string(), (active_state == "active", description));
    }
    loaded
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
        let services = parse_units(units_section, files_section);

        // nginx + mariadb + getty@.service (the template, from list-unit-files)
        // + getty@tty1.service (a live instance, loaded but not itself in
        // list-unit-files) = 4, not 3 - a unit that's real but not
        // currently loaded (unlike this fixture, but see the next test)
        // must never silently disappear just because list-units dropped it.
        assert_eq!(services.len(), 4);

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
        let getty_instance = services.iter().find(|s| s.name == "getty@tty1.service").unwrap();
        assert!(getty_instance.active);
        assert!(!getty_instance.enabled);

        let getty_template = services.iter().find(|s| s.name == "getty@.service").unwrap();
        assert!(!getty_template.enabled);
    }

    #[test]
    fn a_unit_that_is_real_but_not_currently_loaded_still_appears() {
        // The exact scenario that caught this: a oneshot unit gets started,
        // then stopped/disabled, and systemd garbage-collects it out of
        // `list-units --all` even though the file is still on disk and
        // `list-unit-files` still reports it.
        let output = concat!(
            "===UNITS===\n",
            "===FILES===\n",
            "vibessh-check.service                       disabled\n",
        );
        let (units_section, files_section) = split_two_sections(output);
        let services = parse_units(units_section, files_section);

        assert_eq!(services.len(), 1);
        let unit = &services[0];
        assert_eq!(unit.name, "vibessh-check.service");
        assert!(!unit.active, "an unloaded unit must report inactive, not vanish");
        assert!(!unit.enabled);
    }
}
