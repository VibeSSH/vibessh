//! Orchestrates `ApplicationRepository` + `BlueprintRegistry` + whichever
//! `ApplicationRuntime` a given Application's `runtime_type` resolves to
//! (via `runtime::runtime_for`) - the service layer
//! `commands::application_commands` calls into, same shape as
//! `server_service`/`ssh_service`.

use std::collections::HashMap;
use std::sync::Arc;

use uuid::Uuid;

use crate::blueprints::{BlueprintRegistry, ProvisionContext};
use crate::errors::{AppError, AppResult};
use crate::models::{
    Application, ApplicationDetail, ApplicationPort, ApplicationStatus, Blueprint, CreateApplicationFromBlueprintInput,
    CreateApplicationInput, EnvironmentVariable, HealthCheckType, PortInput, PortVisibility, RegistryCredential, RuntimeType,
    SetHealthCheckInput, SetRegistryCredentialInput, SetResourceLimitsInput, UpdateApplicationInput,
};
use crate::runtime::local_process::LocalProcessManager;
use crate::runtime::{self, ApplicationRuntime, HealthCheckSpec, HealthStatus, ResourceUsage, RuntimeContext};
use crate::services::ssh_service::{get_or_connect, retry_on_connection_failure};
// The one shared implementation - this module used to carry its own
// byte-identical copy, one of six across the codebase.
use crate::ssh::command::quote as shell_quote;
use crate::ssh::SshSession;
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::credentials;
use crate::storage::firewall_rule_repository::FirewallRuleRepository;
use crate::storage::log_capture::LogCaptureStore;
use crate::storage::node_network_repository::NodeNetworkRepository;
use crate::storage::registry_credential_repository::RegistryCredentialRepository;
use crate::storage::server_repository::ServerRepository;

pub fn list_applications(repo: &ApplicationRepository) -> AppResult<Vec<Application>> {
    repo.list()
}

pub fn get_application(repo: &ApplicationRepository, id: Uuid) -> AppResult<ApplicationDetail> {
    repo.get(id)?.ok_or_else(|| AppError::NotFound(format!("application {id}")))
}

pub fn list_blueprints(registry: &BlueprintRegistry) -> Vec<Blueprint> {
    registry.list().into_iter().cloned().collect()
}

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
fn resolve_bind_address(network_repo: &NodeNetworkRepository, server_id: Option<Uuid>, port: &PortInput) -> AppResult<String> {
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
        return Err(AppError::InvalidInput(format!("port {external_port} is already published by '{owner}' on this Node")));
    }

    if let Ok(connection) = crate::services::ssh_service::get_or_connect(server_repo, sessions, server_id).await {
        if let Some(process) = crate::services::firewall_service::listening_process(&connection, port.protocol, external_port).await.ok().flatten() {
            return Err(AppError::InvalidInput(format!("port {external_port} is already in use on this Node (by {process})")));
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
    check_external_port_available(repo, server_repo, sessions, application_id, None, &port).await?;
    let created = repo.add_port(application_id, &port)?;
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

/// Creates the working directory (local `create_dir_all`, or `mkdir -p`
/// over the same SSH connection every other Remote feature shares) before
/// the Application row itself is created - so a fresh working directory
/// the user just typed in the wizard (as opposed to one from an existing,
/// already-provisioned setup) doesn't leave `start()` failing on a bare
/// "No such file or directory" the user has to go fix by hand over a
/// separate SSH session. `mkdir -p` (not the single-level SFTP
/// `create_directory` the Files module uses) so a working directory nested
/// under parents that don't exist yet - `/srv/minecraft/my-server` on a
/// freshly provisioned host - still works in one step, and so an already-
/// existing directory is a no-op rather than an error.
pub async fn create_application(
    repo: &ApplicationRepository,
    registry: &BlueprintRegistry,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    input: CreateApplicationFromBlueprintInput,
) -> AppResult<ApplicationDetail> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(AppError::InvalidInput("a name is required".into()));
    }
    let working_directory = input.working_directory.trim();
    if working_directory.is_empty() {
        return Err(AppError::InvalidInput("a working directory is required".into()));
    }
    validate_remote_working_directory(input.server_id, working_directory)?;

    let handler = registry
        .get(&input.blueprint_id)
        .ok_or_else(|| AppError::InvalidInput(format!("unknown blueprint '{}'", input.blueprint_id)))?;
    if !handler.blueprint().supported_runtime_types.contains(&input.runtime_type) {
        return Err(AppError::InvalidInput(format!("'{}' doesn't support this runtime type", handler.blueprint().name)));
    }

    let mut blueprint_inputs: HashMap<String, serde_json::Value> = match input.blueprint_inputs {
        serde_json::Value::Object(map) => map.into_iter().collect(),
        serde_json::Value::Null => HashMap::new(),
        _ => return Err(AppError::InvalidInput("blueprint inputs must be an object".into())),
    };

    ensure_working_directory_exists(server_repo, sessions, input.server_id, working_directory).await?;

    // Resolved once, reused for provisioning - the same connection
    // `start_application` et al. would independently resolve later via
    // `load_runtime`, just needed here too for a blueprint that has to
    // reach the target host during creation (PaperBlueprint downloading a
    // jar over this same connection rather than through the SSH user's
    // desktop).
    let connection = resolve_connection(server_repo, sessions, input.server_id).await?;
    let provision_context = ProvisionContext { working_directory, connection };
    let discovered = handler.provision(&blueprint_inputs, &provision_context).await?;
    blueprint_inputs.extend(discovered);

    let runtime_config = handler.render_runtime_config(&blueprint_inputs)?;

    // A blueprint's own well-known port (Paper/Velocity's 25565) is created
    // as a real, published port from the start - see `Blueprint::default_ports`'s
    // own doc comment for why leaving this for the user to notice and add
    // by hand (on the Ports tab, after wondering why their server isn't
    // reachable) is exactly the kind of manual step this feature set exists
    // to remove. `required: true` since removing it would silently break
    // the one thing this Application is for; still freely editable
    // (a different external port, a different visibility) same as any
    // other port.
    let ports = handler
        .blueprint()
        .default_ports
        .iter()
        .map(|port| PortInput {
            name: port.name.clone(),
            protocol: port.protocol,
            bind_address: "0.0.0.0".to_string(),
            internal_port: port.internal_port,
            external_port: Some(port.external_port),
            visibility: PortVisibility::Public,
            required: true,
        })
        .collect();

    let create_input = CreateApplicationInput {
        server_id: input.server_id,
        name: name.to_string(),
        description: input.description,
        blueprint_id: input.blueprint_id,
        blueprint_version: handler.blueprint().blueprint_version,
        runtime_type: input.runtime_type,
        working_directory: working_directory.to_string(),
        environment: input.environment,
        ports,
        runtime_config,
        // Stored so a later edit (`update_application_config`) can re-render
        // `runtime_config` from the blueprint plus only the fields the user
        // actually changed, instead of needing the whole rendered config
        // reverse-engineered back into field values.
        metadata: serde_json::json!({ "blueprintInputs": blueprint_inputs }),
    };
    let detail = repo.create(&create_input)?;
    store_secret_environment_values(detail.application.id, &create_input.environment)?;
    Ok(detail)
}

/// Writes every secret row's real value into the OS keyring, keyed by this
/// Application's own id - the counterpart to `ApplicationRepository::create`/
/// `set_environment` never writing that value into SQLite themselves (see
/// `EnvironmentVariable::value`'s own doc comment). Takes the caller's own
/// in-memory list (still holding the real values it was given) rather than
/// re-reading from the repository, which would only see the redacted rows
/// it just wrote.
/// An Application on a Node gets a directory that VibeSSH itself will act
/// on with root privileges - most consequentially
/// `runtime::docker::ensure_working_directory_owned_by_dedicated_user`,
/// which runs `sudo chown -R <the Application's own unprivileged account>`
/// over it on every start. Naming a shared system directory there doesn't
/// produce an error, it produces a broken host: `chown -R` on `/`, `/etc`
/// or `/usr` hands the whole filesystem to an account with no login shell,
/// and there is no undo. Validating the path at the one place it enters the
/// system is the only place this can be caught cheaply.
///
/// **Only for Applications on a Node.** A local Application runs on the
/// operator's own machine through `LocalProcessManager` - no SSH, no
/// `sudo`, no chown, and its `working_directory` is a native path
/// (`C:\Users\...` on Windows) that a POSIX-shaped check would reject for
/// no benefit.
fn validate_remote_working_directory(server_id: Option<Uuid>, working_directory: &str) -> AppResult<()> {
    if server_id.is_none() {
        return Ok(());
    }
    crate::ssh::command::validate_application_directory(working_directory)
}

pub(crate) fn store_secret_environment_values(application_id: Uuid, environment: &[EnvironmentVariable]) -> AppResult<()> {
    for env in environment {
        if env.is_secret {
            credentials::store_environment_secret(application_id, &env.key, &env.value)?;
        }
    }
    Ok(())
}

/// The inverse of `store_secret_environment_values` - fills in each secret
/// row's real value from the OS keyring, for the one case that's actually
/// allowed to see it: a runtime about to start/inspect the real process
/// (`load_runtime`). Never called on a path that returns straight to the
/// frontend.
pub(crate) fn resolve_environment_secrets(application_id: Uuid, environment: Vec<EnvironmentVariable>) -> AppResult<Vec<EnvironmentVariable>> {
    environment
        .into_iter()
        .map(|mut env| {
            if env.is_secret {
                env.value = credentials::load_environment_secret(application_id, &env.key)?.unwrap_or_default();
            }
            Ok(env)
        })
        .collect()
}

/// Re-renders `runtime_config` from the blueprint after applying `field_values`
/// on top of whatever was stored at creation (or the last edit) - lets the
/// user change one field (JVM args, Java version, ...) without having to
/// resupply every other field the blueprint needs. An application created
/// before this existed has no stored `blueprintInputs` yet - `field_values`
/// is then all this has to render from, which the caller (the edit form) is
/// responsible for pre-filling with the blueprint's own defaults rather than
/// silently rendering from an empty map.
///
/// **Re-runs `provision()`**, same as `create_application` does - fixes a
/// real bug where changing Paper/Purpur's Minecraft version (or Velocity/
/// Waterfall's own version field) silently kept running the *old* jar:
/// `render_runtime_config` only ever reads the already-downloaded
/// `__jarFilename` a blueprint's `provision()` step discovers, never the
/// version field itself, so skipping `provision()` here left that field
/// editable in the UI but functionally inert. The cost is every edit
/// re-running `provision()` even when only an unrelated field changed
/// (JVM args, say) - for Paper/Purpur/Velocity/Waterfall that means a
/// redundant jar re-download; every other built-in blueprint's `provision()`
/// is a no-op, so this costs them nothing. A `HashMap` has no stable field
/// order to diff against to skip the redundant case cheaply, and knowing
/// *which* fields actually require re-provisioning is knowledge only each
/// blueprint's own `provision()` has - correctness first, this is the
/// simple way to get it without teaching `BlueprintHandler` a new "does
/// this field matter" concept.
pub async fn update_application_config(
    repo: &ApplicationRepository,
    registry: &BlueprintRegistry,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    id: Uuid,
    field_values: serde_json::Value,
) -> AppResult<ApplicationDetail> {
    let detail = repo.get(id)?.ok_or_else(|| AppError::NotFound(format!("application {id}")))?;
    let handler = registry
        .get(&detail.application.blueprint_id)
        .ok_or_else(|| AppError::InvalidInput(format!("unknown blueprint '{}'", detail.application.blueprint_id)))?;
    // A blueprint's own supported runtime types can narrow after an
    // Application already exists on one that's no longer listed (e.g.
    // Paper/Velocity going Docker-only for mandatory isolation, Etap M1,
    // after some existing Application was created as a Remote Process) -
    // that Application keeps running fine on its already-stored
    // `runtime_config` (nothing here touches it), but re-rendering that
    // config from today's blueprint logic would silently assume a runtime
    // type it was never designed for (Paper/Velocity's Docker-shape
    // renderer, for one, doesn't even produce the `{command, args}` shape
    // `RemoteProcessConfig` needs) - rejected outright with a clear reason
    // rather than either corrupting the config or surfacing whatever
    // internal error the renderer happens to fail with first.
    if !handler.blueprint().supported_runtime_types.contains(&detail.application.runtime_type) {
        return Err(AppError::InvalidInput(format!(
            "'{}' no longer supports this application's runtime type - its configuration can't be edited here",
            handler.blueprint().name
        )));
    }

    let edited: HashMap<String, serde_json::Value> = match field_values {
        serde_json::Value::Object(map) => map.into_iter().collect(),
        serde_json::Value::Null => HashMap::new(),
        _ => return Err(AppError::InvalidInput("blueprint inputs must be an object".into())),
    };

    let mut merged: HashMap<String, serde_json::Value> = match detail.metadata.get("blueprintInputs") {
        Some(serde_json::Value::Object(map)) => map.clone().into_iter().collect(),
        _ => HashMap::new(),
    };
    merged.extend(edited);

    let connection = resolve_connection(server_repo, sessions, detail.application.server_id).await?;
    let provision_context = ProvisionContext { working_directory: &detail.application.working_directory, connection };
    let discovered = handler.provision(&merged, &provision_context).await?;
    merged.extend(discovered);

    let runtime_config = handler.render_runtime_config(&merged)?;

    let update_input = UpdateApplicationInput {
        name: detail.application.name.clone(),
        description: detail.application.description.clone(),
        working_directory: detail.application.working_directory.clone(),
        runtime_config,
        metadata: serde_json::json!({ "blueprintInputs": merged }),
    };
    repo.update(id, &update_input)
}

pub(super) async fn ensure_working_directory_exists(
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Option<Uuid>,
    working_directory: &str,
) -> AppResult<()> {
    match server_id {
        None => tokio::fs::create_dir_all(working_directory)
            .await
            .map_err(|err| AppError::InvalidInput(format!("couldn't create working directory '{working_directory}': {err}"))),
        // Same dead-cached-session recovery every other SSH-touching
        // function in this file already gets - this one just never had it
        // before, which made it possible to hit a raw "couldn't open an SSH
        // channel" right at the very first step of creating an Application,
        // on a session that had simply gone idle since it was last used.
        Some(server_id) => retry_on_connection_failure(sessions, Some(server_id), || async {
            // Re-validated here, not only in `create_application`: this is
            // the function that actually runs `mkdir -p`/`chown` against a
            // real host, and `migration_service` calls it directly with a
            // directory it carried over from another Node rather than one a
            // caller just typed. The check has to sit where the privileged
            // command is, not only where the happy path enters.
            validate_remote_working_directory(Some(server_id), working_directory)?;
            let connection = get_or_connect(server_repo, sessions, server_id).await?;
            let output = connection.execute_command(&format!("mkdir -p {}", shell_quote(working_directory))).await?;
            if output.exit_code == 0 {
                return Ok(());
            }

            // Plain `mkdir` fails whenever the connecting SSH user doesn't
            // own some parent in the path - a stock cloud Ubuntu image's
            // default non-root user (e.g. "ubuntu") owns its own home
            // directory but nothing else, so the Pterodactyl-style
            // `/home/container/<name>` default this wizard suggests fails
            // outright there. "Plug and play, no manual server prep" is the
            // whole point of this button, so this retries once with `sudo`
            // (non-interactive: `-n` fails fast instead of hanging on a
            // password prompt the SSH exec channel can never answer) and
            // hands the new directory's ownership back to the connecting
            // user, same "assume passwordless sudo for first-time setup"
            // stance `install_docker` already takes. If sudo itself isn't
            // usable either, its own error is far more actionable ("a
            // password is required") than the original mkdir's bare
            // "Permission denied", so that's what gets surfaced.
            let server = server_repo.get(server_id)?.ok_or_else(|| AppError::NotFound(format!("server {server_id}")))?;
            let dir = shell_quote(working_directory);
            let user = shell_quote(&server.username);
            let sudo_output = connection.execute_command(&format!("sudo -n mkdir -p {dir} && sudo -n chown {user}:{user} {dir}")).await?;
            if sudo_output.exit_code == 0 {
                return Ok(());
            }

            let detail = sudo_output.stderr.trim();
            let detail = if !detail.is_empty() {
                detail.to_string()
            } else {
                let mkdir_detail = output.stderr.trim();
                if mkdir_detail.is_empty() { "mkdir failed".to_string() } else { mkdir_detail.to_string() }
            };
            Err(AppError::InvalidInput(format!("couldn't create working directory '{working_directory}' on the remote host: {detail}")))
        })
        .await,
    }
}


pub async fn delete_application(repo: &ApplicationRepository, log_capture: &LogCaptureStore, id: Uuid) -> AppResult<()> {
    // Best-effort, and before the row itself goes away - `ON DELETE CASCADE`
    // takes care of the `application_environment` rows, but the OS keyring
    // has no idea those rows ever existed, so a secret's entry would
    // otherwise outlive the Application it belonged to forever.
    if let Ok(Some(detail)) = repo.get(id) {
        for env in &detail.environment {
            if env.is_secret {
                let _ = credentials::delete_environment_secret(id, &env.key);
            }
        }
    }
    // Same reasoning as the secrets above - a deleted Application's own
    // captured log history has nothing left to belong to.
    log_capture.delete(id).await;
    repo.delete(id)
}

/// `None` for a Local application, `Some` (via the same cache-then-connect
/// path every other remote feature already shares) for a Remote one.
async fn resolve_connection(
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    server_id: Option<Uuid>,
) -> AppResult<Option<Arc<SshSession>>> {
    match server_id {
        None => Ok(None),
        Some(server_id) => Ok(Some(get_or_connect(server_repo, sessions, server_id).await?)),
    }
}

/// The four `*_application` lifecycle functions below all need the exact
/// same three things before they can act - the Application's current
/// detail, its (possibly absent) SSH connection, and the runtime
/// implementation for its `runtime_type`. Factored here so each of them is
/// a couple of lines, not a repeat of this setup.
async fn load_runtime(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
) -> AppResult<(ApplicationDetail, Option<Arc<SshSession>>, Box<dyn ApplicationRuntime>)> {
    let mut detail = get_application(repo, id)?;
    // Every other reader of `ApplicationDetail` (the Tauri commands that
    // hand it to the frontend) sees a secret row redacted - this is the one
    // path that's actually about to start/inspect the real process, so it's
    // the one place real secret values get resolved back in.
    detail.environment = resolve_environment_secrets(detail.application.id, detail.environment)?;
    let connection = resolve_connection(server_repo, sessions, detail.application.server_id).await?;
    let runtime = runtime::runtime_for(detail.application.runtime_type, local_process_manager.clone());
    Ok((detail, connection, runtime))
}

/// Re-reads status straight from the runtime and persists it - the only
/// way `Application::status`/`last_status_check_at` (a cache, never
/// trusted as sole truth - see `models::application`'s own doc comment)
/// ever gets updated.
async fn refresh_and_persist_status(
    repo: &ApplicationRepository,
    runtime: &dyn ApplicationRuntime,
    ctx: &RuntimeContext<'_>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    let status = runtime.status(ctx).await?;
    repo.update_status(id, status)?;
    Ok(status)
}

pub async fn start_application(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    registry_repo: &RegistryCredentialRepository,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    let server_id = get_application(repo, id)?.application.server_id;
    retry_on_connection_failure(sessions, server_id, || async {
        let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
        // Best-effort login before whatever `start()` does under the hood
        // (a `docker create` that only pulls if the image isn't already
        // cached locally) - a no-op for every image without a stored
        // credential, see `ensure_registry_login`'s own doc comment.
        if let (Some(conn), Some(image)) = (&connection, detail.runtime_config.get("image").and_then(|v| v.as_str())) {
            ensure_registry_login(conn, registry_repo, image).await?;
        }
        let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, ports: &detail.ports, connection };
        runtime.start(&ctx).await?;
        refresh_and_persist_status(repo, runtime.as_ref(), &ctx, id).await
    })
    .await
}

pub async fn stop_application(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
    graceful: bool,
) -> AppResult<ApplicationStatus> {
    let server_id = get_application(repo, id)?.application.server_id;
    retry_on_connection_failure(sessions, server_id, || async {
        let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
        let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, ports: &detail.ports, connection };
        runtime.stop(&ctx, graceful).await?;
        refresh_and_persist_status(repo, runtime.as_ref(), &ctx, id).await
    })
    .await
}

pub async fn restart_application(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    let server_id = get_application(repo, id)?.application.server_id;
    retry_on_connection_failure(sessions, server_id, || async {
        let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
        let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, ports: &detail.ports, connection };
        runtime.restart(&ctx).await?;
        refresh_and_persist_status(repo, runtime.as_ref(), &ctx, id).await
    })
    .await
}

/// "Recreate Container" (Etap M1) - `destroy()` then `start()`, so an edited
/// image/command/resource-limit/restart-policy actually takes effect. Only
/// meaningful for `RuntimeType::Docker`: the other three runtimes already
/// regenerate their config on every `start()` (see each `ApplicationRuntime`
/// impl's own doc comment), so there's nothing a separate recreate step
/// would do beyond what starting already does - rejected outright rather
/// than silently doing nothing, same "don't pretend two runtimes have
/// identical capabilities" stance `set_application_resource_limits` already
/// takes. Safe to call while stopped or running - `destroy()` is a no-op if
/// nothing was ever created, and removes (stopping first) if it was.
pub async fn recreate_application(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    registry_repo: &RegistryCredentialRepository,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    let detail = get_application(repo, id)?;
    if detail.application.runtime_type != RuntimeType::Docker {
        return Err(AppError::InvalidInput("recreating is only meaningful for Docker applications".into()));
    }
    let server_id = detail.application.server_id;
    retry_on_connection_failure(sessions, server_id, || async {
        let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
        let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, ports: &detail.ports, connection };
        runtime.destroy(&ctx).await?;
        // Same best-effort login as `start_application` - `recreate` is
        // exactly the path a changed image (via `ApplicationConfigCard`/
        // `DockerImageCard`'s own auto-recreate) goes through, so this is
        // the realistic place a *new*, never-before-pulled private image
        // actually gets requested.
        if let (Some(conn), Some(image)) = (&ctx.connection, ctx.runtime_config.get("image").and_then(|v| v.as_str())) {
            ensure_registry_login(conn, registry_repo, image).await?;
        }
        runtime.start(&ctx).await?;
        refresh_and_persist_status(repo, runtime.as_ref(), &ctx, id).await
    })
    .await
}

pub async fn kill_application(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    let server_id = get_application(repo, id)?.application.server_id;
    retry_on_connection_failure(sessions, server_id, || async {
        let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
        let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, ports: &detail.ports, connection };
        runtime.kill(&ctx).await?;
        refresh_and_persist_status(repo, runtime.as_ref(), &ctx, id).await
    })
    .await
}

pub async fn refresh_application_status(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
) -> AppResult<ApplicationStatus> {
    let server_id = get_application(repo, id)?.application.server_id;
    retry_on_connection_failure(sessions, server_id, || async {
        let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
        let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, ports: &detail.ports, connection };
        refresh_and_persist_status(repo, runtime.as_ref(), &ctx, id).await
    })
    .await
}

pub async fn application_resource_usage(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
) -> AppResult<ResourceUsage> {
    let server_id = get_application(repo, id)?.application.server_id;
    retry_on_connection_failure(sessions, server_id, || async {
        let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
        let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, ports: &detail.ports, connection };
        runtime.resource_usage(&ctx).await
    })
    .await
}

/// The last `max_lines` lines available right now - a snapshot the Logs tab
/// fetches on open and on manual refresh, same "pull, not push" shape
/// `ContainerLogsPanel`'s existing `get_server_container_logs` already
/// uses. Not live-streamed - see `runtime::mod`'s own `LogProvider` doc
/// comment for why that's a pull-based API in the first place.
/// Merges a fresh live fetch into `log_capture` (see that module's own doc
/// comment for why this exists at all) before answering from the merged,
/// locally-persisted result rather than the live fetch directly - so a
/// Recreate's brand new, empty container log buffer never actually looks
/// empty to the user, and a briefly unreachable Node degrades to "whatever
/// was captured last time" instead of a hard error on a tab that's mostly
/// used to figure out *why* something just failed.
pub async fn application_logs(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    log_capture: &LogCaptureStore,
    id: Uuid,
    max_lines: u32,
) -> AppResult<Vec<String>> {
    let server_id = get_application(repo, id)?.application.server_id;
    let live_fetch = retry_on_connection_failure(sessions, server_id, || async {
        let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
        let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, ports: &detail.ports, connection };
        runtime.logs(&ctx).await?.tail(max_lines).await
    })
    .await;

    if let Ok(live_lines) = live_fetch {
        let previous_last_line = log_capture.tail(id, 1).await?;
        let new_lines = merge_new_log_lines(previous_last_line.first().map(String::as_str), live_lines);
        log_capture.append(id, &new_lines).await?;
    }

    log_capture.tail(id, max_lines).await
}

/// Finds where genuinely new output starts in a fresh live fetch, using the
/// single most-recently-captured line as the anchor - the *rightmost*
/// match (not the first), so a line that happens to repeat further back in
/// the live batch doesn't fool this into re-appending everything after an
/// earlier, spurious match. No anchor at all (first capture ever, or the
/// container was just recreated and its brand new buffer shares nothing
/// with the old one) means the whole live batch is "new" - appended after
/// whatever's already stored, so a Recreate only ever adds to history, it
/// never loses what came before.
fn merge_new_log_lines(previous_last_line: Option<&str>, live_lines: Vec<String>) -> Vec<String> {
    match previous_last_line.and_then(|anchor| live_lines.iter().rposition(|line| line == anchor)) {
        Some(index) => live_lines[index + 1..].to_vec(),
        None => live_lines,
    }
}

/// Sends one line of input to the application's stdin/console (a Minecraft
/// server's `say hello`, a generic process's own REPL, etc.) - `Ok(None)`
/// from `runtime.console` (a systemd unit with no stdin, see that trait
/// method's own doc comment) and a console that reports `supports_input() ==
/// false` are both surfaced as the same clear "read-only" error rather than
/// silently swallowing the keystrokes.
pub async fn application_console_write(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
    input: &str,
) -> AppResult<()> {
    let server_id = get_application(repo, id)?.application.server_id;
    retry_on_connection_failure(sessions, server_id, || async {
        let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
        let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, ports: &detail.ports, connection };
        let console = runtime
            .console(&ctx)
            .await?
            .ok_or_else(|| AppError::InvalidInput("this application has no interactive console".into()))?;
        if !console.supports_input() {
            return Err(AppError::InvalidInput("this application's console is read-only".into()));
        }
        console.write(input).await
    })
    .await
}

/// Validates a health check configuration before it's stored - beyond the
/// repository's own "does `port_id` belong to this application" check
/// (`ApplicationRepository::set_health_check`), `Tcp`/`Http`/
/// `MinecraftStatus` are meaningless without a port, and `Http` additionally
/// needs a path that actually starts a request (`curl`/`reqwest` would
/// otherwise be asked to fetch a URL like `http://host:portfoo`).
pub fn set_application_health_check(
    repo: &ApplicationRepository,
    id: Uuid,
    input: SetHealthCheckInput,
) -> AppResult<ApplicationDetail> {
    if input.health_check_type != HealthCheckType::Process && input.port_id.is_none() {
        return Err(AppError::InvalidInput("this health check type needs a port".into()));
    }
    let http_path = match input.health_check_type {
        HealthCheckType::Http => {
            let path = input.http_path.as_deref().unwrap_or("").trim();
            if !path.starts_with('/') {
                return Err(AppError::InvalidInput("the HTTP health check path must start with '/'".into()));
            }
            Some(path.to_string())
        }
        _ => None,
    };
    repo.set_health_check(id, input.health_check_type, input.port_id, http_path.as_deref())?;
    get_application(repo, id)
}

/// Patches the `memoryLimitMb`/`cpuLimitCores` keys inside an Application's
/// own `runtime_config` - the same keys `runtime::docker::DockerConfig` and
/// `runtime::systemd::SystemdConfig` both read, under one shared name so
/// the frontend can offer a single "CPU cores" input regardless of which of
/// the two runtime types an Application actually uses. Rejected outright
/// for `LocalProcess`/`RemoteProcess`: there's no OS-level mechanism this
/// codebase can enforce a limit through for a bare child process the way
/// Docker/systemd already provide natively, and silently accepting a
/// setting that does nothing would be exactly the "pretend two runtimes
/// have identical capabilities" the architecture doc warns against, not an
/// honest gap.
pub fn set_application_resource_limits(
    repo: &ApplicationRepository,
    id: Uuid,
    input: SetResourceLimitsInput,
) -> AppResult<ApplicationDetail> {
    let detail = get_application(repo, id)?;
    if !matches!(detail.application.runtime_type, RuntimeType::Docker | RuntimeType::Systemd | RuntimeType::RemoteProcess) {
        return Err(AppError::InvalidInput("resource limits are only supported for Docker, systemd, and remote process applications".into()));
    }
    // A bare SSH-launched process has no cgroup of its own to cap memory
    // through the way Docker/systemd do (see `runtime::remote_process`'s own
    // `cpulimit`-based CPU cap for why CPU is still possible there) - reject
    // rather than silently store a limit that will never actually apply.
    if detail.application.runtime_type == RuntimeType::RemoteProcess && input.memory_limit_mb.is_some() {
        return Err(AppError::InvalidInput("a memory limit isn't supported for remote process applications".into()));
    }
    runtime::validate_resource_limits(input.memory_limit_mb, input.cpu_limit_cores)?;

    let mut runtime_config = detail.runtime_config.clone();
    let object = runtime_config.as_object_mut().ok_or_else(|| AppError::Internal("runtime_config wasn't a JSON object".into()))?;
    match input.memory_limit_mb {
        Some(mb) => object.insert("memoryLimitMb".to_string(), serde_json::json!(mb)),
        None => object.remove("memoryLimitMb"),
    };
    match input.cpu_limit_cores {
        Some(cores) => object.insert("cpuLimitCores".to_string(), serde_json::json!(cores)),
        None => object.remove("cpuLimitCores"),
    };

    repo.update_runtime_config(id, &runtime_config)
}

/// Patches only the `image` key inside a Docker Application's own
/// `runtime_config` - same one-key-patch shape `set_application_resource_limits`
/// already established, Docker-only for the same kind of reason: `image` is
/// a Docker-specific concept, the other three runtime types run a bare
/// `command`/`args` instead and have nothing here to change. Like every
/// other `runtime_config` edit in this codebase, this alone doesn't affect
/// an already-created container - the caller still needs a Recreate
/// (`recreate_application`) for a running Application to actually pick up
/// the new image, same "change it, save it, then Recreate" pattern the
/// frontend already applies for resource limits/environment/ports.
pub fn set_application_image(repo: &ApplicationRepository, id: Uuid, image: String) -> AppResult<ApplicationDetail> {
    let detail = get_application(repo, id)?;
    if detail.application.runtime_type != RuntimeType::Docker {
        return Err(AppError::InvalidInput("the image is only configurable for Docker applications".into()));
    }
    let image = image.trim();
    if image.is_empty() {
        return Err(AppError::InvalidInput("an image is required".into()));
    }
    if image.contains(['\n', '\r']) {
        return Err(AppError::InvalidInput("the image can't contain a newline".into()));
    }

    let mut runtime_config = detail.runtime_config.clone();
    let object = runtime_config.as_object_mut().ok_or_else(|| AppError::Internal("runtime_config wasn't a JSON object".into()))?;
    object.insert("image".to_string(), serde_json::json!(image));

    repo.update_runtime_config(id, &runtime_config)
}

// ---- Private Docker registry credentials ----

pub fn list_registry_credentials(repo: &RegistryCredentialRepository) -> AppResult<Vec<RegistryCredential>> {
    repo.list()
}

/// Sets (creating or overwriting) the login for one registry host - keyed
/// by `input.registry`, not a fresh row every call, so re-saving a
/// registry's credential updates the existing keyring entry in place rather
/// than leaking an orphaned one under a discarded id (see
/// `models::RegistryCredential`'s own doc comment on the one-row-per-host
/// shape).
pub fn set_registry_credential(repo: &RegistryCredentialRepository, input: SetRegistryCredentialInput) -> AppResult<RegistryCredential> {
    let registry = input.registry.trim();
    let username = input.username.trim();
    if registry.is_empty() {
        return Err(AppError::InvalidInput("a registry host is required".into()));
    }
    if username.is_empty() {
        return Err(AppError::InvalidInput("a username is required".into()));
    }
    if input.password.is_empty() {
        return Err(AppError::InvalidInput("a password or token is required".into()));
    }

    let credential = match repo.find_by_registry(registry)? {
        Some(existing) => {
            repo.update_username(existing.id, username)?;
            RegistryCredential { username: username.to_string(), ..existing }
        }
        None => repo.create(registry, username)?,
    };
    credentials::store_registry_credential_password(credential.id, &input.password)?;
    Ok(credential)
}

pub fn remove_registry_credential(repo: &RegistryCredentialRepository, id: Uuid) -> AppResult<()> {
    repo.delete(id)?;
    // Best-effort, same reasoning as every other "delete the row, then the
    // secret that went with it" cleanup in this file - the row is already
    // gone either way, and a leftover keyring entry under a dead id is
    // orphaned but harmless, not a correctness problem worth failing this
    // call over.
    let _ = credentials::delete_registry_credential_password(id);
    Ok(())
}

/// Docker's own reference-parsing rule: the first path segment before a
/// `/` is a registry host only if it looks like one (has a `.` or `:`, or
/// is literally `localhost`) - otherwise the whole reference is an implicit
/// Docker Hub repository (`nginx:latest`, `someuser/someimage:tag`). Not
/// guessing - this is the same heuristic the real `docker` CLI/distribution
/// tooling itself uses to tell `myuser/myimage` (Hub) apart from
/// `ghcr.io/myuser/myimage` (not Hub).
fn registry_host(image: &str) -> &str {
    match image.split_once('/') {
        Some((first, _)) if first.contains('.') || first.contains(':') || first == "localhost" => first,
        _ => "docker.io",
    }
}

/// Best-effort `docker login` before a pull/create that might need one -
/// a no-op (not an error) when no credential is stored for the image's own
/// registry host, so every ordinary public-image pull stays exactly as
/// cheap as before this existed. `--password-stdin` (never `-p` /
/// `--password`, both deprecated specifically because the value would
/// otherwise show up in `ps`/shell history on the remote host) - the
/// password is piped in via `printf`, never interpolated into the command
/// string itself.
async fn ensure_registry_login(connection: &SshSession, registry_repo: &RegistryCredentialRepository, image: &str) -> AppResult<()> {
    let host = registry_host(image);
    let Some(credential) = registry_repo.find_by_registry(host)? else { return Ok(()) };
    let Some(password) = credentials::load_registry_credential_password(credential.id)? else { return Ok(()) };

    // A bare `docker login` (no host argument) targets Docker Hub - passing
    // `docker.io` explicitly as a host argument isn't guaranteed to be
    // treated the same way, so the sentinel gets the no-argument form
    // instead of just interpolating it in unconditionally.
    let target = if host == "docker.io" { String::new() } else { format!(" {}", shell_quote(host)) };
    let command = format!("printf '%s' {} | sudo docker login{target} -u {} --password-stdin", shell_quote(&password), shell_quote(&credential.username));
    let output = connection.execute_command(&command).await?;
    if output.exit_code != 0 {
        let detail = output.stderr.trim();
        let detail = if detail.is_empty() { "docker login failed".to_string() } else { detail.to_string() };
        return Err(AppError::Connection(format!("couldn't log in to {host}: {detail}")));
    }
    Ok(())
}

/// `docker pull` for a Docker Application's currently-configured image, on
/// the Node it actually runs on - the design doc's "aktualizacja image"
/// requirement. Only re-fetches whatever layers changed upstream since the
/// last pull (meaningful for a floating tag like `:latest`, or a version
/// tag whose upstream image was rebuilt in place) - an already-running
/// container keeps running its existing layers regardless, same as every
/// other `runtime_config`-adjacent change; the caller still needs a
/// Recreate to actually switch a running container onto the freshly
/// pulled layers. Returns Docker's own pull output (image digest, "Status:
/// Downloaded newer image" / "Image is up to date") for the caller to show
/// as proof something real happened, not just a bare success.
pub async fn pull_application_image(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    registry_repo: &RegistryCredentialRepository,
    application_id: Uuid,
) -> AppResult<String> {
    let detail = get_application(repo, application_id)?;
    if detail.application.runtime_type != RuntimeType::Docker {
        return Err(AppError::InvalidInput("pulling an image is only supported for Docker applications".into()));
    }
    let image = detail
        .runtime_config
        .get("image")
        .and_then(|value| value.as_str())
        .filter(|image| !image.trim().is_empty())
        .ok_or_else(|| AppError::InvalidInput("no image configured for this application".into()))?
        .to_string();
    let server_id = detail.application.server_id;

    retry_on_connection_failure(sessions, server_id, || async {
        let connection = resolve_connection(server_repo, sessions, server_id)
            .await?
            .ok_or_else(|| AppError::Internal("a Docker application must have a Node".into()))?;
        ensure_registry_login(&connection, registry_repo, &image).await?;
        let output = connection.execute_command(&format!("sudo docker pull {}", shell_quote(&image))).await?;
        if output.exit_code != 0 {
            let detail = output.stderr.trim();
            let detail = if detail.is_empty() { "docker pull failed".to_string() } else { detail.to_string() };
            return Err(AppError::Connection(format!("couldn't pull '{image}': {detail}")));
        }
        Ok(output.stdout)
    })
    .await
}

/// Replaces an Application's whole environment variable set - same
/// "editable after creation, not just once in the wizard" bar
/// `set_application_resource_limits`/`update_application_config` already
/// meet for their own fields. No runtime-type restriction (unlike resource
/// limits): every runtime already reads `ctx.environment` the same way, so
/// there's no runtime that genuinely can't support this. Key/value
/// validation itself is deliberately left to whichever runtime's own
/// `start`/`create_container` reads this back (`validate_environment` in
/// `runtime::docker`/`runtime::remote_process`) rather than duplicated
/// here - the same single-source-of-truth reasoning
/// `update_application_config` already applies to blueprint field
/// validation.
/// **Secret handling**: the frontend never has a secret row's real value to
/// resend (`ApplicationRepository::get` always redacts it - see
/// `EnvironmentVariable::value`'s own doc comment), so a secret row with an
/// empty value here means "unchanged," not "clear it" - resolved below by
/// keeping the previous real value from the keyring. A key that was secret
/// before and no longer appears (removed, or flipped back to a plain
/// variable) has its keyring entry deleted, so nothing outlives the row
/// that referenced it.
pub fn set_application_environment(repo: &ApplicationRepository, id: Uuid, environment: Vec<EnvironmentVariable>) -> AppResult<ApplicationDetail> {
    let previous = get_application(repo, id)?.environment;

    let mut resolved = Vec::with_capacity(environment.len());
    for mut env in environment {
        if env.is_secret && env.value.is_empty() && previous.iter().any(|p| p.key == env.key && p.is_secret) {
            env.value = credentials::load_environment_secret(id, &env.key)?.unwrap_or_default();
        }
        resolved.push(env);
    }

    for prev in &previous {
        if prev.is_secret && !resolved.iter().any(|env| env.key == prev.key && env.is_secret) {
            let _ = credentials::delete_environment_secret(id, &prev.key);
        }
    }

    repo.set_environment(id, &resolved)?;
    store_secret_environment_values(id, &resolved)?;
    get_application(repo, id)
}

/// Turns an Application's stored `health_check_*` columns into the
/// `HealthCheckSpec` its runtime actually probes with - `Ok(None)` (not an
/// error) whenever the configuration can't be resolved right now (the
/// referenced port was removed, or the target Server was deleted), matching
/// `Application::health_check_port_id`'s own doc comment: that case reports
/// `HealthStatus::Unknown`, not a failure.
fn resolve_health_check_spec(
    server_repo: &ServerRepository,
    application: &Application,
    ports: &[ApplicationPort],
) -> AppResult<Option<HealthCheckSpec>> {
    let resolve_port = || application.health_check_port_id.and_then(|port_id| ports.iter().find(|p| p.id == port_id)).map(|p| p.internal_port);

    Ok(match application.health_check_type {
        HealthCheckType::Process => Some(HealthCheckSpec::Process),
        HealthCheckType::Tcp => resolve_port().map(|port| HealthCheckSpec::Tcp { port }),
        HealthCheckType::Http => {
            let (Some(port), Some(path)) = (resolve_port(), application.health_check_http_path.clone()) else { return Ok(None) };
            Some(HealthCheckSpec::Http { port, path })
        }
        HealthCheckType::MinecraftStatus => {
            let Some(port) = resolve_port() else { return Ok(None) };
            let host = match application.server_id {
                None => "127.0.0.1".to_string(),
                Some(server_id) => match server_repo.get(server_id)? {
                    Some(server) => server.host,
                    None => return Ok(None),
                },
            };
            Some(HealthCheckSpec::MinecraftStatus { host, port })
        }
    })
}

pub async fn application_health_check(
    repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    local_process_manager: &Arc<LocalProcessManager>,
    id: Uuid,
) -> AppResult<HealthStatus> {
    let server_id = get_application(repo, id)?.application.server_id;
    retry_on_connection_failure(sessions, server_id, || async {
        let (detail, connection, runtime) = load_runtime(repo, server_repo, sessions, local_process_manager, id).await?;
        let Some(spec) = resolve_health_check_spec(server_repo, &detail.application, &detail.ports)? else {
            return Ok(HealthStatus::Unknown);
        };
        let ctx = RuntimeContext { application: &detail.application, runtime_config: &detail.runtime_config, environment: &detail.environment, ports: &detail.ports, connection };
        runtime.health_check(&ctx, &spec).await
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_new_log_lines_with_no_anchor_treats_the_whole_batch_as_new() {
        let live = vec!["a".to_string(), "b".to_string()];
        assert_eq!(merge_new_log_lines(None, live.clone()), live);
    }

    #[test]
    fn merge_new_log_lines_returns_only_what_comes_after_the_anchor() {
        let live = vec!["a".to_string(), "b".to_string(), "c".to_string(), "d".to_string()];
        assert_eq!(merge_new_log_lines(Some("b"), live), vec!["c".to_string(), "d".to_string()]);
    }

    #[test]
    fn merge_new_log_lines_returns_nothing_new_when_the_anchor_is_the_last_line() {
        let live = vec!["a".to_string(), "b".to_string()];
        assert!(merge_new_log_lines(Some("b"), live).is_empty());
    }

    #[test]
    fn merge_new_log_lines_uses_the_rightmost_match_when_a_line_repeats() {
        let live = vec!["retry".to_string(), "ok".to_string(), "retry".to_string(), "done".to_string()];
        assert_eq!(merge_new_log_lines(Some("retry"), live), vec!["done".to_string()]);
    }

    #[test]
    fn merge_new_log_lines_treats_a_missing_anchor_as_a_fresh_container_and_keeps_everything() {
        // The anchor line isn't in the live batch at all - a Recreate gave
        // the container a brand new buffer with nothing in common with what
        // was captured before. Everything live is new, not dropped.
        let live = vec!["fresh start".to_string(), "line two".to_string()];
        assert_eq!(merge_new_log_lines(Some("something from the old container"), live.clone()), live);
    }

    #[test]
    fn registry_host_treats_a_bare_image_as_docker_hub() {
        assert_eq!(registry_host("nginx:latest"), "docker.io");
        assert_eq!(registry_host("alpine"), "docker.io");
    }

    #[test]
    fn registry_host_treats_a_user_org_path_with_no_dot_or_colon_as_docker_hub() {
        assert_eq!(registry_host("someuser/someimage:tag"), "docker.io");
    }

    #[test]
    fn registry_host_recognizes_a_domain_looking_first_segment_as_the_registry() {
        assert_eq!(registry_host("ghcr.io/someuser/someimage:tag"), "ghcr.io");
        assert_eq!(registry_host("my.private.registry/team/app:tag"), "my.private.registry");
    }

    #[test]
    fn registry_host_recognizes_a_localhost_or_port_first_segment_as_the_registry() {
        assert_eq!(registry_host("localhost:5000/app:tag"), "localhost:5000");
        assert_eq!(registry_host("localhost/app:tag"), "localhost");
    }

    #[test]
    fn registry_host_recognizes_an_explicit_docker_io_prefix_too() {
        assert_eq!(registry_host("docker.io/library/nginx:latest"), "docker.io");
    }

    /// A real `ApplicationRepository` + `ServerRepository` against a fresh
    /// temp SQLite file, a real `LocalProcessManager`, and the real
    /// built-in `BlueprintRegistry` - the same components `lib.rs` wires
    /// together for the actual app, exercised end to end (create -> start
    /// -> status -> stop -> delete) rather than only unit-tested in
    /// isolation. `ServerRepository`/`SshSessionManager` are unused by a
    /// Local application's own lifecycle but still required by every
    /// function's signature, matching production's own shape.
    #[allow(clippy::type_complexity)]
    fn temp_setup() -> (
        ApplicationRepository,
        ServerRepository,
        NodeNetworkRepository,
        SshSessionManager,
        Arc<LocalProcessManager>,
        BlueprintRegistry,
        FirewallRuleRepository,
        RegistryCredentialRepository,
        LogCaptureStore,
    ) {
        let path = std::env::temp_dir().join(format!("vibessh-app-service-test-{}.sqlite3", Uuid::new_v4()));
        let app_repo = ApplicationRepository::open(&path).unwrap();
        let server_repo = ServerRepository::open(&path).unwrap();
        let network_repo = NodeNetworkRepository::open(&path).unwrap();
        let firewall_rule_repo = FirewallRuleRepository::open(&path).unwrap();
        let registry_repo = RegistryCredentialRepository::open(&path).unwrap();
        let log_capture = LogCaptureStore::new(std::env::temp_dir().join(format!("vibessh-app-service-test-logs-{}", Uuid::new_v4()))).unwrap();
        (
            app_repo,
            server_repo,
            network_repo,
            SshSessionManager::new(),
            Arc::new(LocalProcessManager::new()),
            BlueprintRegistry::with_builtins(),
            firewall_rule_repo,
            registry_repo,
            log_capture,
        )
    }

    fn sleep_command_input() -> CreateApplicationFromBlueprintInput {
        #[cfg(windows)]
        let (command, args) = ("cmd", vec!["/C".to_string(), "echo hello-from-application-service && ping -n 6 127.0.0.1 >NUL".to_string()]);
        #[cfg(not(windows))]
        let (command, args) = ("sh", vec!["-c".to_string(), "echo hello-from-application-service; sleep 5".to_string()]);

        CreateApplicationFromBlueprintInput {
            server_id: None,
            name: "Integration Test App".to_string(),
            description: None,
            blueprint_id: "generic".to_string(),
            runtime_type: RuntimeType::LocalProcess,
            working_directory: std::env::temp_dir().to_string_lossy().into_owned(),
            environment: vec![],
            blueprint_inputs: serde_json::json!({ "command": command, "args": args }),
        }
    }

    #[tokio::test]
    async fn full_lifecycle_create_start_status_stop_delete() {
        let (app_repo, server_repo, _network_repo, sessions, local_process_manager, registry, _firewall_rule_repo, registry_credential_repo, log_capture) = temp_setup();

        let detail = create_application(&app_repo, &registry, &server_repo, &sessions, sleep_command_input()).await.unwrap();
        assert_eq!(detail.application.status, ApplicationStatus::Unknown);
        assert_eq!(detail.runtime_config["command"], serde_json::json!(if cfg!(windows) { "cmd" } else { "sh" }));

        let status = start_application(&app_repo, &server_repo, &sessions, &registry_credential_repo, &local_process_manager, detail.application.id).await.unwrap();
        assert_eq!(status, ApplicationStatus::Running);

        let refreshed = get_application(&app_repo, detail.application.id).unwrap();
        assert_eq!(refreshed.application.status, ApplicationStatus::Running);

        // The stdout pump runs on its own background task - poll rather
        // than assume it's already flushed by the time start() returned.
        let mut saw_output = false;
        for _ in 0..30 {
            let lines = application_logs(&app_repo, &server_repo, &sessions, &local_process_manager, &log_capture, detail.application.id, 10).await.unwrap();
            if lines.iter().any(|line| line.contains("hello-from-application-service")) {
                saw_output = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(saw_output, "expected application_logs to eventually show the process's stdout");

        let status = stop_application(&app_repo, &server_repo, &sessions, &local_process_manager, detail.application.id, true).await.unwrap();
        assert_eq!(status, ApplicationStatus::Stopped);

        delete_application(&app_repo, &log_capture, detail.application.id).await.unwrap();
        assert!(get_application(&app_repo, detail.application.id).is_err());
    }

    #[tokio::test]
    async fn create_rejects_an_unknown_blueprint() {
        let (app_repo, server_repo, _network_repo, sessions, _local_process_manager, registry, ..) = temp_setup();
        let mut input = sleep_command_input();
        input.blueprint_id = "does-not-exist".to_string();
        assert!(create_application(&app_repo, &registry, &server_repo, &sessions, input).await.is_err());
    }

    #[tokio::test]
    async fn create_rejects_a_runtime_type_the_blueprint_doesnt_support() {
        let (app_repo, server_repo, _network_repo, sessions, _local_process_manager, registry, ..) = temp_setup();
        let mut input = sleep_command_input();
        input.runtime_type = RuntimeType::Docker;
        assert!(create_application(&app_repo, &registry, &server_repo, &sessions, input).await.is_err());
    }

    #[tokio::test]
    async fn create_rejects_a_blank_name() {
        let (app_repo, server_repo, _network_repo, sessions, _local_process_manager, registry, ..) = temp_setup();
        let mut input = sleep_command_input();
        input.name = "   ".to_string();
        assert!(create_application(&app_repo, &registry, &server_repo, &sessions, input).await.is_err());
    }

    /// An Application on a Node has `sudo chown -R <its own account>` run
    /// over its `working_directory` on every start
    /// (`runtime::docker::ensure_working_directory_owned_by_dedicated_user`).
    /// Naming a shared system directory there doesn't fail, it hands the
    /// host's filesystem to an unprivileged account with no way back - so
    /// these have to be refused before anything touches the Node at all,
    /// which is also why this test needs no live connection to pass.
    #[tokio::test]
    async fn create_refuses_a_system_directory_for_an_application_on_a_node() {
        let (app_repo, server_repo, _network_repo, sessions, _local_process_manager, registry, ..) = temp_setup();
        let server = server_repo
            .create(&crate::models::ServerInput {
                name: "Node".to_string(),
                host: "203.0.113.10".to_string(),
                ssh_port: 22,
                username: "root".to_string(),
                authentication_type: crate::models::AuthenticationType::Password,
                private_key_path: None,
                group_id: None,
                password: Some("unused - validation rejects before connecting".to_string()),
                key_passphrase: None,
            })
            .unwrap();
        for hostile in ["/", "/etc", "/home", "/usr", "/var", "/root", "/srv", "srv/app", "/srv/../etc"] {
            let mut input = sleep_command_input();
            input.server_id = Some(server.id);
            input.runtime_type = RuntimeType::RemoteProcess;
            input.working_directory = hostile.to_string();
            let result = create_application(&app_repo, &registry, &server_repo, &sessions, input).await;
            assert!(result.is_err(), "should have refused {hostile:?}");
        }
    }

    fn port_input(visibility: PortVisibility, bind_address: &str) -> PortInput {
        PortInput {
            name: "game".to_string(),
            protocol: crate::models::PortProtocol::Tcp,
            bind_address: bind_address.to_string(),
            internal_port: 25565,
            external_port: Some(25565),
            visibility,
            required: false,
        }
    }

    /// The regression test for the finding that "Vibe Network only" ports
    /// were publicly reachable. `VibeNetwork` must never resolve to
    /// `0.0.0.0` - a UFW source-CIDR rule cannot restrict a published
    /// Docker port, because Docker's own iptables rules are evaluated
    /// first. Binding the mesh address is what makes the kernel enforce it.
    #[tokio::test]
    async fn a_vibe_network_port_binds_the_mesh_address_never_all_interfaces() {
        let (app_repo, server_repo, network_repo, ..) = temp_setup();
        let _ = &app_repo;
        let server = server_repo
            .create(&crate::models::ServerInput {
                name: "Node".to_string(),
                host: "203.0.113.10".to_string(),
                ssh_port: 22,
                username: "root".to_string(),
                authentication_type: crate::models::AuthenticationType::Password,
                private_key_path: None,
                group_id: None,
                password: Some("unused".to_string()),
                key_passphrase: None,
            })
            .unwrap();
        let member = network_repo.join(server.id, "K4hV1cB0mQ2sT7nZ9xY3lJ6pR8dW5gA0fE1uI2oC3vM=").unwrap();

        let resolved = resolve_bind_address(&network_repo, Some(server.id), &port_input(PortVisibility::VibeNetwork, "")).unwrap();
        assert_eq!(resolved, member.wireguard_ip);
        assert_ne!(resolved, "0.0.0.0");
    }

    /// ...and when the Node has no mesh address to bind, that must be a
    /// loud error rather than a silent fallback to `0.0.0.0`.
    #[tokio::test]
    async fn a_vibe_network_port_is_refused_when_the_node_isnt_on_the_mesh() {
        let (_app_repo, server_repo, network_repo, ..) = temp_setup();
        let server = server_repo
            .create(&crate::models::ServerInput {
                name: "Node".to_string(),
                host: "203.0.113.11".to_string(),
                ssh_port: 22,
                username: "root".to_string(),
                authentication_type: crate::models::AuthenticationType::Password,
                private_key_path: None,
                group_id: None,
                password: Some("unused".to_string()),
                key_passphrase: None,
            })
            .unwrap();

        let result = resolve_bind_address(&network_repo, Some(server.id), &port_input(PortVisibility::VibeNetwork, ""));
        assert!(result.is_err());
        // A local Application has no mesh address at all.
        assert!(resolve_bind_address(&network_repo, None, &port_input(PortVisibility::VibeNetwork, "")).is_err());
    }

    #[tokio::test]
    async fn the_other_visibilities_are_unchanged() {
        let (_app_repo, _server_repo, network_repo, ..) = temp_setup();
        assert_eq!(resolve_bind_address(&network_repo, None, &port_input(PortVisibility::Public, "")).unwrap(), "0.0.0.0");
        assert_eq!(resolve_bind_address(&network_repo, None, &port_input(PortVisibility::Localhost, "")).unwrap(), "127.0.0.1");
        assert_eq!(
            resolve_bind_address(&network_repo, None, &port_input(PortVisibility::Custom, "10.1.2.3")).unwrap(),
            "10.1.2.3"
        );
    }

    /// The mirror of the test above: the same check must not reject a
    /// *local* Application, whose `working_directory` is a native path on
    /// the operator's own machine (`C:\Users\...` on Windows) and which
    /// never goes near `sudo` or a remote host.
    #[tokio::test]
    async fn create_still_accepts_a_native_local_working_directory() {
        let (app_repo, server_repo, _network_repo, sessions, _local_process_manager, registry, ..) = temp_setup();
        let input = sleep_command_input();
        assert!(input.server_id.is_none());
        assert!(create_application(&app_repo, &registry, &server_repo, &sessions, input).await.is_ok());
    }

    #[tokio::test]
    async fn create_creates_a_missing_local_working_directory() {
        let (app_repo, server_repo, _network_repo, sessions, _local_process_manager, registry, ..) = temp_setup();
        let mut input = sleep_command_input();
        let fresh_dir = std::env::temp_dir().join(format!("vibessh-app-service-workdir-{}", Uuid::new_v4()));
        assert!(!fresh_dir.exists());
        input.working_directory = fresh_dir.to_string_lossy().into_owned();

        let detail = create_application(&app_repo, &registry, &server_repo, &sessions, input).await.unwrap();

        assert!(fresh_dir.is_dir());
        assert_eq!(detail.application.working_directory, fresh_dir.to_string_lossy());
        std::fs::remove_dir_all(&fresh_dir).ok();
    }

    #[tokio::test]
    async fn port_crud_add_update_remove_round_trips_through_the_service_layer() {
        let (app_repo, server_repo, network_repo, sessions, _local_process_manager, registry, firewall_rule_repo, ..) = temp_setup();
        let detail = create_application(&app_repo, &registry, &server_repo, &sessions, sleep_command_input()).await.unwrap();
        let application_id = detail.application.id;

        assert!(list_application_ports(&app_repo, application_id).unwrap().is_empty());

        let input = crate::models::PortInput {
            name: "game".to_string(),
            protocol: crate::models::PortProtocol::Tcp,
            bind_address: "0.0.0.0".to_string(),
            internal_port: 25565,
            external_port: None,
            visibility: crate::models::PortVisibility::Public,
            required: false,
        };
        let added = add_application_port(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, application_id, &input).await.unwrap();
        assert_eq!(added.internal_port, 25565);
        assert_eq!(list_application_ports(&app_repo, application_id).unwrap().len(), 1);

        // Adding the exact same internal_port/bind_address/protocol again
        // is a real collision, not a silent duplicate - the service layer
        // must surface the repository's own collision error, not swallow it.
        assert!(add_application_port(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, application_id, &input).await.is_err());

        let updated_input = crate::models::PortInput { internal_port: 25566, ..input };
        let updated = update_application_port(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, application_id, added.id, &updated_input).await.unwrap();
        assert_eq!(updated.internal_port, 25566);

        remove_application_port(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, application_id, added.id).await.unwrap();
        assert!(list_application_ports(&app_repo, application_id).unwrap().is_empty());
    }

    /// The design doc's own "check other Applications, other Exit Ports"
    /// collision requirement, exercised end to end through the service
    /// layer that actually enforces it (`check_external_port_available`) -
    /// the repository-level check this reuses only ever looked at ports on
    /// the *same* Application (see `add_port`'s own doc comment), so a
    /// second, unrelated Application publishing the exact same host port
    /// used to be silently allowed. The Node's host is unreachable
    /// (`203.0.113.10` is a TEST-NET-3 address, RFC 5737) - the live `ss`
    /// probe half of the check is expected to fail to connect and get
    /// skipped, proving the DB-level half alone is what's catching this,
    /// not a lucky live probe result.
    #[tokio::test]
    async fn add_application_port_rejects_an_external_port_already_published_by_a_different_application_on_the_same_node() {
        let (app_repo, server_repo, network_repo, sessions, _local_process_manager, _registry, firewall_rule_repo, ..) = temp_setup();
        let server = server_repo
            .create(&crate::models::ServerInput {
                name: "Collision Test Node".into(),
                host: "203.0.113.10".into(),
                ssh_port: 22,
                username: "root".into(),
                authentication_type: crate::models::AuthenticationType::Password,
                private_key_path: None,
                group_id: None,
                password: Some("x".into()),
                key_passphrase: None,
            })
            .unwrap();

        fn docker_app_input(server_id: Uuid, name: &str) -> CreateApplicationInput {
            CreateApplicationInput {
                server_id: Some(server_id),
                name: name.to_string(),
                description: None,
                blueprint_id: "generic-docker".to_string(),
                blueprint_version: 1,
                runtime_type: RuntimeType::Docker,
                working_directory: "/srv/app".to_string(),
                environment: vec![],
                ports: vec![],
                runtime_config: serde_json::json!({}),
                metadata: serde_json::json!({}),
            }
        }
        let app_a = app_repo.create(&docker_app_input(server.id, "App A")).unwrap();
        let app_b = app_repo.create(&docker_app_input(server.id, "App B")).unwrap();

        let published_by_a = crate::models::PortInput {
            name: "game".to_string(),
            protocol: crate::models::PortProtocol::Tcp,
            bind_address: "0.0.0.0".to_string(),
            internal_port: 25565,
            external_port: Some(25565),
            visibility: crate::models::PortVisibility::Public,
            required: false,
        };
        add_application_port(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, app_a.application.id, &published_by_a).await.unwrap();

        let colliding_from_b = crate::models::PortInput { internal_port: 25566, ..published_by_a.clone() };
        let err = add_application_port(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, app_b.application.id, &colliding_from_b).await.unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
        assert!(list_application_ports(&app_repo, app_b.application.id).unwrap().is_empty(), "the colliding port must never have been saved");

        // A different protocol on the same port number is not a collision.
        let different_protocol = crate::models::PortInput { protocol: crate::models::PortProtocol::Udp, ..colliding_from_b.clone() };
        assert!(add_application_port(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, app_b.application.id, &different_protocol).await.is_ok());

        // Re-saving App A's own port unchanged (e.g. editing its name) must
        // not collide against itself.
        let app_a_port = list_application_ports(&app_repo, app_a.application.id).unwrap().remove(0);
        let renamed = crate::models::PortInput { name: "renamed".to_string(), ..published_by_a };
        assert!(update_application_port(&app_repo, &server_repo, &network_repo, &firewall_rule_repo, &sessions, app_a.application.id, app_a_port.id, &renamed).await.is_ok());
    }

    fn create_raw(app_repo: &ApplicationRepository, runtime_type: RuntimeType, runtime_config: serde_json::Value) -> ApplicationDetail {
        // Bypasses `create_application`'s blueprint/runtime-type compatibility
        // check deliberately - Docker isn't one of the built-in blueprints'
        // `supported_runtime_types` yet (a real, separate gap, not this
        // test's concern), so this goes straight through the repository the
        // same way `runtime::docker`'s own unit tests build a stub
        // `Application` rather than going through the service layer.
        app_repo
            .create(&CreateApplicationInput {
                server_id: None,
                name: "Resource Limits Test App".to_string(),
                description: None,
                blueprint_id: "generic".to_string(),
                blueprint_version: 1,
                runtime_type,
                working_directory: std::env::temp_dir().to_string_lossy().into_owned(),
                environment: vec![],
                ports: vec![],
                runtime_config,
                metadata: serde_json::json!({}),
            })
            .unwrap()
    }

    /// Reproduces a real Application from before Paper/Velocity went
    /// Docker-only (Etap M1): the row still has `runtime_type =
    /// RemoteProcess` and its own already-working `runtime_config`
    /// (untouched by this test, and by `update_application_config` itself -
    /// see that function's own doc comment), but the blueprint that created
    /// it no longer lists RemoteProcess as supported.
    #[tokio::test]
    async fn update_application_config_rejects_an_application_whose_runtime_type_the_blueprint_no_longer_supports() {
        let (app_repo, server_repo, _network_repo, sessions, _local_process_manager, registry, ..) = temp_setup();
        let legacy_velocity = app_repo
            .create(&CreateApplicationInput {
                server_id: None,
                name: "Legacy Velocity".to_string(),
                description: None,
                blueprint_id: "velocity".to_string(),
                blueprint_version: 1,
                runtime_type: RuntimeType::RemoteProcess,
                working_directory: std::env::temp_dir().to_string_lossy().into_owned(),
                environment: vec![],
                ports: vec![],
                runtime_config: serde_json::json!({ "command": "java", "args": ["-jar", "velocity-3.4.0-566.jar"] }),
                metadata: serde_json::json!({}),
            })
            .unwrap();

        let err = update_application_config(&app_repo, &registry, &server_repo, &sessions, legacy_velocity.application.id, serde_json::json!({ "javaVersion": "25" }))
            .await
            .unwrap_err();

        assert!(matches!(err, AppError::InvalidInput(_)));
        // The Application's own config must be completely untouched - this
        // rejects before ever calling `render_runtime_config`, not after.
        let reloaded = get_application(&app_repo, legacy_velocity.application.id).unwrap();
        assert_eq!(reloaded.runtime_config, serde_json::json!({ "command": "java", "args": ["-jar", "velocity-3.4.0-566.jar"] }));
    }

    /// The real-infra regression test for the bug this session actually
    /// found: changing Velocity's own version field used to silently keep
    /// running the jar downloaded at creation, because `update_application_config`
    /// never re-ran `provision()` - see that function's own doc comment for
    /// the full explanation. A real network call against papermc.io, same
    /// "skip if unreachable" pattern this crate's other provision tests
    /// already use.
    #[tokio::test]
    async fn update_application_config_re_provisions_so_a_changed_version_downloads_a_different_jar() {
        let (app_repo, server_repo, _network_repo, sessions, _local_process_manager, registry, ..) = temp_setup();
        let working_directory = std::env::temp_dir().join(format!("vibessh-config-reprovision-test-{}", uuid::Uuid::new_v4()));

        let input = CreateApplicationFromBlueprintInput {
            server_id: None,
            name: "Version Change Test".to_string(),
            description: None,
            blueprint_id: "velocity".to_string(),
            runtime_type: RuntimeType::Docker,
            working_directory: working_directory.to_string_lossy().into_owned(),
            environment: vec![],
            blueprint_inputs: serde_json::json!({ "velocityVersion": "3.1.1" }),
        };

        let created = match create_application(&app_repo, &registry, &server_repo, &sessions, input).await {
            Ok(created) => created,
            Err(err) => {
                eprintln!("skipping: papermc.io unreachable from this environment ({err:?})");
                return;
            }
        };
        let first_jar = created.runtime_config["command"][2].as_str().unwrap().to_string();
        assert!(first_jar.contains("3.1.1"), "expected the 3.1.1 jar, got {first_jar}");

        let updated = update_application_config(
            &app_repo,
            &registry,
            &server_repo,
            &sessions,
            created.application.id,
            serde_json::json!({ "velocityVersion": "3.4.0" }),
        )
        .await
        .unwrap();

        let second_jar = updated.runtime_config["command"][2].as_str().unwrap().to_string();
        assert!(second_jar.contains("3.4.0"), "expected the 3.4.0 jar after changing the version, got {second_jar}");
        assert_ne!(first_jar, second_jar, "changing the version must actually change the downloaded jar");
        assert!(working_directory.join(&second_jar).is_file(), "the newly downloaded jar should exist in the working directory");

        tokio::fs::remove_dir_all(&working_directory).await.ok();
    }

    #[tokio::test]
    async fn recreate_application_rejects_a_non_docker_runtime_type() {
        let (app_repo, server_repo, _network_repo, sessions, local_process_manager, _registry, _firewall_rule_repo, registry_credential_repo, ..) = temp_setup();
        let local = create_raw(&app_repo, RuntimeType::LocalProcess, serde_json::json!({ "command": "sh", "args": [] }));

        let err = recreate_application(&app_repo, &server_repo, &sessions, &registry_credential_repo, &local_process_manager, local.application.id).await.unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }

    #[test]
    fn set_application_resource_limits_rejects_a_runtime_type_that_cant_enforce_them() {
        let (app_repo, _server_repo, _network_repo, _sessions, _local_process_manager, _registry, ..) = temp_setup();
        let local = create_raw(&app_repo, RuntimeType::LocalProcess, serde_json::json!({ "command": "sh", "args": [] }));

        let result = set_application_resource_limits(&app_repo, local.application.id, SetResourceLimitsInput { memory_limit_mb: Some(512), cpu_limit_cores: None });
        assert!(result.is_err());
    }

    #[test]
    fn set_application_resource_limits_patches_and_clears_the_docker_runtime_config() {
        let (app_repo, _server_repo, _network_repo, _sessions, _local_process_manager, _registry, ..) = temp_setup();
        let docker = create_raw(&app_repo, RuntimeType::Docker, serde_json::json!({ "image": "alpine:latest", "command": [] }));

        let updated = set_application_resource_limits(
            &app_repo,
            docker.application.id,
            SetResourceLimitsInput { memory_limit_mb: Some(512), cpu_limit_cores: Some(1.5) },
        )
        .unwrap();
        assert_eq!(updated.runtime_config["memoryLimitMb"], serde_json::json!(512));
        assert_eq!(updated.runtime_config["cpuLimitCores"], serde_json::json!(1.5));
        // The rest of the config (set at creation, untouched by this call)
        // must survive the patch - this isn't a full runtime_config replace.
        assert_eq!(updated.runtime_config["image"], serde_json::json!("alpine:latest"));

        let cleared =
            set_application_resource_limits(&app_repo, docker.application.id, SetResourceLimitsInput { memory_limit_mb: None, cpu_limit_cores: None }).unwrap();
        assert!(cleared.runtime_config.get("memoryLimitMb").is_none());
        assert!(cleared.runtime_config.get("cpuLimitCores").is_none());
    }

    #[test]
    fn set_application_resource_limits_rejects_a_zero_memory_limit() {
        let (app_repo, _server_repo, _network_repo, _sessions, _local_process_manager, _registry, ..) = temp_setup();
        let systemd = create_raw(&app_repo, RuntimeType::Systemd, serde_json::json!({ "command": "/usr/bin/java", "args": [] }));

        let result = set_application_resource_limits(&app_repo, systemd.application.id, SetResourceLimitsInput { memory_limit_mb: Some(0), cpu_limit_cores: None });
        assert!(result.is_err());
    }

    #[test]
    fn set_application_image_patches_the_docker_runtime_config_and_leaves_the_rest_untouched() {
        let (app_repo, _server_repo, _network_repo, _sessions, _local_process_manager, _registry, ..) = temp_setup();
        let docker = create_raw(&app_repo, RuntimeType::Docker, serde_json::json!({ "image": "alpine:latest", "command": ["sleep", "999"] }));

        let updated = set_application_image(&app_repo, docker.application.id, "  eclipse-temurin:25-jre-alpine  ".to_string()).unwrap();
        assert_eq!(updated.runtime_config["image"], serde_json::json!("eclipse-temurin:25-jre-alpine"));
        assert_eq!(updated.runtime_config["command"], serde_json::json!(["sleep", "999"]));
    }

    #[test]
    fn set_application_image_rejects_a_non_docker_runtime_type() {
        let (app_repo, _server_repo, _network_repo, _sessions, _local_process_manager, _registry, ..) = temp_setup();
        let local = create_raw(&app_repo, RuntimeType::LocalProcess, serde_json::json!({ "command": "sh", "args": [] }));

        assert!(set_application_image(&app_repo, local.application.id, "alpine:latest".to_string()).is_err());
    }

    #[test]
    fn set_application_image_rejects_a_blank_or_newline_containing_image() {
        let (app_repo, _server_repo, _network_repo, _sessions, _local_process_manager, _registry, ..) = temp_setup();
        let docker = create_raw(&app_repo, RuntimeType::Docker, serde_json::json!({ "image": "alpine:latest", "command": [] }));

        assert!(set_application_image(&app_repo, docker.application.id, "   ".to_string()).is_err());
        assert!(set_application_image(&app_repo, docker.application.id, "alpine:latest\nrm -rf /".to_string()).is_err());
    }

    #[tokio::test]
    async fn pull_application_image_rejects_a_non_docker_runtime_type() {
        let (app_repo, server_repo, _network_repo, sessions, _local_process_manager, _registry, _firewall_rule_repo, registry_credential_repo, ..) = temp_setup();
        let local = create_raw(&app_repo, RuntimeType::LocalProcess, serde_json::json!({ "command": "sh", "args": [] }));

        let err = pull_application_image(&app_repo, &server_repo, &sessions, &registry_credential_repo, local.application.id).await.unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn pull_application_image_rejects_a_docker_application_with_no_image_configured() {
        let (app_repo, server_repo, _network_repo, sessions, _local_process_manager, _registry, _firewall_rule_repo, registry_credential_repo, ..) = temp_setup();
        let docker = create_raw(&app_repo, RuntimeType::Docker, serde_json::json!({ "command": [] }));

        let err = pull_application_image(&app_repo, &server_repo, &sessions, &registry_credential_repo, docker.application.id).await.unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }

    /// End-to-end through the real OS keyring (guarded by the same
    /// process-wide lock every other keyring-touching test in this crate
    /// takes - see `storage::credentials::KEYRING_TEST_LOCK`'s own doc
    /// comment), covering the whole life of a secret environment variable:
    /// never plaintext on a normal read, resolved back only for an actual
    /// runtime, "leave blank to keep" on edit, and cleaned up both when
    /// removed and when the Application itself is deleted.
    #[tokio::test]
    async fn secret_environment_variables_never_leak_plaintext_and_round_trip_through_the_keyring() {
        let _guard = crate::storage::credentials::KEYRING_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (app_repo, server_repo, _network_repo, sessions, local_process_manager, registry, _firewall_rule_repo, _registry_credential_repo, log_capture) = temp_setup();

        let mut input = sleep_command_input();
        input.environment = vec![
            EnvironmentVariable { key: "PLAIN".into(), value: "visible".into(), is_secret: false },
            EnvironmentVariable { key: "DB_PASSWORD".into(), value: "hunter2".into(), is_secret: true },
        ];
        let created = create_application(&app_repo, &registry, &server_repo, &sessions, input).await.unwrap();
        let id = created.application.id;
        struct Cleanup(Uuid);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = crate::storage::credentials::delete_environment_secret(self.0, "DB_PASSWORD");
            }
        }
        let _cleanup = Cleanup(id);

        // A plain read - what every Tauri command hands to the frontend -
        // never carries the secret's real value, only that it is one.
        let redacted = get_application(&app_repo, id).unwrap();
        let secret_row = redacted.environment.iter().find(|e| e.key == "DB_PASSWORD").unwrap();
        assert!(secret_row.is_secret);
        assert_eq!(secret_row.value, "");
        assert_eq!(redacted.environment.iter().find(|e| e.key == "PLAIN").unwrap().value, "visible");

        // An actual runtime (about to start the process) resolves the real
        // value back in.
        let (runtime_detail, _connection, _runtime) = load_runtime(&app_repo, &server_repo, &sessions, &local_process_manager, id).await.unwrap();
        assert_eq!(runtime_detail.environment.iter().find(|e| e.key == "DB_PASSWORD").unwrap().value, "hunter2");

        // Editing another field without retyping the secret (the frontend
        // never has the real value to resend) preserves it.
        set_application_environment(
            &app_repo,
            id,
            vec![
                EnvironmentVariable { key: "PLAIN".into(), value: "still-visible".into(), is_secret: false },
                EnvironmentVariable { key: "DB_PASSWORD".into(), value: "".into(), is_secret: true },
            ],
        )
        .unwrap();
        let (runtime_detail, _connection, _runtime) = load_runtime(&app_repo, &server_repo, &sessions, &local_process_manager, id).await.unwrap();
        assert_eq!(runtime_detail.environment.iter().find(|e| e.key == "DB_PASSWORD").unwrap().value, "hunter2");

        // Removing the key deletes its keyring entry rather than leaving it
        // orphaned forever.
        set_application_environment(&app_repo, id, vec![EnvironmentVariable { key: "PLAIN".into(), value: "still-visible".into(), is_secret: false }])
            .unwrap();
        assert_eq!(crate::storage::credentials::load_environment_secret(id, "DB_PASSWORD").unwrap(), None);

        // Deleting the Application cleans up any secret still attached to it.
        set_application_environment(&app_repo, id, vec![EnvironmentVariable { key: "DB_PASSWORD".into(), value: "again".into(), is_secret: true }]).unwrap();
        delete_application(&app_repo, &log_capture, id).await.unwrap();
        assert_eq!(crate::storage::credentials::load_environment_secret(id, "DB_PASSWORD").unwrap(), None);
    }
}
