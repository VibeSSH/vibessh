//! Creating an Application, and changing the configuration it was created
//! from.
//!
//! These two belong together because they answer the same question at
//! different times - what should exist on the Node - and because a config
//! change is the one path that has to decide whether an already-running
//! container needs recreating.//!
//! Split out of a single 2685-line `application_service` (FIX_PLAN E.7).
//! Behaviour is unchanged; only the file boundaries moved.

use std::collections::HashMap;

use uuid::Uuid;

use crate::blueprints::{BlueprintRegistry, ProvisionContext};
use crate::errors::{AppError, AppResult};
use crate::models::{
    ApplicationDetail, CreateApplicationFromBlueprintInput,
    CreateApplicationInput, EnvironmentVariable, PortInput, PortVisibility, UpdateApplicationInput,
};
use crate::services::ssh_service::{get_or_connect, retry_on_connection_failure};
// The one shared implementation - this module used to carry its own
// byte-identical copy, one of six across the codebase.
use crate::ssh::command::quote as shell_quote;
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::credentials;
use crate::storage::server_repository::ServerRepository;

use super::*;

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
    // Shared by every Application rather than one copy each - see
    // `services::java_runtime_service`.
    java_root: &std::path::Path,
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
    let provision_context = ProvisionContext { working_directory, connection, runtime_type: input.runtime_type, java_root };
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

    // The chosen target's own env rows, resolved before anything is written
    // so a bad target fails the create rather than leaving an Application
    // that half-points at something.
    let environment = match (handler.blueprint().connects_to.as_ref(), input.connect_to_application_id) {
        (Some(connection), Some(peer_id)) => connection_environment(repo, connection, peer_id, input.environment)?,
        _ => input.environment,
    };

    let create_input = CreateApplicationInput {
        server_id: input.server_id,
        name: name.to_string(),
        description: input.description,
        blueprint_id: input.blueprint_id,
        blueprint_version: handler.blueprint().blueprint_version,
        runtime_type: input.runtime_type,
        working_directory: working_directory.to_string(),
        environment,
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

    // Granted last, and only after the Application exists to grant it to.
    //
    // The row alone is enough here: nothing is running yet, and
    // `runtime::docker::reconcile_networks` builds the shared network from
    // the stored allow-list on every start. So there is no Docker call to
    // make and none to fail - unlike `links::connect_applications`, which
    // has live containers to attach and therefore has to apply as well.
    if let Some(peer_id) = input.connect_to_application_id {
        if handler.blueprint().connects_to.is_some() {
            grant_connection(repo, detail.application.id, peer_id)?;
        }
    }
    Ok(detail)
}

/// Turns "point this at that Application" into the environment rows the
/// image itself reads.
///
/// The host is the target's **network alias**, not its Application name as
/// typed: `MariaDB (EU)` is reachable as `mariadb-eu`, and somebody typing
/// the name they can see into PMA_HOST gets a host that does not resolve.
/// That single mismatch is most of why this setup is reported broken, and it
/// is exactly the kind of thing the machine should fill in.
///
/// A row the user typed themselves wins. Somebody who has already put a
/// PMA_HOST in the wizard meant it, and silently overwriting it would be
/// worse than ignoring the picker.
fn connection_environment(
    repo: &ApplicationRepository,
    connection: &crate::models::BlueprintConnection,
    peer_id: Uuid,
    mut environment: Vec<EnvironmentVariable>,
) -> AppResult<Vec<EnvironmentVariable>> {
    let peer = repo.get(peer_id)?.ok_or_else(|| AppError::NotFound(format!("application {peer_id}")))?;
    if !connection.blueprint_ids.is_empty() && !connection.blueprint_ids.contains(&peer.application.blueprint_id) {
        return Err(AppError::InvalidInput(format!("'{}' isn't a kind of application this one can connect to", peer.application.name)));
    }

    let already_set = |key: &str, rows: &[EnvironmentVariable]| rows.iter().any(|row| row.key == key);
    if !already_set(&connection.host_env, &environment) {
        environment.push(EnvironmentVariable {
            key: connection.host_env.clone(),
            value: crate::runtime::docker::network_alias(&peer.application),
            is_secret: false,
        });
    }
    if !already_set(&connection.port_env, &environment) {
        environment.push(EnvironmentVariable {
            key: connection.port_env.clone(),
            value: connection.default_port.to_string(),
            is_secret: false,
        });
    }
    Ok(environment)
}

/// Records that these two are allowed to reach each other.
///
/// A failure here is logged rather than returned: the Application has been
/// created and its environment already names the target, so failing the
/// whole create would destroy something that exists for the sake of a grant
/// the user can add in one click on the Connections card. The log is what
/// makes the difference visible if it ever happens.
fn grant_connection(repo: &ApplicationRepository, id: Uuid, peer_id: Uuid) -> AppResult<()> {
    if let Err(err) = repo.add_link(id, peer_id) {
        log::error!("created application {id} but couldn't grant its connection to {peer_id}: {err}");
    }
    Ok(())
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
/// Rebuilds an Application's `runtime_config` from today's blueprint logic
/// and its own stored answers.
///
/// **Why this is not part of `update_application_config`.** That one
/// re-provisions - it downloads the jar again, because a changed Minecraft
/// version needs a different one. Nothing here has changed for the
/// Application; what has changed is how a blueprint renders the same
/// answers. So the discovered values are read back from where creation left
/// them (`metadata.blueprintInputs` holds the merged set, jar filename
/// included) and nothing is fetched.
///
/// This is the only way an improvement to a blueprint reaches an Application
/// that already exists. Recreating a container rebuilt it from the stored
/// command, so a fix to how that command is built could never arrive - the
/// button promised that a changed command would take effect, and for
/// anything but a hand-edited field it did not.
///
/// `Ok(None)` when there is nothing to re-render or it would not be safe to:
/// an Application not made from a blueprint, one whose blueprint is gone, or
/// one whose runtime type the blueprint no longer supports - the last for the
/// same reason `update_application_config` refuses it, since today's renderer
/// would produce a shape that runtime was never designed for.
pub fn rerender_runtime_config(
    registry: &BlueprintRegistry,
    detail: &ApplicationDetail,
) -> AppResult<Option<serde_json::Value>> {
    let Some(handler) = registry.get(&detail.application.blueprint_id) else { return Ok(None) };
    if !handler.blueprint().supported_runtime_types.contains(&detail.application.runtime_type) {
        return Ok(None);
    }

    let inputs: HashMap<String, serde_json::Value> = match detail.metadata.get("blueprintInputs") {
        Some(serde_json::Value::Object(map)) => map.clone().into_iter().collect(),
        // Nothing recorded, so there is nothing to render from - an
        // Application created before this was stored, or not from a
        // blueprint at all.
        _ => return Ok(None),
    };

    let rendered = handler.render_runtime_config(&inputs)?;
    Ok((rendered != detail.runtime_config).then_some(rendered))
}

/// Moves an Application to a different blueprint, in either direction.
///
/// **Both directions matter, and they are not symmetrical.** Going to a
/// managed blueprint - Paper, Velocity - hands version management over, and
/// its provisioning is what does that: it downloads that server's own jar
/// into a directory somebody may already be running a server out of. Going
/// the other way, to a plain Docker Application, gives management up and
/// touches nothing: the files stay exactly as they are and the jar that is
/// there keeps running.
///
/// So the download is the thing to be warned about, and it belongs to the
/// direction that causes it rather than to this function, which cannot know
/// what the caller was told.
///
/// The runtime type does not change. A blueprint that cannot run the way this
/// Application already runs is refused rather than quietly rewritten into a
/// shape its runtime was never designed for - the same reasoning
/// `update_application_config` uses to refuse editing one whose blueprint
/// narrowed underneath it.
pub async fn change_application_blueprint(
    repo: &ApplicationRepository,
    registry: &BlueprintRegistry,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    java_root: &std::path::Path,
    id: Uuid,
    blueprint_id: &str,
    field_values: serde_json::Value,
) -> AppResult<ApplicationDetail> {
    let detail = repo.get(id)?.ok_or_else(|| AppError::NotFound(format!("application {id}")))?;
    if detail.application.blueprint_id == blueprint_id {
        return Ok(detail);
    }

    let handler = registry
        .get(blueprint_id)
        .ok_or_else(|| AppError::InvalidInput(format!("unknown blueprint '{blueprint_id}'")))?;
    if !handler.blueprint().supported_runtime_types.contains(&detail.application.runtime_type) {
        return Err(AppError::InvalidInput(format!(
            "'{}' can't run the way this application already runs",
            handler.blueprint().name
        )));
    }

    // The new blueprint's own answers only. Carrying the old ones over would
    // mean a Paper Application holding a `generic-docker` image field, which
    // nothing reads and everything has to then ignore.
    let mut inputs: HashMap<String, serde_json::Value> = match field_values {
        serde_json::Value::Object(map) => map.into_iter().collect(),
        serde_json::Value::Null => HashMap::new(),
        _ => return Err(AppError::InvalidInput("blueprint inputs must be an object".into())),
    };

    let connection = resolve_connection(server_repo, sessions, detail.application.server_id).await?;
    let provision_context = ProvisionContext {
        working_directory: &detail.application.working_directory,
        connection,
        runtime_type: detail.application.runtime_type,
        java_root,
    };
    let discovered = handler.provision(&inputs, &provision_context).await?;
    inputs.extend(discovered);

    let runtime_config = handler.render_runtime_config(&inputs)?;

    // The blueprint is set last. Everything before it can fail - a download,
    // a rejected input - and an Application left claiming to be a Paper
    // server while still holding a generic config would be worse than one
    // that never changed.
    repo.update(
        id,
        &UpdateApplicationInput {
            name: detail.application.name.clone(),
            description: detail.application.description.clone(),
            working_directory: detail.application.working_directory.clone(),
            runtime_config,
            metadata: serde_json::json!({ "blueprintInputs": inputs }),
        },
    )?;
    repo.set_blueprint(id, blueprint_id, handler.blueprint().blueprint_version)?;
    repo.get(id)?.ok_or_else(|| AppError::NotFound(format!("application {id}")))
}

pub async fn update_application_config(
    repo: &ApplicationRepository,
    registry: &BlueprintRegistry,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    java_root: &std::path::Path,
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
    let provision_context = ProvisionContext { working_directory: &detail.application.working_directory, connection, runtime_type: detail.application.runtime_type, java_root };
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

pub(crate) async fn ensure_working_directory_exists(
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

#[cfg(test)]
mod rerender_tests {
    use super::*;
    use crate::models::{Application, ApplicationStatus, HealthCheckType, RuntimeType};

    fn detail(blueprint_id: &str, runtime_config: serde_json::Value, inputs: serde_json::Value) -> ApplicationDetail {
        ApplicationDetail {
            application: Application {
                id: Uuid::new_v4(),
                server_id: Some(Uuid::new_v4()),
                name: "lobby".to_string(),
                description: None,
                blueprint_id: blueprint_id.to_string(),
                blueprint_version: 1,
                runtime_type: RuntimeType::Docker,
                working_directory: "/srv/lobby".to_string(),
                status: ApplicationStatus::Stopped,
                last_status_check_at: None,
                health_check_type: HealthCheckType::Process,
                health_check_port_id: None,
                health_check_http_path: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            environment: vec![],
            ports: vec![],
            runtime_config,
            metadata: serde_json::json!({ "blueprintInputs": inputs }),
            links: vec![],
        }
    }

    /// The case this exists for. An Application created before a blueprint
    /// changed carries the command it was given then, and recreating its
    /// container rebuilt it from exactly that - so an improvement to how the
    /// command is built could never reach it.
    #[test]
    fn a_stale_command_is_rebuilt_from_todays_blueprint() {
        let registry = BlueprintRegistry::with_builtins();
        let stale = serde_json::json!({
            "image": "eclipse-temurin:21-jre",
            "command": ["java", "-jar", "paper-1.21.11-132.jar", "nogui"],
            "runAsDedicatedUser": true
        });
        let inputs = serde_json::json!({
            "minecraftVersion": "1.21.11",
            "javaVersion": "21",
            "eulaAccepted": true,
            "jvmArgs": [],
            "programArgs": ["nogui"],
            "__jarFilename": "paper-1.21.11-132.jar"
        });

        let rebuilt = rerender_runtime_config(&registry, &detail("paper", stale, inputs)).unwrap();

        let rebuilt = rebuilt.expect("the command should have been rebuilt");
        let command = rebuilt["command"].as_array().unwrap();
        assert!(command.iter().any(|arg| arg == "-Dterminal.ansi=true"), "{rebuilt}");
    }

    /// Nothing to say when the stored command already matches - the caller
    /// writes nothing and recreates with what it has.
    #[test]
    fn an_up_to_date_command_is_left_alone() {
        let registry = BlueprintRegistry::with_builtins();
        let inputs = serde_json::json!({
            "minecraftVersion": "1.21.11",
            "javaVersion": "21",
            "eulaAccepted": true,
            "jvmArgs": [],
            "programArgs": ["nogui"],
            "__jarFilename": "paper-1.21.11-132.jar"
        });
        let current = rerender_runtime_config(&registry, &detail("paper", serde_json::json!({}), inputs.clone()))
            .unwrap()
            .expect("a first render");

        let again = rerender_runtime_config(&registry, &detail("paper", current, inputs)).unwrap();

        assert!(again.is_none());
    }

    #[test]
    fn an_application_with_no_recorded_answers_is_left_alone() {
        let registry = BlueprintRegistry::with_builtins();
        let mut without = detail("paper", serde_json::json!({}), serde_json::json!({}));
        without.metadata = serde_json::json!({});

        assert!(rerender_runtime_config(&registry, &without).unwrap().is_none());
    }

    #[test]
    fn an_unknown_blueprint_is_left_alone() {
        let registry = BlueprintRegistry::with_builtins();

        let detail = detail("something-that-was-removed", serde_json::json!({}), serde_json::json!({}));

        assert!(rerender_runtime_config(&registry, &detail).unwrap().is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{BlueprintConnection, CreateApplicationInput, RuntimeType};

    fn temp_repo() -> ApplicationRepository {
        let path = std::env::temp_dir().join(format!("vibessh-connect-test-{}.sqlite3", Uuid::new_v4()));
        ApplicationRepository::open(&path).unwrap()
    }

    fn create(repo: &ApplicationRepository, name: &str, blueprint_id: &str) -> Uuid {
        repo.create(&CreateApplicationInput {
            server_id: None,
            name: name.to_string(),
            description: None,
            blueprint_id: blueprint_id.to_string(),
            blueprint_version: 1,
            runtime_type: RuntimeType::Docker,
            working_directory: "/tmp/db".to_string(),
            environment: vec![],
            ports: vec![],
            runtime_config: serde_json::json!({}),
            metadata: serde_json::json!({}),
        })
        .unwrap()
        .application
        .id
    }

    fn phpmyadmin_connection() -> BlueprintConnection {
        BlueprintConnection {
            blueprint_ids: vec!["mariadb".to_string()],
            host_env: "PMA_HOST".to_string(),
            port_env: "PMA_PORT".to_string(),
            default_port: 3306,
        }
    }

    fn value_of<'a>(rows: &'a [EnvironmentVariable], key: &str) -> Option<&'a str> {
        rows.iter().find(|row| row.key == key).map(|row| row.value.as_str())
    }

    /// The whole point. A name with a space in it is reachable by its
    /// slugified alias and by nothing else, so typing the name you can see
    /// into PMA_HOST is the mistake this exists to stop somebody making.
    #[test]
    fn the_host_is_the_targets_network_alias_not_its_name() {
        let repo = temp_repo();
        let peer = create(&repo, "MariaDB (EU)", "mariadb");

        let rows = connection_environment(&repo, &phpmyadmin_connection(), peer, vec![]).unwrap();

        assert_eq!(value_of(&rows, "PMA_HOST"), Some("mariadb-eu"));
        assert_eq!(value_of(&rows, "PMA_PORT"), Some("3306"));
    }

    /// The port a database publishes to the host is a different question
    /// from the port it listens on inside its own container, and this is
    /// the second one - a database deliberately left unpublished is still
    /// on 3306 to something that has been granted a route to it.
    #[test]
    fn the_port_is_the_containers_own_regardless_of_what_it_publishes() {
        let repo = temp_repo();
        let peer = create(&repo, "db", "mariadb");

        let rows = connection_environment(&repo, &phpmyadmin_connection(), peer, vec![]).unwrap();

        assert_eq!(value_of(&rows, "PMA_PORT"), Some("3306"));
    }

    #[test]
    fn a_value_the_user_typed_themselves_is_left_alone() {
        let repo = temp_repo();
        let peer = create(&repo, "db", "mariadb");
        let typed = vec![EnvironmentVariable { key: "PMA_HOST".into(), value: "db.example.com".into(), is_secret: false }];

        let rows = connection_environment(&repo, &phpmyadmin_connection(), peer, typed).unwrap();

        assert_eq!(value_of(&rows, "PMA_HOST"), Some("db.example.com"));
        // The half they did not answer is still filled in.
        assert_eq!(value_of(&rows, "PMA_PORT"), Some("3306"));
        assert_eq!(rows.iter().filter(|row| row.key == "PMA_HOST").count(), 1, "the row was duplicated rather than kept");
    }

    #[test]
    fn pointing_it_at_something_that_is_not_a_database_is_refused() {
        let repo = temp_repo();
        let peer = create(&repo, "Paper Survival", "paper");

        let err = connection_environment(&repo, &phpmyadmin_connection(), peer, vec![]).unwrap_err();

        assert!(matches!(err, AppError::InvalidInput(_)), "expected InvalidInput, got {err:?}");
    }

    #[test]
    fn pointing_it_at_an_application_that_is_gone_is_refused() {
        let repo = temp_repo();

        let err = connection_environment(&repo, &phpmyadmin_connection(), Uuid::new_v4(), vec![]).unwrap_err();

        assert!(matches!(err, AppError::NotFound(_)), "expected NotFound, got {err:?}");
    }

    /// A blueprint that names no targets accepts any - the escape hatch for
    /// something genuinely able to talk to anything, and worth a test so
    /// the empty list is not read as "nothing is allowed".
    #[test]
    fn an_empty_target_list_accepts_anything() {
        let repo = temp_repo();
        let peer = create(&repo, "whatever", "generic-docker");
        let connection = BlueprintConnection { blueprint_ids: vec![], ..phpmyadmin_connection() };

        let rows = connection_environment(&repo, &connection, peer, vec![]).unwrap();

        assert_eq!(value_of(&rows, "PMA_HOST"), Some("whatever"));
    }
}
