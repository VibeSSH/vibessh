//! Etap M4/M5: Private DNS - a generated `/etc/hosts` fragment pushed to
//! every Vibe Network member, not a real DNS server (see this module's own
//! "v1 shape" note below). Two kinds of alias share one namespace:
//!
//! - **Node aliases** (`<slugified-name>.vibe`) - always present for every
//!   mesh member, computed on the fly from `Server::name`, never stored.
//! - **Service aliases** (`dns_records`, e.g. `db01.vibe`) - explicitly
//!   created by the user, one per Application.
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
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::dns_repository::DnsRepository;
use crate::storage::node_network_repository::NodeNetworkRepository;
use crate::storage::server_repository::ServerRepository;

const SUFFIX: &str = ".vibe";
const BEGIN_MARKER: &str = "# BEGIN VIBESSH-MANAGED-DNS";
const END_MARKER: &str = "# END VIBESSH-MANAGED-DNS";

/// Lowercases, replaces anything that isn't `[a-z0-9-]` with `-`, collapses
/// repeats, and trims leading/trailing `-` - the same treatment a Node's
/// own name (arbitrary user text) and a user-typed alias both need before
/// either is safe to use as a hostname or to interpolate into a shell
/// heredoc. Never empty: an all-symbol input becomes `"node"`.
fn slugify(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut last_was_dash = false;
    for ch in input.chars().flat_map(char::to_lowercase) {
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

/// Slugifies and appends `.vibe` if not already present - `"db01"` and
/// `"db01.vibe"` (and `"DB01!!"`) all normalize to the exact same stored
/// hostname, so a user typing either the bare alias or the full name gets
/// the same, predictable result.
pub fn normalize_alias(input: &str) -> String {
    let trimmed = input.trim().to_ascii_lowercase();
    let base = trimmed.strip_suffix(SUFFIX).unwrap_or(&trimmed);
    format!("{}{SUFFIX}", slugify(base))
}

fn node_alias(server_name: &str) -> String {
    format!("{}{SUFFIX}", slugify(server_name))
}

/// Every alias a mesh member's `/etc/hosts` should carry - every current
/// member's own Node alias, plus every service alias whose Application
/// currently lives on a mesh member (a service alias for an Application on
/// a Node that hasn't joined the mesh, or with no `server_id` at all,
/// simply can't resolve to anything yet and is skipped, not an error).
pub fn resolve_dns_view(network_repo: &NodeNetworkRepository, server_repo: &ServerRepository, app_repo: &ApplicationRepository, dns_repo: &DnsRepository) -> AppResult<Vec<DnsView>> {
    let members = network_repo.list()?;
    let mut views = Vec::new();

    for member in &members {
        let server = server_repo.get(member.server_id)?.ok_or_else(|| AppError::NotFound(format!("server {}", member.server_id)))?;
        views.push(DnsView { hostname: node_alias(&server.name), ip: member.wireguard_ip.clone(), kind: DnsViewKind::Node, server_id: member.server_id });
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
fn reject_unsafe(value: &str) -> AppResult<()> {
    if value.contains('\n') || value.contains('\r') || value.contains("VIBESSH_DNS_EOF") {
        return Err(AppError::InvalidInput("that value contains characters that aren't allowed in a DNS alias".into()));
    }
    Ok(())
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
    network_repo: &NodeNetworkRepository,
    server_repo: &ServerRepository,
    app_repo: &ApplicationRepository,
    dns_repo: &DnsRepository,
    sessions: &SshSessionManager,
) -> AppResult<Vec<DnsSyncResult>> {
    let views = resolve_dns_view(network_repo, server_repo, app_repo, dns_repo)?;
    let fragment = render_hosts_fragment(&views)?;
    let members = network_repo.list()?;

    let mut results = Vec::with_capacity(members.len());
    for member in &members {
        let outcome = match get_or_connect(server_repo, sessions, member.server_id).await {
            Ok(connection) => push_fragment(&connection, &fragment).await,
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
    let connection = get_or_connect(server_repo, sessions, verifier.server_id).await?;
    let output = connection.execute_command(&format!("getent hosts {}", shell_quote(hostname))).await?;
    Ok(output.exit_code == 0 && output.stdout.split_whitespace().next() == Some(expected_ip))
}

fn shell_quote(value: &str) -> String {
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

pub fn list_records(dns_repo: &DnsRepository) -> AppResult<Vec<DnsRecord>> {
    dns_repo.list()
}

pub fn create_alias(dns_repo: &DnsRepository, application_id: Uuid, hostname: &str) -> AppResult<DnsRecord> {
    dns_repo.create(application_id, &normalize_alias(hostname))
}

pub fn update_alias(dns_repo: &DnsRepository, id: Uuid, hostname: &str) -> AppResult<DnsRecord> {
    dns_repo.update_hostname(id, &normalize_alias(hostname))
}

pub fn delete_alias(dns_repo: &DnsRepository, id: Uuid) -> AppResult<()> {
    dns_repo.delete(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_alias_slugifies_and_appends_the_suffix() {
        assert_eq!(normalize_alias("db01"), "db01.vibe");
        assert_eq!(normalize_alias("DB01"), "db01.vibe");
        assert_eq!(normalize_alias("db 01!!"), "db-01.vibe");
        assert_eq!(normalize_alias("db01.vibe"), "db01.vibe", "already-suffixed input must not become db01.vibe.vibe");
    }

    #[test]
    fn node_alias_slugifies_the_server_name() {
        assert_eq!(node_alias("Hetzner 01"), "hetzner-01.vibe");
        assert_eq!(node_alias("  weird///name  "), "weird-name.vibe");
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
