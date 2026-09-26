//! Carrying out an agreed Pterodactyl migration plan.
//!
//! The plan (`pterodactyl_import_service`) decides; this does. Keeping the
//! two apart is what makes the plan worth showing: nothing here invents a
//! mapping or picks a version - it is handed the decisions and executes
//! them.
//!
//! **The order is not arbitrary.** For each server:
//!
//! 1. Stop it on the panel's side. A Minecraft world or a database directory
//!    copied while its process is still writing can be corrupt, and there is
//!    no way around that short of stopping it. This is why the operator was
//!    asked to accept downtime per server before any of this was built.
//! 2. Create the Application, through exactly the same service the Create
//!    Application wizard uses - so provisioning, validation and the Node-side
//!    directory all behave as they do for a hand-made Application.
//! 3. Give it its ports and limits.
//! 4. Copy the files.
//!
//! **Nothing is started at the end.** Thirty servers coming up at once on a
//! machine that was hosting none of them a minute ago is not a good default,
//! and starting is the one step the operator can trivially do themselves, one
//! at a time, watching the logs. A migration that ends with everything
//! created and stopped is reversible; one that ends with everything running
//! is not.
//!
//! **The panel is never written to.** Servers are stopped through Docker on
//! their own node, which is visible and reversible, rather than through the
//! panel's API - see `pterodactyl::client`'s own doc comment.

use uuid::Uuid;

use crate::blueprints::BlueprintRegistry;
use crate::errors::{AppError, AppResult};
use crate::files::{sftp::SftpApplicationFileProvider, ApplicationFileProvider};
use crate::models::{CreateApplicationFromBlueprintInput, EnvironmentVariable, PortInput, PortProtocol, PortVisibility, RuntimeType, SetResourceLimitsInput};
use crate::pterodactyl::mapping::PlanNote;
use crate::services::application_service;
use crate::services::database_service;
use crate::services::pterodactyl_import_service::{PlannedDatabase, PlannedServer};
use crate::services::ssh_service::get_or_connect;
use crate::ssh::command::quote as shell_quote;
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::database_repository::DatabaseRepository;
use crate::storage::firewall_rule_repository::FirewallRuleRepository;
use crate::storage::node_network_repository::NodeNetworkRepository;
use crate::storage::server_repository::ServerRepository;

/// How long to give a server to shut down before giving up on a clean stop.
///
/// Generous on purpose: a large Minecraft world can take a while to save, and
/// the entire reason for stopping it is to let it finish writing. Killing it
/// early would defeat the step.
const STOP_TIMEOUT_SECONDS: u32 = 120;

/// Which step of one server's import is happening, for the progress the
/// operator watches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportStep {
    Stopping,
    Creating,
    Configuring,
    CopyingFiles,
    MovingDatabases,
    Done,
}

/// What happened to one server.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcome {
    pub source_id: i64,
    pub name: String,
    /// The Application that now exists here, when one does.
    pub application_id: Option<Uuid>,
    pub files_copied: u64,
    /// Things that went wrong but did not stop the import.
    pub warnings: Vec<PlanNote>,
    /// The databases that moved, by their new VibeSSH names. The panel
    /// names are not reused: VibeSSH generates its own, and a database this
    /// app did not name is one it cannot manage.
    pub databases_moved: Vec<String>,
    /// Set when this server did not migrate. The others still did - one
    /// failure must not abandon a panel halfway.
    ///
    /// A note rather than a string, for the same reason everything else here
    /// is: it has to be readable in the operator's own language, and it has
    /// to be able to carry the specifics (which machine, which port, which
    /// error) that make it actionable.
    pub failed: Option<PlanNote>,
}

/// Pterodactyl names each container after the server's uuid.
///
/// Validated rather than trusted: this string came from an HTTP response and
/// is about to become part of a command on the operator's machine. A uuid is
/// a closed shape, so checking it is cheap and exact - see
/// `ssh::command`'s own doc comment for why nothing here is ever concatenated
/// unchecked.
fn container_name(source_uuid: &str) -> AppResult<String> {
    let parsed = Uuid::parse_str(source_uuid)
        .map_err(|_| AppError::InvalidInput(format!("the panel gave a server id that is not a uuid: {source_uuid}")))?;
    Ok(parsed.to_string())
}

/// Stops the server on the panel's node, and proves it stopped.
///
/// A container that is already stopped, or that no longer exists, satisfies
/// the goal - "not writing to its files" - so neither is a failure. Anything
/// else is, and is returned so the copy never starts.
///
/// **This used to end in `|| true`, and to call `docker` without `sudo`.**
/// Every other Docker call in this codebase goes through `sudo docker`,
/// because the connecting SSH user is not necessarily in the `docker` group;
/// without it the stop failed, and `|| true` meant nothing could see that.
/// The import then copied a running server's world and database, which is
/// precisely what stopping exists to prevent and is not detectable
/// afterwards. Hence the verification below: this returns `Ok` only when the
/// container is provably not running.
async fn stop_on_the_panel(
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    source_server_id: Uuid,
    source_uuid: &str,
) -> AppResult<()> {
    let container = container_name(source_uuid)?;
    let connection = get_or_connect(server_repo, sessions, source_server_id).await?;

    let stop = connection
        .execute_command(&format!("sudo docker stop --time {STOP_TIMEOUT_SECONDS} {}", shell_quote(&container)))
        .await?;
    if stop.exit_code != 0 && !stop.stderr.contains("No such container") {
        return Err(AppError::Connection(format!("couldn't stop the panel's container: {}", stop.stderr.trim())));
    }

    // Asked rather than assumed. `docker inspect` prints `true`/`false` for a
    // container that exists and fails for one that does not - and "it does
    // not exist" is a perfectly good answer to "is it still writing".
    let state = connection
        .execute_command(&format!("sudo docker inspect -f '{{{{.State.Running}}}}' {}", shell_quote(&container)))
        .await?;
    if state.exit_code == 0 && state.stdout.trim() == "true" {
        return Err(AppError::Connection(
            "the panel's container is still running after being asked to stop, so its files would be copied mid-write".to_string(),
        ));
    }
    Ok(())
}

/// Copies the panel's volume into the new Application's working directory.
///
/// Two paths, because they are genuinely different operations. When the
/// panel's node and the target Node are the same machine, the whole copy is
/// one `cp -a` on that machine and no byte travels over the network. When
/// they are not, there is nowhere for the bytes to go except through this
/// desktop, file by file, which is slow but correct and is the only thing
/// available without assuming the two Nodes can reach each other.
async fn copy_volume(
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    source_server_id: Uuid,
    volume_path: &str,
    target_server_id: Uuid,
    working_directory: &str,
) -> AppResult<u64> {
    if source_server_id == target_server_id {
        let connection = get_or_connect(server_repo, sessions, target_server_id).await?;
        // `/.` copies the directory's *contents*, including dotfiles, rather
        // than nesting the volume inside the working directory. `-a`
        // preserves modes and timestamps, which matters for a world that a
        // server will compare against its own region files.
        let output = connection.execute_command_with_timeout(&same_machine_copy_command(volume_path, working_directory), VOLUME_COPY_TIMEOUT).await?;
        return copied_file_count(&output);
    }

    let source_connection = get_or_connect(server_repo, sessions, source_server_id).await?;
    let target_connection = get_or_connect(server_repo, sessions, target_server_id).await?;
    let source = SftpApplicationFileProvider::new(source_connection, volume_path.to_string());
    let target = SftpApplicationFileProvider::new(target_connection, working_directory.to_string());
    copy_tree(&source, &target).await
}

/// How long a same-machine volume copy may take. A world of tens of GB on a
/// slow disk is a long `cp`; the old ten-minute command limit cut it off.
const VOLUME_COPY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(4 * 60 * 60);

/// The copy, as root, and the count of what arrived.
///
/// **As root**, because Wings' volumes belong to its own `pterodactyl`
/// account (mode 0700 on most panels): an admin who is not root could not
/// read them, and the copy found nothing to copy. `-a` keeps modes and
/// timestamps - a world compares its own region files - and the start that
/// follows hands the directory to the Application's account.
///
/// `/.` copies the directory's *contents*, including dotfiles, rather than
/// nesting the volume inside the working directory.
fn same_machine_copy_command(volume_path: &str, working_directory: &str) -> String {
    format!(
        "sudo cp -a {volume}/. {target}/ && sudo find {target} -type f | wc -l",
        volume = shell_quote(volume_path),
        target = shell_quote(working_directory),
    )
}

/// How many files the copy left in place - or why it failed.
///
/// The exit code used to be ignored: a `cp` that could not read the volume
/// never reached `find`, the empty output parsed as zero, and the import
/// went on to start an Application with an empty directory - a brand-new
/// world in place of the one being moved, reported as a success. A failed
/// copy is now an error with `cp`'s own words, and the import stops there.
fn copied_file_count(output: &crate::transport::CommandOutput) -> AppResult<u64> {
    if output.exit_code != 0 {
        let detail = output.stderr.trim();
        return Err(AppError::Connection(if detail.is_empty() {
            "couldn't copy the server's files".to_string()
        } else {
            format!("couldn't copy the server's files: {detail}")
        }));
    }
    output
        .stdout
        .trim()
        .parse::<u64>()
        .map_err(|_| AppError::Connection(format!("couldn't count the copied files (got {:?})", output.stdout.trim())))
}

/// Mirrors one provider's tree onto another, a file at a time.
///
/// The same shape `migration_service::copy_directory` uses, and for the same
/// reason: a directory is created during its parent's turn, so by the time
/// this descends into it the parent is guaranteed to exist.
async fn copy_tree(source: &dyn ApplicationFileProvider, target: &dyn ApplicationFileProvider) -> AppResult<u64> {
    let mut copied = 0u64;
    let mut pending = vec![String::new()];
    while let Some(dir) = pending.pop() {
        for entry in source.list_directory(&dir).await? {
            let relative = if dir.is_empty() { entry.name.clone() } else { format!("{dir}/{}", entry.name) };
            if entry.is_dir {
                target.create_directory(&relative).await?;
                pending.push(relative);
            } else {
                let bytes = source.read_file(&relative).await?;
                target.write_file(&relative, &bytes).await?;
                copied += 1;
            }
        }
    }
    Ok(copied)
}

/// Moves one database: dump it where it lives, get the dump to the machine
/// that will hold it, create the destination through VibeSSH own
/// provisioning, then load it in.
///
/// **Why a dump and not a copy of the data directory.** A MySQL data
/// directory is only consistent for the server that wrote it, and the
/// destination is a different server that will hold the same schema under a
/// different name. A logical dump is the only thing that survives both.
///
/// **Why `sudo mysqldump` and not a connection as the panel own user.**
/// Pterodactyl never returns database passwords through its API, so there is
/// no credential for that user to connect with. Root on the database machine
/// authenticates through the local socket on every default Debian and Ubuntu
/// MySQL or MariaDB install, which needs no password and puts no secret in a
/// command string.
async fn move_one_database(
    db_repo: &DatabaseRepository,
    app_repo: &ApplicationRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    planned: &PlannedDatabase,
    application_id: Uuid,
    target_database_host_id: Uuid,
) -> Result<String, PlanNote> {
    let Some(source_host_server_id) = planned.host_server_id else {
        return Err(PlanNote::with(
            "databaseHostUnknown",
            &[("database", &planned.name), ("host", if planned.host_address.is_empty() { "?" } else { &planned.host_address })],
        ));
    };
    // The panel generated names are always plain, so anything else is either
    // hand-made or something worth refusing before it reaches a command.
    if planned.name.is_empty() || !planned.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(PlanNote::with("databaseNameRefused", &[("database", &planned.name)]));
    }

    let dump_path = format!(".vibessh-ptero-dump-{}.sql", Uuid::new_v4());
    let source = get_or_connect(server_repo, sessions, source_host_server_id)
        .await
        .map_err(|err| PlanNote::with("databaseDumpFailed", &[("database", &planned.name), ("error", &err.to_string())]))?;

    // Real credentials first, socket auth second.
    //
    // `sudo mysqldump <db>` only works where MySQL's root authenticates
    // through the local socket. A server whose root has a password refuses
    // it - and that is exactly the kind of server somebody registers in
    // VibeSSH as a database host, admin password included. Those credentials
    // are already stored, so they are tried first; `sudo` remains for the
    // socket-auth installs where no such host is configured.
    let credentialed =
        database_service::dump_database_to_file(db_repo, server_repo, sessions, target_database_host_id, &planned.name, &dump_path).await;

    if let Err(credentialed_error) = credentialed {
        // Created 0600 *before* the dump is written into it: a dump is a
        // full copy of somebody's data, and a redirect alone would create
        // the file with whatever the login shell's umask happens to be.
        let dump_command = format!(
            "install -m 600 /dev/null {path} && sudo mysqldump --single-transaction --routines --triggers --no-tablespaces --no-create-db {db} > {path}",
            path = shell_quote(&dump_path),
            db = shell_quote(&planned.name),
        );
        let dumped = source
            .execute_command(&dump_command)
            .await
            .map_err(|err| PlanNote::with("databaseDumpFailed", &[("database", &planned.name), ("error", &err.to_string())]))?;
        if dumped.exit_code != 0 {
            // Both routes reported, because "access denied" from one and
            // "no such database" from the other are different problems and
            // guessing which mattered is the operator's time.
            let detail = format!("{credentialed_error}; sudo mysqldump: {}", dumped.stderr.trim());
            let _ = source.execute_command(&format!("rm -f {}", shell_quote(&dump_path))).await;
            return Err(PlanNote::with("databaseDumpFailed", &[("database", &planned.name), ("error", &detail)]));
        }
    }

    // The destination is created through the ordinary provisioning path, so
    // the new database, its user and its generated password are recorded and
    // shown exactly as a hand-made one would be.
    let created = match database_service::create_application_database(
        db_repo,
        app_repo,
        server_repo,
        sessions,
        application_id,
        target_database_host_id,
        Some(&planned.name),
    )
    .await
    {
        Ok(created) => created,
        Err(err) => {
            let _ = source.execute_command(&format!("rm -f {}", shell_quote(&dump_path))).await;
            return Err(PlanNote::with("databaseCreateFailed", &[("database", &planned.name), ("error", &err.to_string())]));
        }
    };

    // The restore reads the dump from the database machine own filesystem, so
    // when that is a different machine the file has to travel. It goes
    // through this desktop, like the file copy - the only route that does not
    // assume the two machines can reach each other.
    let restore_result = match move_dump_if_needed(server_repo, sessions, &source, &dump_path, db_repo, target_database_host_id).await {
        Ok(()) => database_service::restore_dump_into_database(db_repo, server_repo, sessions, target_database_host_id, &created.database_name, &dump_path)
            .await
            .map_err(|err| PlanNote::with("databaseRestoreFailed", &[("database", &planned.name), ("error", &err.to_string())])),
        Err(note) => Err(note),
    };

    // A dump is a full copy of the data and must not be left lying in a home
    // directory on either machine, whichever way this went.
    let _ = source.execute_command(&format!("rm -f {}", shell_quote(&dump_path))).await;
    if let Ok(target) = database_host_connection(db_repo, server_repo, sessions, target_database_host_id).await {
        let _ = target.execute_command(&format!("rm -f {}", shell_quote(&dump_path))).await;
    }

    restore_result.map(|()| created.database_name)
}

/// Opens a connection to whichever Node a VibeSSH database host lives on.
async fn database_host_connection(
    db_repo: &DatabaseRepository,
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    database_host_id: Uuid,
) -> AppResult<std::sync::Arc<crate::ssh::SshSession>> {
    let host = database_service::list_database_hosts(db_repo)?
        .into_iter()
        .find(|host| host.id == database_host_id)
        .ok_or_else(|| AppError::NotFound(format!("database host {database_host_id}")))?;
    let server_id = host
        .server_id
        .ok_or_else(|| AppError::InvalidInput("that database host is not on a Node VibeSSH manages, so a dump cannot be placed on it".to_string()))?;
    get_or_connect(server_repo, sessions, server_id).await
}

/// Puts the dump where the restore will read it from, if that is not already
/// where it is.
async fn move_dump_if_needed(
    server_repo: &ServerRepository,
    sessions: &SshSessionManager,
    source: &crate::ssh::SshSession,
    dump_path: &str,
    db_repo: &DatabaseRepository,
    target_database_host_id: Uuid,
) -> Result<(), PlanNote> {
    let target = database_host_connection(db_repo, server_repo, sessions, target_database_host_id)
        .await
        .map_err(|err| PlanNote::with("databaseRestoreFailed", &[("database", dump_path), ("error", &err.to_string())]))?;
    if target.id() == source.id() {
        return Ok(());
    }
    let bytes = source
        .read_file(dump_path)
        .await
        .map_err(|err| PlanNote::with("databaseRestoreFailed", &[("database", dump_path), ("error", &err.to_string())]))?;
    crate::ssh::write_private_file(&target, dump_path, &bytes)
        .await
        .map_err(|err| PlanNote::with("databaseRestoreFailed", &[("database", dump_path), ("error", &err.to_string())]))?;
    Ok(())
}

/// A working directory for the imported Application, in the same shape the
/// Create Application wizard suggests.
fn working_directory_for(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let slug = slug.split('-').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("-");
    format!("/home/container/{}", if slug.is_empty() { "imported" } else { &slug })
}

/// Imports one server from an agreed plan.
///
/// Errors from a single server are returned inside the outcome rather than as
/// an `Err`, because the caller is working through a whole panel: one server
/// whose node is unreachable must not abandon the twenty that came after it.
#[allow(clippy::too_many_arguments)]
pub async fn import_server(
    app_repo: &ApplicationRepository,
    registry: &BlueprintRegistry,
    server_repo: &ServerRepository,
    network_repo: &NodeNetworkRepository,
    firewall_rule_repo: &FirewallRuleRepository,
    db_repo: &DatabaseRepository,
    sessions: &SshSessionManager,
    java_root: &std::path::Path,
    planned: &PlannedServer,
    target_server_id: Uuid,
    target_database_host_id: Option<Uuid>,
    on_step: &(dyn Fn(ImportStep) + Send + Sync),
) -> ImportOutcome {
    let mut outcome = ImportOutcome {
        source_id: planned.source_id,
        name: planned.name.clone(),
        application_id: None,
        files_copied: 0,
        databases_moved: Vec::new(),
        warnings: Vec::new(),
        failed: None,
    };

    let Some(source_server_id) = planned.source.matched_server_id else {
        outcome.failed = Some(PlanNote::with("nodeUnknown", &[("fqdn", &planned.source.fqdn)]));
        return outcome;
    };

    on_step(ImportStep::Stopping);
    if let Err(err) = stop_on_the_panel(server_repo, sessions, source_server_id, &planned.source_uuid).await {
        // Nothing has been created yet, so this is a clean failure: the panel
        // is exactly as it was.
        outcome.failed = Some(PlanNote::with("stopFailed", &[("error", &err.to_string())]));
        return outcome;
    }

    on_step(ImportStep::Creating);
    let working_directory = working_directory_for(&planned.name);
    let create_input = CreateApplicationFromBlueprintInput {
        server_id: Some(target_server_id),
        name: planned.name.clone(),
        description: if planned.egg.is_empty() { None } else { Some(format!("Imported from Pterodactyl ({})", planned.egg)) },
        blueprint_id: planned.blueprint_id.clone(),
        runtime_type: RuntimeType::Docker,
        working_directory: working_directory.clone(),
        environment: planned
            .environment
            .iter()
            // Never marked secret: these came out of the panel's own API in
            // plain text, so calling them secrets here would claim a
            // protection that was never there. The operator can mark any of
            // them on the Environment tab afterwards.
            .map(|variable| EnvironmentVariable { key: variable.key.clone(), value: variable.value.clone(), is_secret: false })
            .collect(),
        blueprint_inputs: serde_json::Value::Object(planned.fields.iter().map(|(key, value)| (key.clone(), value.clone())).collect()),
        // A migrated server arrives with whatever the panel had it pointed
        // at already in its own environment - there is nothing here to pick
        // a target from, and inventing one would rewrite a working setup.
        connect_to_application_id: None,
    };

    let created = match application_service::create_application(app_repo, registry, server_repo, sessions, java_root, create_input).await {
        Ok(created) => created,
        Err(err) => {
            outcome.failed = Some(PlanNote::with("createFailed", &[("error", &err.to_string())]));
            return outcome;
        }
    };
    let application_id = created.application.id;
    outcome.application_id = Some(application_id);

    on_step(ImportStep::Configuring);
    // Creating from a blueprint already made that blueprint's own well-known
    // port - Paper publishes 25565. Right for a new server, wrong for an
    // imported one: it publishes a port this server never had, and the port
    // players actually use would end up alongside it. So the existing entry
    // is retargeted at the panel's primary port instead of competing with it.
    let existing_default = created.ports.first().map(|port| port.id);
    let mut retargeted = false;

    for port in &planned.ports {
        // Pterodactyl maps a host port straight onto the same port inside the
        // container, and writes that number into the server's own
        // configuration - so the copied `server.properties` says 3002, and
        // the two have to stay equal for the server to answer at all.
        let input = PortInput {
            name: if port.primary { "primary".to_string() } else { format!("port-{}", port.port) },
            protocol: PortProtocol::Tcp,
            bind_address: String::new(),
            internal_port: port.port,
            external_port: Some(port.port),
            visibility: PortVisibility::Public,
            required: port.primary,
        };

        let result = match existing_default {
            Some(port_id) if port.primary && !retargeted => {
                retargeted = true;
                application_service::update_application_port(app_repo, server_repo, network_repo, firewall_rule_repo, sessions, application_id, port_id, &input)
                    .await
                    .map(|_| ())
            }
            _ => application_service::add_application_port(app_repo, server_repo, network_repo, firewall_rule_repo, sessions, application_id, &input)
                .await
                .map(|_| ()),
        };

        if let Err(err) = result {
            // A port already taken on the target is a real, common case when
            // the panel and VibeSSH share a machine. It is a warning, not a
            // failure: everything else about this server still migrated, and
            // the operator fixes one number on the Ports tab.
            //
            // The owner is spelled out rather than left to `Display`, which
            // says only "already in use" - the collision check has just
            // worked out *what* holds it, and that is the whole difference
            // between a warning somebody can act on and one they cannot.
            let detail = match &err {
                AppError::PortInUse { owner: Some(owner), .. } => format!("{err} ({owner})"),
                other => other.to_string(),
            };
            outcome.warnings.push(PlanNote::with("portNotTaken", &[("port", &port.port.to_string()), ("error", &detail)]));
        }
    }

    // A blueprint port left pointing at its own default, because the panel
    // told us no primary allocation, would be a published port nobody chose.
    if existing_default.is_some() && !retargeted && !planned.ports.is_empty() {
        outcome.warnings.push(PlanNote::new("defaultPortKept"));
    }

    if planned.memory_mb.is_some() || planned.cpu_cores.is_some() {
        let limits = SetResourceLimitsInput {
            memory_limit_mb: planned.memory_mb.and_then(|mb| u32::try_from(mb).ok()),
            cpu_limit_cores: planned.cpu_cores.map(|cores| cores as f32),
        };
        if let Err(err) = application_service::set_application_resource_limits(app_repo, application_id, limits) {
            outcome.warnings.push(PlanNote::with("limitsNotSet", &[("error", &err.to_string())]));
        }
    }

    on_step(ImportStep::CopyingFiles);
    match copy_volume(server_repo, sessions, source_server_id, &planned.source.volume_path, target_server_id, &working_directory).await {
        Ok(copied) => outcome.files_copied = copied,
        Err(err) => {
            // The Application exists and is configured, but it has no data.
            // Reported as a failure on this server rather than a warning,
            // because starting it in that state would generate a brand new
            // world over the top of a migration that did not happen.
            outcome.failed = Some(PlanNote::with("copyFailed", &[("error", &err.to_string())]));
            outcome.warnings.push(PlanNote::new("createdButEmpty"));
            return outcome;
        }
    }

    if !planned.databases.is_empty() {
        on_step(ImportStep::MovingDatabases);
        match target_database_host_id {
            Some(host_id) => {
                for database in &planned.databases {
                    match move_one_database(db_repo, app_repo, server_repo, sessions, database, application_id, host_id).await {
                        Ok(new_name) => outcome.databases_moved.push(new_name),
                        // A database that did not move is a warning, not a
                        // failure of the whole server: the Application, its
                        // configuration and its files are all in place, and
                        // the operator can move one schema by hand knowing
                        // exactly which one and why.
                        Err(note) => outcome.warnings.push(note),
                    }
                }
            }
            // Asked for without somewhere to put them. Said plainly rather
            // than skipped, because "the databases are missing" discovered
            // later is the worst version of this.
            None => outcome.warnings.push(PlanNote::with("noDatabaseHostChosen", &[("count", &planned.databases.len().to_string())])),
        }
    }

    on_step(ImportStep::Done);
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(exit_code: i32, stdout: &str, stderr: &str) -> crate::transport::CommandOutput {
        crate::transport::CommandOutput { exit_code, stdout: stdout.to_string(), stderr: stderr.to_string() }
    }

    /// The silent empty import: `cp` could not read the volume, `find` never
    /// ran, and the empty output was read as "0 files copied" - a success.
    #[test]
    fn a_copy_that_failed_is_an_error_not_zero_files() {
        let failed = copied_file_count(&output(1, "", "cp: cannot open '/var/lib/pterodactyl/volumes/x/world/level.dat' for reading: Permission denied"));
        match failed {
            Err(AppError::Connection(message)) => assert!(message.contains("Permission denied"), "{message}"),
            other => panic!("expected an error, got {other:?}"),
        }
        assert!(copied_file_count(&output(0, "", "")).is_err(), "no count at all is not zero");
        assert_eq!(copied_file_count(&output(0, "1532\n", "")).unwrap(), 1532);
    }

    /// Wings' volumes are its own account's; the copy has to read them as root.
    #[test]
    fn the_same_machine_copy_reads_the_volume_as_root() {
        let command = same_machine_copy_command("/var/lib/pterodactyl/volumes/abc", "/home/container/lobby");
        assert!(command.starts_with("sudo cp -a '/var/lib/pterodactyl/volumes/abc'/. '/home/container/lobby'/"), "{command}");
        assert!(command.contains("&& sudo find '/home/container/lobby' -type f"), "{command}");
    }

    #[test]
    fn a_container_name_has_to_be_a_uuid_before_it_reaches_a_command() {
        assert!(container_name("1a7ce997-259b-452e-8b4e-cecc464142ca").is_ok());
        // The shapes that matter: anything that could carry a shell
        // metacharacter into the command built around it.
        assert!(container_name("; rm -rf /").is_err());
        assert!(container_name("$(whoami)").is_err());
        assert!(container_name("").is_err());
    }

    #[test]
    fn a_working_directory_is_derived_from_the_name_without_surprises() {
        assert_eq!(working_directory_for("Survival"), "/home/container/survival");
        assert_eq!(working_directory_for("My Server 2!"), "/home/container/my-server-2");
        assert_eq!(working_directory_for("  "), "/home/container/imported");
        // Non-ASCII names still have to produce a path a shell and Docker
        // will accept, rather than an empty one.
        assert_eq!(working_directory_for("Serwer Główny"), "/home/container/serwer-g-wny");
    }
}
