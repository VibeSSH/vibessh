//! Declaring and publishing an Application's ports, and the collision
//! checks standing in front of both.
//!
//! The module boundary is where the audit's findings clustered: every
//! function here can change what is reachable from outside a Node, which is
//! why bind-address resolution and the two collision checks live together
//! rather than beside lifecycle calls that never touch a socket.//!
//! Split out of a single 2685-line `application_service` (FIX_PLAN E.7).
//! Behaviour is unchanged; only the file boundaries moved.


use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::protocol_name;
use crate::models::{
    ApplicationPort, PortInput, PortVisibility,
};
// The one shared implementation - this module used to carry its own
// byte-identical copy, one of six across the codebase.
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::firewall_rule_repository::FirewallRuleRepository;
use crate::storage::node_network_repository::NodeNetworkRepository;
use crate::storage::server_repository::ServerRepository;

use super::*;

pub fn list_application_ports(repo: &ApplicationRepository, application_id: Uuid) -> AppResult<Vec<ApplicationPort>> {
    repo.list_ports(application_id)
}

/// Resolves the `bind_address` this port should actually publish on,
/// straight from the user's chosen `visibility` intent (Etap M4's
/// "Application Network") - the caller never has to compute a bind address
/// by hand. `Public`/`VibeNetwork` both bind `0.0.0.0`: what actually
/// restricts a "Vibe Network only" port to mesh members is the firewall
/// rule `services::firewall_service::desired_rules` derives from this same
/// `visibility`, not a different bind address - see that function's own
/// doc comment.
/// Turns a port's declared *visibility* into the address it actually binds
/// to. This is the only thing standing between "Vibe Network only" meaning
/// what it says and the port being reachable from the public internet.
///
/// **`VibeNetwork` binds the Node's own mesh address, not `0.0.0.0`.** It
/// used to bind `0.0.0.0` and rely on a UFW rule scoped to the mesh CIDR to
/// keep the rest of the internet out. That does not work for a Docker
/// Application, which is most of them: Docker installs its own DNAT and
/// FORWARD-chain ACCEPT rules that are evaluated *before* UFW's
/// `ufw-user-input` chain, so a published container port is reachable
/// regardless of what `ufw status` shows. The operator saw a correct-looking
/// firewall rule and an exposed database.
///
/// Binding the mesh address instead moves the restriction from a filter
/// rule into the socket itself: the kernel will not accept a connection
/// that did not arrive on the WireGuard interface, and there is nothing for
/// Docker's iptables rules to bypass. It also fails *loudly* and early -
/// a Node that has not joined the mesh gets a clear error here rather than
/// a silently public port.
// `pub(super)` purely so the unit tests in `mod.rs` can reach it. The tests
// stayed in one place when this file was split - moving a thousand lines of
// shared setup into eight files would have been a second, larger change
// wearing the same commit.
pub(super) fn resolve_bind_address(network_repo: &NodeNetworkRepository, server_id: Option<Uuid>, port: &PortInput) -> AppResult<String> {
    match port.visibility {
        PortVisibility::Public => Ok("0.0.0.0".to_string()),
        PortVisibility::Localhost => Ok("127.0.0.1".to_string()),
        PortVisibility::Custom => Ok(port.bind_address.clone()),
        PortVisibility::VibeNetwork => {
            let server_id = server_id.ok_or_else(|| {
                AppError::InvalidInput("a local application has no Vibe Network address - use 'Localhost' or 'Public' instead".into())
            })?;
            let member = network_repo.get(server_id)?.ok_or_else(|| {
                AppError::InvalidInput(
                    "this Node hasn't joined the Vibe Network yet, so it has no private address to bind to - join it from the Vibe Network page first, or pick a different visibility".into(),
                )
            })?;
            Ok(member.wireguard_ip)
        }
    }
}

/// Re-resolves the bind address of every `VibeNetwork` port on a Node.
///
/// A Node's mesh address is allocated on join and released on leave, so
/// rejoining can hand out a different one. Without this, ports saved under
/// the old address would keep trying to bind an address the Node no longer
/// holds - the container would fail to start with a bare "cannot assign
/// requested address". Called after any mesh membership change.
pub async fn refresh_vibe_network_bind_addresses(
    repo: &ApplicationRepository,
    network_repo: &NodeNetworkRepository,
    server_id: Uuid,
) -> AppResult<usize> {
    let mut updated = 0;
    for application in repo.list_by_server(server_id)? {
        for port in repo.list_ports(application.id)? {
            if port.visibility != PortVisibility::VibeNetwork {
                continue;
            }
            let input = PortInput {
                name: port.name.clone(),
                protocol: port.protocol,
                bind_address: port.bind_address.clone(),
                internal_port: port.internal_port,
                external_port: port.external_port,
                visibility: port.visibility,
                required: port.required,
            };
            // A Node that just left the mesh has no address to resolve to;
            // leave the stored value alone rather than failing the whole
            // pass, and let the next start surface it.
            let Ok(resolved) = resolve_bind_address(network_repo, Some(server_id), &input) else { continue };
            if resolved != port.bind_address {
                repo.update_port(application.id, port.id, &PortInput { bind_address: resolved, ..input })?;
                updated += 1;
            }
        }
    }
    Ok(updated)
}

/// Blocks a port save that would collide with something else, before any
/// firewall/container change ever happens - the design doc's own
/// requirement ("Przed zastosowaniem EXIT PORT VibeSSH musi sprawdzić: inne
/// APPLICATION, inne EXIT PORTS, Docker bindings, listening sockets").
/// Two checks, in order: a DB-level check against every other Application's
/// own declared ports on this same Node (`ApplicationRepository::
/// find_external_port_owner` - fast, always available, catches the most
/// common mistake of two Applications both wanting the same port), then a
/// live probe of what's actually bound on the host right now
/// (`firewall_service::listening_process` via `ss` - catches a port taken
/// by something VibeSSH doesn't know about at all: a manually-run service,
/// a Docker container from outside VibeSSH). A `None` `external_port`, or
/// no `server_id` (a Local application), or the port being saved unchanged
/// from what it already was, all skip straight through - nothing is
/// actually about to change in any of those cases, so there's nothing new
/// to collide with. The live probe is best-effort in the sense that a
/// Node the desktop can't currently reach never blocks the save (the same
/// "a connectivity hiccup must never block an otherwise valid change"
/// stance `sync_firewall_best_effort` below already takes) - but an
/// *answered* probe that finds the port already bound to something else is
/// a hard stop, same as the DB check.
async fn check_external_port_available(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    excluding_port_id: Option<Uuid>,
    port: &PortInput,
) -> AppResult<()> {
    let Some(external_port) = port.external_port else { return Ok(()) };
    let application = get_application(repo, application_id)?;
    let Some(server_id) = application.application.server_id else { return Ok(()) };

    if let Some(current_port_id) = excluding_port_id {
        let unchanged = application
            .ports
            .iter()
            .any(|existing| existing.id == current_port_id && existing.external_port == Some(external_port) && existing.protocol == port.protocol);
        if unchanged {
            return Ok(());
        }
    }

    if let Some(owner) = repo.find_external_port_owner(server_id, excluding_port_id, port.protocol, external_port)? {
        return Err(AppError::PortInUse { port: external_port, protocol: protocol_name(port.protocol), owner: Some(owner) });
    }

    if let Ok(connection) = crate::services::ssh_service::get_or_connect(server_repo, sessions, server_id).await {
        if let Some(process) = crate::services::firewall_service::listening_process(&connection, port.protocol, external_port).await.ok().flatten() {
            return Err(AppError::PortInUse { port: external_port, protocol: protocol_name(port.protocol), owner: Some(process) });
        }
    }
    Ok(())
}

/// Publishing a port (`external_port` set) should open it in the Node's
/// firewall right away, not only whenever someone next thinks to click
/// "Sync Firewall" on the Ports tab - `sync_firewall_best_effort` fires
/// after every successful write. Best-effort deliberately: a sync failure
/// (host unreachable, no supported firewall detected, a transient SSH
/// hiccup) must never fail the port CRUD call itself - the port is already
/// correctly saved either way, and `firewall_service::reconcile_node` is
/// safe to retry from the Ports tab at any time.
pub async fn add_application_port(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    network_repo: &NodeNetworkRepository,
    firewall_rule_repo: &FirewallRuleRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    port: &PortInput,
) -> AppResult<ApplicationPort> {
    let server_id = get_application(repo, application_id)?.application.server_id;
    let port = PortInput { bind_address: resolve_bind_address(network_repo, server_id, port)?, ..port.clone() };
    // The live half of the check first - it talks to the Node and cannot be
    // inside a database transaction. The database half then happens
    // *atomically* with the insert (`claim_external_port`), because a
    // separate check and insert is a window two concurrent adds both fit
    // through - which a double-clicked button produces.
    check_external_port_available(repo, server_repo, sessions, application_id, None, &port).await?;
    let created = match server_id {
        Some(server_id) => repo.claim_external_port(application_id, server_id, &port)?,
        // A Local application has no Node to share a port with, so there is
        // no cross-Application claim to make.
        None => repo.add_port(application_id, &port)?,
    };
    sync_firewall_best_effort(repo, server_repo, network_repo, firewall_rule_repo, sessions, application_id).await;
    Ok(created)
}

pub async fn update_application_port(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    network_repo: &NodeNetworkRepository,
    firewall_rule_repo: &FirewallRuleRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    port_id: Uuid,
    port: &PortInput,
) -> AppResult<ApplicationPort> {
    let server_id = get_application(repo, application_id)?.application.server_id;
    let port = PortInput { bind_address: resolve_bind_address(network_repo, server_id, port)?, ..port.clone() };
    check_external_port_available(repo, server_repo, sessions, application_id, Some(port_id), &port).await?;
    let updated = repo.update_port(application_id, port_id, &port)?;
    sync_firewall_best_effort(repo, server_repo, network_repo, firewall_rule_repo, sessions, application_id).await;
    Ok(updated)
}

async fn sync_firewall_best_effort(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    network_repo: &NodeNetworkRepository,
    firewall_rule_repo: &FirewallRuleRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
) {
    if let Err(err) =
        crate::services::firewall_service::sync_application_node_firewall(repo, server_repo, network_repo, firewall_rule_repo, sessions, application_id).await
    {
        log::warn!("firewall sync after a port change failed (application {application_id}): {err}");
    }
}

/// Also syncs the firewall afterward (best-effort, same as
/// `add_application_port`/`update_application_port`) - a removed port's
/// rule no longer appears in `firewall_service::desired_rules`, so this is
/// what actually revokes it on the host rather than leaving it open
/// forever (see `firewall::mod`'s own doc comment on removal).
pub async fn remove_application_port(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    network_repo: &NodeNetworkRepository,
    firewall_rule_repo: &FirewallRuleRepository,
    sessions: &SshSessionManager,
    application_id: Uuid,
    port_id: Uuid,
) -> AppResult<()> {
    repo.remove_port(application_id, port_id)?;
    sync_firewall_best_effort(repo, server_repo, network_repo, firewall_rule_repo, sessions, application_id).await;
    Ok(())
}
