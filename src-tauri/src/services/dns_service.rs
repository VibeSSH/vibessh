//! Etap M4/M5: Private DNS - a generated `/etc/hosts` fragment pushed to
//! every Vibe Network member, not a real DNS server (see this module's own
//! "v1 shape" note below). Two kinds of alias share one namespace:
//!
//! - **Node aliases** (`<slugified-name><suffix>`) - always present for
//!   every mesh member, computed on the fly from `Server::name`, never
//!   stored.
//! - **Service aliases** (`dns_records`, e.g. `db01.vibe`) - explicitly
//!   created by the user, one per Application.
//!
//! **The suffix is a configurable, global per-install setting**
//! (`storage::dns_config`/`state::DnsSuffixState`, `.vibe` by default -
//! never `.local`, which collides with mDNS), passed as a plain `&str`
//! into every function here that needs it rather than a module-level
//! constant, so `normalize_alias`/`node_alias`/`resolve_dns_view` stay
//! synchronous and unit-testable without a runtime (the caller, which does
//! have `State<DnsSuffixState>` access, resolves it once and passes it
//! down). **Changing the suffix is not retroactive**: an already-created
//! service alias (`dns_records.hostname`) keeps its exact stored hostname
//! forever - only Node aliases (always computed fresh) and any *new*
//! service alias pick up a changed suffix.
//!
//! **What makes a service alias survive moving its Application to another
//! Node**: the IP behind `db01.vibe` is resolved at render time via
//! `applications.server_id -> node_network_members.wireguard_ip`, never
//! baked into the `dns_records` row itself. Repointing `applications.server_id`
//! (a service migration) and re-running `sync_dns` is the whole story - no
//! DNS record ever needs to change.
//!
//! **v1 shape**: no real DNS server/consensus layer - this pushes a plain
//! `/etc/hosts`-managed block to every mesh member over SSH exec, replacing
//! whatever this module wrote there last time. A real resolver (dnsmasq,
//! CoreDNS) is a future, additive upgrade behind the same `DnsView`
//! data model, not a redesign.

use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::{DnsRecord, DnsView, DnsViewKind};
use crate::services::ssh_service::get_or_connect;
use crate::ssh::command;
use crate::state::SshSessionManager;
// The one shared implementation - every module that builds a remote
// command used to carry its own byte-identical copy of this.
use crate::ssh::command::quote as shell_quote;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::dns_repository::DnsRepository;
use crate::storage::node_network_repository::NodeNetworkRepository;
use crate::storage::server_repository::ServerRepository;

/// A suffix must be a plausible FQDN label chain: starts with `.`, at
/// least one more character after it, and only `[a-z0-9.-]` - the same
/// character set `slugify` itself ever produces, plus `.` as the label
/// separator. Rejects whitespace/newlines outright (this gets
/// string-interpolated into a shell heredoc via `push_fragment`, same
/// `reject_unsafe` concern as a hostname itself).
pub fn validate_dns_suffix(suffix: &str) -> AppResult<()> {
    if !suffix.starts_with('.') || suffix.len() < 2 {
        return Err(AppError::InvalidInput("the DNS suffix must start with '.' and have at least one character after it".into()));
    }
    if suffix.len() > 32 {
        return Err(AppError::InvalidInput("the DNS suffix is too long".into()));
    }
    if !suffix.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-') {
        return Err(AppError::InvalidInput("the DNS suffix can only contain lowercase letters, digits, '.', and '-'".into()));
    }
    Ok(())
}

const BEGIN_MARKER: &str = "# BEGIN VIBESSH-MANAGED-DNS";
const END_MARKER: &str = "# END VIBESSH-MANAGED-DNS";

/// Lowercases, replaces anything that isn't `[a-z0-9-]` with `-`, collapses
/// repeats, and trims leading/trailing `-` - the same treatment a Node's
/// own name (arbitrary user text) and a user-typed alias both need before
/// either is safe to use as a hostname or to interpolate into a shell
/// heredoc. Never empty: an all-symbol input becomes `"node"`. Capped at 63
/// characters - RFC 1123's own limit for a single DNS label - so a long
/// Node/Application name can't produce a hostname real DNS/`/etc/hosts`
/// rejects; every char actually pushed here is single-byte ASCII
/// (alphanumeric or `-`), so `result.len()` is a safe stand-in for a char
/// count when checking the cap.
fn slugify(input: &str) -> String {
    let mut result = String::with_capacity(input.len().min(63));
    let mut last_was_dash = false;
    for ch in input.chars().flat_map(char::to_lowercase) {
        if result.len() >= 63 {
            break;
        }
        if ch.is_ascii_alphanumeric() {
            result.push(ch);
            last_was_dash = false;
        } else if !last_was_dash && !result.is_empty() {
            result.push('-');
            last_was_dash = true;
        }
    }
    while result.ends_with('-') {
        result.pop();
    }
    if result.is_empty() {
        "node".to_string()
    } else {
        result
    }
}

/// Slugifies and appends `suffix` if not already present - `"db01"` and
/// `"db01.vibe"` (and `"DB01!!"`) all normalize to the exact same stored
/// hostname when `suffix` is `.vibe`, so a user typing either the bare
/// alias or the full name gets the same, predictable result.
pub fn normalize_alias(suffix: &str, input: &str) -> String {
    let trimmed = input.trim().to_ascii_lowercase();
    let base = trimmed.strip_suffix(suffix).unwrap_or(&trimmed);
    format!("{}{suffix}", slugify(base))
}

fn node_alias(suffix: &str, server_name: &str) -> String {
    format!("{}{suffix}", slugify(server_name))
}

/// Every alias a mesh member's `/etc/hosts` should carry - every current
/// member's own Node alias, plus every service alias whose Application
/// currently lives on a mesh member (a service alias for an Application on
/// a Node that hasn't joined the mesh, or with no `server_id` at all,
/// simply can't resolve to anything yet and is skipped, not an error).
pub fn resolve_dns_view(
    suffix: &str,
    network_repo: &NodeNetworkRepository,
    server_repo: &ServerRepository,
    app_repo: &ApplicationRepository,
    dns_repo: &DnsRepository,
) -> AppResult<Vec<DnsView>> {
    let members = network_repo.list()?;
    let mut views = Vec::new();

    for member in &members {
        let server = server_repo.get(member.server_id)?.ok_or_else(|| AppError::NotFound(format!("server {}", member.server_id)))?;
        views.push(DnsView { hostname: node_alias(suffix, &server.name), ip: member.wireguard_ip.clone(), kind: DnsViewKind::Node, server_id: member.server_id });
    }

    for record in dns_repo.list()? {
        let Some(application) = app_repo.get(record.application_id).ok().flatten() else { continue };
        let Some(server_id) = application.application.server_id else { continue };
        let Some(member) = members.iter().find(|m| m.server_id == server_id) else { continue };
        views.push(DnsView { hostname: record.hostname, ip: member.wireguard_ip.clone(), kind: DnsViewKind::Service { application_id: record.application_id }, server_id });
    }

    Ok(views)
}

/// A raw newline or the heredoc's own delimiter smuggled into a hostname
/// could inject extra `/etc/hosts` lines or shell statements - rejected
/// outright, same stance `network::wireguard::reject_unsafe` already takes
/// for the same reason.
/// The `/etc/hosts` fragment is written through a *quoted* heredoc, so
/// nothing here is shell-expanded today - but a value still must not be
/// able to introduce a new line into a line-oriented config file, or
/// terminate the heredoc early by containing its delimiter.
///
/// `reject_shell_metacharacters` on top of that is defense in depth. It
/// costs nothing for values that are supposed to be hostnames and IPs, and
/// it keeps this safe if the heredoc ever loses its quotes the way
/// `network::wireguard`'s had - which is exactly the bug that turned a
/// peer's public key into remote code execution across the whole mesh.
fn reject_unsafe(value: &str) -> AppResult<()> {
    if value.contains("VIBESSH_DNS_EOF") {
        return Err(AppError::InvalidInput("that value contains characters that aren't allowed in a DNS alias".into()));
    }
    command::reject_newlines(value, "a DNS alias")?;
    command::reject_shell_metacharacters(value, "a DNS alias")
}

/// Pure rendering, separated from the actual SSH push - same split every
/// other real-server-mutating module here uses so the shape can be unit
/// tested without a live connection.
pub fn render_hosts_fragment(views: &[DnsView]) -> AppResult<String> {
    for view in views {
        reject_unsafe(&view.hostname)?;
        reject_unsafe(&view.ip)?;
    }
    let mut fragment = format!("{BEGIN_MARKER}\n");
    for view in views {
        fragment.push_str(&format!("{} {}\n", view.ip, view.hostname));
    }
    fragment.push_str(&format!("{END_MARKER}\n"));
    Ok(fragment)
}

/// Replaces the previous managed block wholesale (idempotent full-sync,
/// same shape `firewall_service`/`network_service::reconcile_mesh` already
/// use) rather than diffing - `sed` deletes the old block (a no-op if this
/// is the first sync and it doesn't exist yet), then the fresh one is
/// appended.
async fn push_fragment(connection: &crate::ssh::SshSession, fragment: &str) -> AppResult<()> {
    let script = format!(
        "set -e\nsudo sed -i '/{BEGIN_MARKER}/,/{END_MARKER}/d' /etc/hosts\nsudo tee -a /etc/hosts >/dev/null <<'VIBESSH_DNS_EOF'\n{fragment}VIBESSH_DNS_EOF\n"
    );
    let output = connection.execute_command(&script).await?;
    if output.exit_code != 0 {
        let detail = output.stderr.trim();
        return Err(AppError::Connection(format!("couldn't update /etc/hosts: {}", if detail.is_empty() { "sed/tee failed" } else { detail })));
    }
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsSyncResult {
    pub server_id: Uuid,
    pub ok: bool,
    pub error: Option<String>,
}

/// Pushes the exact same fragment to every current mesh member - per-member
/// outcome, never collapsed into one boolean, the same "don't claim SUCCESS
/// for a Node that was unreachable" requirement every other reconcile path
/// here applies.
pub async fn sync_dns(
    suffix: &str,
    network_repo: &NodeNetworkRepository,
    server_repo: &ServerRepository,
    app_repo: &ApplicationRepository,
    dns_repo: &DnsRepository,
    sessions: &SshSessionManager,
) -> AppResult<Vec<DnsSyncResult>> {
    let views = resolve_dns_view(suffix, network_repo, server_repo, app_repo, dns_repo)?;
    let fragment = render_hosts_fragment(&views)?;
    let members = network_repo.list()?;

    let mut results = Vec::with_capacity(members.len());
    for member in &members {
        // Same dead-cached-session recovery as `network_service::reconcile_mesh`
        // - see that call site's comment for why this can't just rely on
        // `get_or_connect` alone.
        let outcome = match get_or_connect(server_repo, sessions, member.server_id).await {
            Ok(connection) => match push_fragment(&connection, &fragment).await {
                Ok(()) => Ok(()),
                Err(first_err) => {
                    sessions.remove(member.server_id).await;
                    match get_or_connect(server_repo, sessions, member.server_id).await {
                        Ok(connection) => push_fragment(&connection, &fragment).await.map_err(|_| first_err),
                        Err(_) => Err(first_err),
                    }
                }
            },
            Err(err) => Err(err),
        };
        results.push(match outcome {
            Ok(()) => DnsSyncResult { server_id: member.server_id, ok: true, error: None },
            Err(err) => DnsSyncResult { server_id: member.server_id, ok: false, error: Some(err.to_string()) },
        });
    }
    Ok(results)
}

/// A real check, not an assumption: SSHes into one live mesh member and
/// asks it to resolve `hostname` itself (`getent hosts`, the same
/// mechanism any real program on that host would use), comparing the
/// result against what Desktop expects it to be.
pub async fn verify_alias(
    network_repo: &NodeNetworkRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    hostname: &str,
    expected_ip: &str,
) -> AppResult<bool> {
    reject_unsafe(hostname)?;
    let members = network_repo.list()?;
    let verifier = members.first().ok_or_else(|| AppError::InvalidInput("the Vibe Network has no members to verify DNS from".into()))?;
    // A single command, so this can just delegate to the same
    // retry-on-dead-session primitive plain command execution already gets
    // elsewhere, instead of calling the connection directly.
    let output = crate::services::ssh_service::execute_command(server_repo, sessions, verifier.server_id, &format!("getent hosts {}", shell_quote(hostname))).await?;
    Ok(output.exit_code == 0 && output.stdout.split_whitespace().next() == Some(expected_ip))
}


/// What every alias-mutating Tauri command actually returns - the mutated
/// alias itself (`None` for a delete, nothing left to describe) plus the
/// outcome of the full-mesh `sync_dns` push this codebase now runs
/// automatically right after, instead of leaving the user to notice and
/// click "Synchronizuj" themselves. The DB write and the sync are still two
/// separate steps under the hood (a sync failure on one unreachable Node
/// must never undo an alias that was otherwise saved successfully) - this
/// struct is just what bundles their outcomes for the frontend to show in
/// one place.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsAliasWithSync {
    pub alias: Option<DnsRecord>,
    pub sync_results: Vec<DnsSyncResult>,
}

pub fn get_dns_suffix(state: &crate::state::DnsSuffixState) -> String {
    state.get()
}

/// Persists the new suffix (file) and updates the live state every
/// alias/Node-hostname computation reads from - see `state::DnsSuffixState`'s
/// own doc comment for why those are two separate steps. Rejects an
/// invalid suffix before touching either (see `validate_dns_suffix`) -
/// this is not retroactive, see this module's own doc comment.
pub fn set_dns_suffix(state: &crate::state::DnsSuffixState, config_dir: &std::path::Path, suffix: &str) -> AppResult<String> {
    let suffix = suffix.trim().to_ascii_lowercase();
    validate_dns_suffix(&suffix)?;
    crate::storage::dns_config::save_dns_suffix(config_dir, &suffix)?;
    state.set(suffix.clone());
    Ok(suffix)
}

pub fn list_records(dns_repo: &DnsRepository) -> AppResult<Vec<DnsRecord>> {
    dns_repo.list()
}

pub fn create_alias(suffix: &str, dns_repo: &DnsRepository, application_id: Uuid, hostname: &str) -> AppResult<DnsRecord> {
    dns_repo.create(application_id, &normalize_alias(suffix, hostname))
}

pub fn update_alias(suffix: &str, dns_repo: &DnsRepository, id: Uuid, hostname: &str) -> AppResult<DnsRecord> {
    dns_repo.update_hostname(id, &normalize_alias(suffix, hostname))
}

pub fn delete_alias(dns_repo: &DnsRepository, id: Uuid) -> AppResult<()> {
    dns_repo.delete(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_alias_slugifies_and_appends_the_suffix() {
        assert_eq!(normalize_alias(".vibe", "db01"), "db01.vibe");
        assert_eq!(normalize_alias(".vibe", "DB01"), "db01.vibe");
        assert_eq!(normalize_alias(".vibe", "db 01!!"), "db-01.vibe");
        assert_eq!(normalize_alias(".vibe", "db01.vibe"), "db01.vibe", "already-suffixed input must not become db01.vibe.vibe");
    }

    #[test]
    fn normalize_alias_uses_whatever_suffix_is_configured() {
        assert_eq!(normalize_alias(".internal", "db01"), "db01.internal");
        assert_eq!(normalize_alias(".internal", "db01.vibe"), "db01-vibe.internal", "a stale .vibe-suffixed input isn't the new suffix, so it's just more text to slugify");
    }

    #[test]
    fn node_alias_slugifies_the_server_name() {
        assert_eq!(node_alias(".vibe", "Hetzner 01"), "hetzner-01.vibe");
        assert_eq!(node_alias(".vibe", "  weird///name  "), "weird-name.vibe");
    }

    #[test]
    fn validate_dns_suffix_requires_a_leading_dot_and_a_safe_character_set() {
        assert!(validate_dns_suffix(".vibe").is_ok());
        assert!(validate_dns_suffix(".my-network.internal").is_ok());
        assert!(validate_dns_suffix("vibe").is_err(), "must start with a dot");
        assert!(validate_dns_suffix(".").is_err(), "must have at least one character after the dot");
        assert!(validate_dns_suffix(".Vibe").is_err(), "uppercase isn't a valid DNS label character here");
        assert!(validate_dns_suffix(".vi be").is_err(), "no whitespace");
        assert!(validate_dns_suffix(&format!(".{}", "a".repeat(40))).is_err(), "too long");
    }

    /// A real bug, not a hypothetical: without this cap, a Node/Application
    /// name longer than 63 characters produced a hostname label real
    /// DNS/`/etc/hosts` would reject outright - see `slugify`'s own doc
    /// comment for why RFC 1123's 63-character DNS label limit is the exact
    /// bound.
    #[test]
    fn slugify_truncates_to_the_rfc1123_dns_label_limit() {
        let long_name = "a".repeat(80);
        let slug = slugify(&long_name);
        assert_eq!(slug.len(), 63);
        assert_eq!(slug, "a".repeat(63));
    }

    /// Truncation must never leave a dangling trailing dash, even in the
    /// unlucky case where the cutoff lands exactly on a separator - a
    /// slightly shorter label is correct here, a label ending in `-` is not
    /// (rejected by real DNS the same as one over the length limit).
    #[test]
    fn slugify_truncation_never_leaves_a_trailing_dash() {
        let name = format!("{} more-text-after-the-cutoff", "a".repeat(62));
        let slug = slugify(&name);
        assert!(slug.len() <= 63);
        assert!(!slug.ends_with('-'), "must never end with a dash: {slug:?}");
    }

    #[test]
    fn render_hosts_fragment_wraps_entries_in_markers() {
        let views = vec![
            DnsView { hostname: "hetzner-01.vibe".into(), ip: "10.77.0.1".into(), kind: DnsViewKind::Node, server_id: Uuid::new_v4() },
            DnsView { hostname: "db01.vibe".into(), ip: "10.77.0.1".into(), kind: DnsViewKind::Service { application_id: Uuid::new_v4() }, server_id: Uuid::new_v4() },
        ];
        let fragment = render_hosts_fragment(&views).unwrap();
        assert_eq!(fragment, format!("{BEGIN_MARKER}\n10.77.0.1 hetzner-01.vibe\n10.77.0.1 db01.vibe\n{END_MARKER}\n"));
    }

    #[test]
    fn render_hosts_fragment_rejects_a_newline_smuggled_into_a_hostname() {
        let views = vec![DnsView { hostname: "evil.vibe\nrm -rf /".into(), ip: "10.77.0.1".into(), kind: DnsViewKind::Node, server_id: Uuid::new_v4() }];
        assert!(render_hosts_fragment(&views).is_err());
    }
}
