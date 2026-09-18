//! `DockerRuntime` - Applications running as a Docker container over SSH.
//! Extends `ssh::docker` (list/start/stop/restart/remove/logs reused
//! verbatim via `SshSession`'s own methods, `validate_container_ref`'s
//! injection-safe validation reused as-is) with `docker create`, which that
//! module didn't need before Applications existed. SSH-only, matching the
//! same confirmed decision `runtime::systemd` documents
//! (docs/architecture/APPLICATIONS_ARCHITECTURE.md Section 5.3/11) - Agent-managed
//! Docker stays a later, explicit decision made with a real feature in
//! hand, not granted on spec.
//!
//! **Port publishing** (`-p bind:external:internal/proto`, in
//! `build_create_command`) only happens for a declared `ApplicationPort`
//! whose `external_port` is actually set - one without it is documentation
//! of an internal-only port (e.g. a database another container reaches over
//! the Docker network), not something meant to be reachable from outside
//! the host, so it gets no `-p` flag at all.
//!
//! **Etap M1: `working_directory` is bind-mounted in** (`-v host:container`
//! plus `-w container`), so a jar/config path that's already just a bare
//! filename relative to `working_directory` - the same assumption
//! `runtime::local_process`/`remote_process`/`systemd` already make by
//! launching with that as their own cwd - resolves identically inside the
//! container. On a Node the two sides are the same string, because
//! `/srv/my-app` is a path both sides can have; for a local Application on
//! Windows they cannot be, and `container_working_directory` says what the
//! container side is instead.
//! This is *why* `start()`'s recreate-avoidance below is safe now instead
//! of destroying state on every recreate: the container's writable layer is
//! no longer the only place a world save/database file/config lives, the
//! bind-mounted host directory is. **`--user`** (`DockerConfig::run_as_dedicated_user`)
//! runs the container as its own dedicated, per-Application Linux account
//! (`crate::dedicated_user`) instead of the image's default (usually root)
//! whenever a blueprint opts in (currently every Java-family one, via
//! `blueprints::render_java_docker_config`) - so a file the process creates
//! through the bind mount comes out owned by an identity that belongs to
//! *this* Application alone, not root, and not the connecting SSH admin
//! either (an Application's dedicated account is a genuinely narrower
//! identity than that - see `crate::dedicated_user`'s own doc comment for
//! why that distinction matters). Deliberately not forced for
//! `GenericDockerBlueprint`'s own arbitrary, user-picked image - see that
//! field's own doc comment. Application Files for such an Application
//! also switches providers accordingly - see `files::sudo_user`.
//!
//! **Private per-Application networks** (`app_network_name`/`network_alias`):
//! every container gets its own bridge network and, by default, shares it
//! with nothing - one Application cannot open a socket to another at all.
//! An operator grants reachability explicitly (`ApplicationDetail::links`),
//! and a grant is a private two-member network shared by exactly those two
//! containers, on which each resolves the other by its `network_alias`. See
//! `APP_NETWORK_PREFIX`'s own doc comment for why default-deny, and for why
//! a grant is its own network rather than the client joining the target's.
//! `reconcile_networks` applies the allow-list on every start and on every
//! grant/revoke, so a change lands on a running container without a restart.
//!
//! **Interactive console** (`console()`/`DockerConsole`): every container
//! created here gets `-i` and, on each `start`/`restart`, a background
//! `docker attach` piped from a host-side named pipe (`attach_console_fifo`)
//! - the same FIFO-backed design `runtime::remote_process` uses for a bare
//! process, just pointed at `docker attach` instead of the process itself.
//! Best-effort: a container that started before this existed, or whose
//! attach step failed for some reason, just falls back to a read-only
//! console until its next recreate.
//!
//! **Unlike `runtime::systemd`/`runtime::remote_process`, `start()` does
//! NOT unconditionally recreate.** Those runtimes persist only *config*
//! (a unit file, a shell command) that's always safe to regenerate. Now that
//! a Docker container's state lives in the bind mount rather than its
//! writable layer, recreating the *container* on every start would still be
//! wasteful (and briefly drop it, mid-health-check, for no reason) even
//! though it's no longer destructive - `start()` still only runs `docker
//! create` when no container with this application's name exists yet.
//! Picking up an edited image/command/restart policy needs an explicit
//! recreate - `destroy()` below, plus `start()` - exposed as a "Recreate
//! Container" action
//! (`services::application_service::recreate_application`), not implicit on
//! every start.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::dedicated_user;
use crate::errors::{AppError, AppResult};
use crate::models::{Application, ApplicationStatus, EnvironmentVariable, PortProtocol};
use crate::ssh::docker::validate_container_ref;
use super::docker_command::{DockerCommandRunner, LocalDocker, PrivateFile};
use crate::ssh::SshSession;
// The one shared implementation - every module that builds a remote
// command used to carry its own byte-identical copy of this.
use crate::ssh::command::{quote as shell_quote, reject_newlines};

use super::{
    health_check, validate_resource_limits, ApplicationConsole, ApplicationRuntime, HealthCheckSpec, HealthStatus, LogProvider,
    ResourceUsage, RuntimeContext,
};

/// What `runtime_config` deserializes into for `RuntimeType::Docker`.
/// `command`, if given, overrides the image's own `ENTRYPOINT`/`CMD` - a
/// container created without one just runs the image as authored.
///
/// `memory_limit_mb`/`cpu_limit_cores` are set through
/// `services::set_application_resource_limits`, not the Create Application
/// wizard - see that function's own doc comment. They're only baked in at
/// `docker create` time (see `create_container`), so, same as an edited
/// `image`/`command`, a change here doesn't reach an already-existing
/// container until it's removed and recreated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerConfig {
    pub image: String,
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub memory_limit_mb: Option<u32>,
    #[serde(default)]
    pub cpu_limit_cores: Option<f32>,
    /// `--restart <policy>` - defaults to `unless-stopped` when absent (see
    /// `restart_policy_or_default`), including for every Docker Application
    /// created before this field existed, via `#[serde(default)]`.
    #[serde(default)]
    pub restart_policy: Option<String>,
    /// Set only by `blueprints::render_java_docker_config` (Paper/Velocity/
    /// Generic Java) - runs the container as its own dedicated,
    /// unprivileged Linux account (`crate::dedicated_user`, one per
    /// Application) instead of the image's default (root, for
    /// eclipse-temurin), so a file the process creates through the
    /// bind-mounted `working_directory` (a generated `server.properties`,
    /// `velocity.toml`, a plugin's own config) is owned by an identity that
    /// belongs to *this* Application alone - not root, and not the shared
    /// connecting SSH admin either, which would otherwise let a bug in this
    /// codebase (or a compromised Application) reach every other
    /// Application's files too. Only the boolean itself is decided here, at
    /// render time - the account's actual `uid:gid` can't be, since
    /// resolving it means actually creating the account first
    /// (`dedicated_user::ensure_provisioned`), which needs a live
    /// connection `create_container` has and rendering doesn't. Deliberately
    /// `false`/absent for `GenericDockerBlueprint`'s own arbitrary,
    /// user-picked image: some official images (a database doing root-owned
    /// setup in their entrypoint before dropping to their own service user,
    /// say) genuinely need to start as root, so forcing this for an image
    /// this codebase knows nothing about would be a regression, not a fix.
    #[serde(default)]
    pub run_as_dedicated_user: bool,
}

/// `docker create --restart` only accepts a fixed set of values - validated
/// here (not left for the daemon to reject) so a typo surfaces as a clear
/// `AppError::InvalidInput` before ever reaching `execute_command`, matching
/// how `validate_resource_limits` already validates the other two config
/// values before they're used to build a command.
const VALID_RESTART_POLICIES: &[&str] = &["no", "always", "unless-stopped", "on-failure"];

fn restart_policy_or_default(config: &DockerConfig) -> AppResult<&str> {
    let policy = config.restart_policy.as_deref().unwrap_or("unless-stopped");
    if !VALID_RESTART_POLICIES.contains(&policy) {
        return Err(AppError::InvalidInput(format!(
            "'{policy}' isn't a valid restart policy - expected one of {VALID_RESTART_POLICIES:?}"
        )));
    }
    Ok(policy)
}

fn parse_config(ctx: &RuntimeContext<'_>) -> AppResult<DockerConfig> {
    serde_json::from_value(ctx.runtime_config.clone())
        .map_err(|err| AppError::InvalidInput(format!("invalid Docker configuration: {err}")))
}

/// Where the bind-mounted working directory lands *inside* the container.
///
/// Host path and container path were the same string until this existed, and
/// on a Node they can be: `/srv/my-app` is a path a Linux host and a Linux
/// container can both have. For an Application running on this machine they
/// cannot. A local working directory is a Windows path - the wizard prefills
/// one from `app_data_dir()` - and a Linux container has no
/// `C:\Users\...\applications\paper`, so `-v C:\...:C:\...` and `-w C:\...`
/// were rejected by the daemon with a complaint about a path the user was
/// given nowhere to correct. Nowhere existed: nothing in VibeSSH asks for a
/// container-side path, because until now there was no such thing.
///
/// **The test is the path's own shape, not this machine's operating system.**
/// A VibeSSH running on Windows and managing a remote Linux Node must keep
/// passing that Node's paths through untouched, and does, because they start
/// with `/`. Only a path that could not be a path inside a Linux container
/// gets replaced.
///
/// `/home/container` rather than `/app`: it is what a remote Application's
/// working directory already looks like here (`working_directory_for` builds
/// `/home/container/<slug>`), so the directory a user sees inside the
/// container is the one they would expect from every other VibeSSH
/// Application - and it is far less likely to be sitting on top of something
/// an arbitrary image already ships.
const CONTAINER_WORKING_DIRECTORY: &str = "/home/container";

fn container_working_directory(working_directory: &str) -> &str {
    if working_directory.starts_with('/') {
        working_directory
    } else {
        CONTAINER_WORKING_DIRECTORY
    }
}

/// `vibessh-app-<uuid>` - unambiguously VibeSSH-owned, matching
/// `runtime::systemd`'s unit-naming reasoning, so this runtime never
/// touches a container it didn't create.
fn container_name(application_id: Uuid) -> String {
    format!("vibessh-app-{application_id}")
}

/// Where this Application's `docker` runs.
///
/// A missing connection is not an error here any more: `server_id` is
/// `None` exactly when the Application is local, so no connection *is* the
/// signal that the daemon on this machine is the one meant.
fn expect_success(output: crate::transport::CommandOutput, what: &str) -> AppResult<()> {
    if output.exit_code == 0 {
        return Ok(());
    }
    let detail = output.stderr.trim();
    let detail = if detail.is_empty() { output.stdout.trim().to_string() } else { detail.to_string() };
    let detail = if detail.is_empty() { format!("docker exited with {}", output.exit_code) } else { detail };
    // A daemon that is not running is the single most common way this fails,
    // and the CLI's own words for it are about a named pipe - see
    // `daemon_unreachable`.
    if let Some(message) = super::docker_command::daemon_unreachable(&detail) {
        return Err(AppError::Connection(message));
    }
    Err(AppError::Connection(format!("couldn't {what}: {detail}")))
}

fn runner<'a>(ctx: &'a RuntimeContext<'_>) -> &'a dyn DockerCommandRunner {
    match ctx.connection.as_deref() {
        Some(session) => session,
        None => &LocalDocker,
    }
}

/// The same choice, owned - for the console and the log reader, which outlive
/// the call that made them.
fn runner_arc(ctx: &RuntimeContext<'_>) -> Arc<dyn DockerCommandRunner> {
    match ctx.connection.clone() {
        Some(session) => session,
        None => Arc::new(LocalDocker),
    }
}

/// Whether this Application's per-Application Linux account can be honoured
/// here at all.
///
/// **One function because the answer used to differ between three places
/// that had to agree.** `create_container` learned that a local daemon has
/// no POSIX accounts and stopped refusing; `start` and `restart` kept the
/// older stance and went on demanding a connection. Since
/// `render_java_docker_config` sets `run_as_dedicated_user` unconditionally
/// for every Java blueprint, that made a local Minecraft Application
/// creatable and then impossible to start, with "DockerRuntime requires a
/// connection" - a message about SSH shown to somebody who never asked for
/// a remote server.
///
/// Nothing is lost locally: the isolation exists so a *remote* Node keeps an
/// Application's files out of root's ownership, and on this machine the
/// files are already the user's own.
fn dedicated_user_applies(config: &DockerConfig, runner: &dyn DockerCommandRunner) -> bool {
    config.run_as_dedicated_user && runner.supports_dedicated_user()
}

/// The connection to set that account up on, or `None` where there is no
/// account to set up.
///
/// **Returns the connection rather than a boolean so the mistake cannot be
/// made again.** The bug was not that the rule was wrong; it was that two
/// callers checked a weaker condition and then reached for
/// `connection_ref(ctx)?`, which fails locally. A caller handed an
/// `Option<&SshSession>` has nothing left to reach for: either there is a
/// session and the work is due, or there is not and there is nothing to do.
fn dedicated_user_connection<'a>(config: &DockerConfig, runner: &dyn DockerCommandRunner, ctx: &'a RuntimeContext<'_>) -> Option<&'a SshSession> {
    if !dedicated_user_applies(config, runner) {
        return None;
    }
    // `supports_dedicated_user()` is only true for the SSH runner, so this
    // is belt and braces rather than a case anybody has seen.
    ctx.connection.as_deref()
}

// `connection_ref` stood here and raised "DockerRuntime requires a
// connection". It has no callers left: every remaining use of an
// `SshSession` in this runtime comes from `dedicated_user_connection` or a
// plain `ctx.connection.as_deref()`, both of which treat its absence as
// "this is the local daemon" rather than as a fault. The error is gone
// rather than merely unused, so it cannot come back by someone reaching for
// the convenient helper.

/// Same `.vibessh-app-<uuid>.stdin` naming/location `runtime::remote_process`
/// uses for its own FIFO - not shared code (this module has no dependency on
/// that one), just the same convention, safe to reuse verbatim since an
/// Application only ever has one active `RuntimeType` at a time.
fn fifo_file_name(application_id: Uuid) -> String {
    format!(".vibessh-app-{application_id}.stdin")
}

/// Where an Application's console fifo actually lives - see
/// `build_attach_script` for why this is not inside `working_directory`.
fn console_fifo_path(application_id: Uuid) -> String {
    format!("{}/{application_id}.stdin", crate::node_paths::CONSOLE_DIR)
}

fn remote_path(working_directory: &str, file_name: &str) -> String {
    format!("{}/{}", working_directory.trim_end_matches('/'), file_name)
}



/// POSIX environment variable name rule - also guards against a key
/// containing `=`, which `docker create -e` would otherwise misparse.
fn is_valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn validate_environment(environment: &[EnvironmentVariable]) -> AppResult<()> {
    for env in environment {
        if !is_valid_env_key(&env.key) {
            return Err(AppError::InvalidInput(format!("'{}' isn't a valid environment variable name", env.key)));
        }
        reject_newlines(&env.value, &format!("the '{}' environment variable", env.key))?;
    }
    Ok(())
}

/// Every VibeSSH-created container gets its own private bridge network,
/// named from its Application's id, and by default shares it with nothing.
///
/// This replaced a single shared `vibessh-net` that every container joined
/// with a resolvable alias. That made service discovery free - a Velocity
/// proxy found its Paper backend by name, stable across either side being
/// recreated - and it also meant any Application could open a socket to any
/// other Application's *unpublished* ports on the same Node, including the
/// ones left unpublished precisely because they were never meant to be
/// reachable (`AUDIT_REPORT.md` S-018). Of the four isolation guarantees the
/// architecture claims, that was the one not actually enforced: a hostile
/// image, or one compromised Application, was a single `connect()` away
/// from every other Application's database and admin port on the box.
///
/// Reachability is now default-deny and an operator grants it explicitly
/// (`application_links`, migration 16). A grant is implemented as a private
/// two-member network shared by exactly those containers
/// (`link_network_name`), **not** by putting the client onto the target's
/// own network. The distinction is the whole point: under the simpler
/// scheme, two Applications each granted access to a shared third one would
/// land on that third one's network together and silently become reachable
/// to each other, which is not what either grant said.
///
/// Cost of the design, stated plainly: one Docker network per Application
/// plus one per connection, against a default daemon address pool of
/// roughly thirty. `ensure_network` turns exhaustion into an error that
/// names the cause rather than a raw Docker string.
const APP_NETWORK_PREFIX: &str = "vibessh-net-";

/// A granted connection's own network. Exactly two containers ever join
/// one, and it is created `--internal`: a link network exists to carry
/// traffic between two Applications, never to reach the outside, and each
/// container still has egress through its own `APP_NETWORK_PREFIX` network.
const LINK_NETWORK_PREFIX: &str = "vibessh-link-";

/// Everything this module considers its own. `reconcile_networks` only ever
/// *disconnects* a container from a network whose name starts with this: a
/// network the operator attached by hand is theirs, and tearing it off on
/// the next start would be its own kind of surprise.
const MANAGED_NETWORK_PREFIX: &str = "vibessh-";

/// The pre-S-018 shared network. Nothing joins it any more, and
/// `reconcile_networks` disconnects any container still on it - which is
/// what actually migrates an existing Node, at each Application's next
/// start. `MANAGED_NETWORK_PREFIX` already covers the name; this constant
/// exists so that behaviour is greppable and can be asserted by name.
///
/// **This is a breaking change for an existing Node** and deliberately so:
/// cross-Application connectivity that worked yesterday because everything
/// shared one network stops working, and has to be granted. There is no
/// safe way to infer which of those connections were load-bearing - only
/// the operator knows - and guessing would have meant re-creating the
/// exposure under a new name.
const LEGACY_SHARED_NETWORK: &str = "vibessh-net";

/// First 12 hex digits of the Application's id.
///
/// A network name has to fit alongside a second one in
/// `link_network_name`, and two full UUIDs make an 85-character name that
/// is unreadable in `docker network ls` for no gain. Twelve hex digits is
/// 48 bits: two Applications on one Node colliding is not a practical
/// concern, and a collision would be an availability bug (two Applications
/// sharing a network name) rather than a silent grant.
fn short_id(id: Uuid) -> String {
    // `Uuid::simple` is always 32 hex digits, so this slice cannot panic.
    id.simple().to_string()[..12].to_string()
}

fn app_network_name(application_id: Uuid) -> String {
    format!("{APP_NETWORK_PREFIX}{}", short_id(application_id))
}

/// Order-normalised, exactly as `application_links` normalises the stored
/// pair - both ends of a connection have to derive the same name from their
/// own point of view, or each would create its own network and neither
/// would ever reach the other.
fn link_network_name(a: Uuid, b: Uuid) -> String {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    format!("{LINK_NETWORK_PREFIX}{}-{}", short_id(lo), short_id(hi))
}

/// The networks this container should be on: its own, plus one per granted
/// connection. Pure, so the allow-list translation is unit-testable without
/// a Node - the same split `build_create_command` already uses.
fn desired_networks(ctx: &RuntimeContext<'_>) -> Vec<String> {
    let id = ctx.application.id;
    let mut networks = vec![app_network_name(id)];
    for peer in ctx.links {
        // A self-link is rejected at the storage layer; skipping it here too
        // keeps this function total rather than relying on that.
        if *peer == id {
            continue;
        }
        let name = link_network_name(id, *peer);
        if !networks.contains(&name) {
            networks.push(name);
        }
    }
    networks
}

/// Slugified `Application::name` (lowercase, `[a-z0-9-]`, collapsed
/// repeats, never empty) - the same treatment `services::dns_service::slugify`
/// gives a Node's own name for the exact same reason (a value safe to use
/// as a hostname/network alias), duplicated rather than shared across the
/// `runtime`/`services` module boundary the same way several other small
/// helpers already are in this codebase. Not guaranteed globally unique
/// (two Applications named e.g. "Test" and "test!!" both slugify to
/// `"test"`) - Docker's own behavior for a duplicate alias (round-robin
/// across every container that registered it) is an availability quirk in
/// that rare case, not a security issue, and every container's own
/// `--name` (`container_name`, always unique) still resolves unambiguously
/// regardless.
/// Capped at 63 characters (RFC 1123's own limit for a single DNS label) -
/// otherwise a long Application name produces a `--network-alias` Docker's
/// embedded DNS itself would reject, same fix `services::dns_service::slugify`
/// needed for the identical reason. Every char actually pushed here is
/// single-byte ASCII, so `result.len()` is a safe stand-in for a char count.
pub(crate) fn network_alias(application: &Application) -> String {
    // Falls back to the container name rather than a literal, so two
    // Applications whose names both slugify to nothing still get distinct,
    // resolvable aliases instead of colliding on the same one.
    crate::naming::dns_label(&application.name).unwrap_or_else(|| container_name(application.id))
}

/// Runs one ad-hoc command against the server inside a container and hands
/// back what it said.
///
/// **The user's command is a parameter, never part of the script.** It goes
/// in as `sh -c <script> vibessh <command>`, so the blueprint's own snippet
/// reads it as `"$1"`. There is no arrangement of quotes, semicolons or
/// backticks in it that can end that script and start another one - the
/// shell never parses it as code, it only ever assigns it.
///
/// That is also why the blueprint declares a snippet rather than a template
/// with a hole in it: a hole is something a caller can escape from, and a
/// positional parameter is not.
///
/// **Both streams come back.** A client that failed says so on stderr, and
/// dropping it would leave the console showing nothing at all for the most
/// interesting case. The exit code is not an error here either: `redis-cli`
/// reporting a wrong command is an answer to show, not a failure of the
/// call.
pub(crate) async fn run_command_in_container(ctx: &RuntimeContext<'_>, shell: &str, command: &str) -> AppResult<String> {
    let runner = runner(ctx);
    let name = container_name(ctx.application.id);
    validate_container_ref(&name)?;

    let args = exec_command_args(&name, shell, command);
    let output = runner.docker(&args.iter().map(String::as_str).collect::<Vec<_>>()).await?;

    let mut answer = String::new();
    answer.push_str(output.stdout.trim_end());
    let stderr = output.stderr.trim_end();
    if !stderr.is_empty() {
        if !answer.is_empty() {
            answer.push('\n');
        }
        answer.push_str(stderr);
    }
    Ok(answer)
}

/// The argument list for one console command, built where it can be read.
///
/// `sh -c <script> vibessh <command>` - the `vibessh` is `$0`, a name for the
/// shell rather than anything the script uses, which leaves the user's
/// command as `"$1"`. Pure and separate so a test can assert the one property
/// that matters: the command is its own argument and is never part of the
/// script text.
fn exec_command_args(container: &str, shell: &str, command: &str) -> Vec<String> {
    vec![
        "exec".to_string(),
        container.to_string(),
        "sh".to_string(),
        "-c".to_string(),
        shell.to_string(),
        "vibessh".to_string(),
        command.to_string(),
    ]
}

/// Idempotent - a plain `docker network create` errors on a network that
/// already exists, so this probes first via `inspect`, the same "cheap check
/// before touching the mutating command" shape `dedicated_user::ensure_provisioned`
/// already uses for its own group/user creation. The probe is also what
/// makes a lost race harmless: two Applications starting at once can both
/// miss the same link network, and the loser re-probes instead of failing.
///
/// `internal` creates the network with no route out (`--internal`), which is
/// right for a link network and wrong for an Application's own - see
/// `LINK_NETWORK_PREFIX`.
async fn ensure_network(runner: &dyn DockerCommandRunner, name: &str, internal: bool) -> AppResult<()> {
    // The redirections these commands used to carry were only ever silencing
    // output nobody read - the exit code is the answer. Dropping them is what
    // lets the same call run without a shell.
    let probe = runner.docker(&["network", "inspect", name]).await?;
    if probe.exit_code == 0 {
        return Ok(());
    }
    let mut create = vec!["network", "create"];
    if internal {
        create.push("--internal");
    }
    create.push(name);
    let output = runner.docker(&create).await?;
    if output.exit_code != 0 {
        // Lost race: somebody else created it between the probe and here.
        let recheck = runner.docker(&["network", "inspect", name]).await?;
        if recheck.exit_code == 0 {
            return Ok(());
        }
        let detail = output.stderr.trim();
        // The one failure worth naming. Per-Application networks make it
        // reachable in a way one shared network never was, and Docker's own
        // wording gives an operator nothing to act on.
        if detail.contains("non-overlapping IPv4 address pool") {
            return Err(AppError::Connection(format!(
                "Docker has no address space left for another private network on this node. Each application gets its own network, \
                 and each connection between two applications gets one more, against a default pool of about thirty. Remove unused \
                 networks with 'docker network prune', or widen 'default-address-pools' in /etc/docker/daemon.json. Docker said: {detail}"
            )));
        }
        let detail = if detail.is_empty() { "docker network create failed".to_string() } else { detail.to_string() };
        return Err(AppError::Connection(format!("couldn't create the '{name}' network: {detail}")));
    }
    Ok(())
}

/// A Go template rather than JSON: the whole answer is a list of names, and
/// `docker inspect -f` is already how this module reads a single field.
/// Kept as a plain constant rather than a `format!` argument, so the doubled
/// braces a Go template needs are not also doubled for Rust.
const INSPECT_NETWORKS_FORMAT: &str = "{{range $name, $config := .NetworkSettings.Networks}}{{$name}} {{end}}";

/// The networks this container is on right now, according to the Node -
/// never inferred from what VibeSSH last did. A container created before
/// per-Application networks existed is still sitting on the old shared
/// network, and asking is the only way to find that out.
async fn current_networks(runner: &dyn DockerCommandRunner, container: &str) -> AppResult<Vec<String>> {
    validate_container_ref(container)?;
    let output = runner.docker(&["inspect", "-f", INSPECT_NETWORKS_FORMAT, container]).await?;
    if output.exit_code != 0 {
        let detail = output.stderr.trim();
        let detail = if detail.is_empty() { "docker inspect failed".to_string() } else { detail.to_string() };
        return Err(AppError::Connection(format!("couldn't read the container's networks: {detail}")));
    }
    Ok(output.stdout.split_whitespace().map(str::to_string).collect())
}

/// Brings a container's actual network membership in line with the granted
/// allow-list: connect what is missing, then disconnect what is no longer
/// granted.
///
/// Connect first, disconnect second, so a container that is only on the
/// legacy shared network is never momentarily left with no network at all.
///
/// A failed *disconnect* fails the whole call, and callers propagate it -
/// `start` included. Leaving a container attached to a network it is no
/// longer allowed on is an open exposure, and a start that reports success
/// while quietly keeping it open is exactly the failure shape this audit
/// kept finding (`AGENTS.md` rule 3).
async fn reconcile_networks(runner: &dyn DockerCommandRunner, ctx: &RuntimeContext<'_>, container: &str) -> AppResult<()> {
    let own = app_network_name(ctx.application.id);
    let desired = desired_networks(ctx);
    let alias = network_alias(ctx.application);
    let current = current_networks(runner, container).await?;

    for name in &desired {
        if current.iter().any(|existing| existing == name) {
            continue;
        }
        ensure_network(runner, name, name != &own).await?;
        // The alias is the one genuinely useful thing the old shared network
        // did, kept - but now only between two Applications someone
        // connected on purpose.
        let output = runner.docker(&["network", "connect", "--alias", &alias, name, container]).await?;
        if output.exit_code != 0 {
            let detail = output.stderr.trim();
            let detail = if detail.is_empty() { "docker network connect failed".to_string() } else { detail.to_string() };
            return Err(AppError::Connection(format!("couldn't join the '{name}' network: {detail}")));
        }
    }

    for name in &current {
        if !name.starts_with(MANAGED_NETWORK_PREFIX) || desired.iter().any(|wanted| wanted == name) {
            continue;
        }
        if name == LEGACY_SHARED_NETWORK {
            // Worth a log line rather than a silent disconnect: this is the
            // one-off migration off the pre-S-018 shared network, and it is
            // also the moment cross-Application connectivity an operator was
            // relying on stops working. If they come asking why, this is the
            // line that answers it.
            log::info!(
                "taking '{container}' off the legacy shared '{LEGACY_SHARED_NETWORK}' network -                  reachability between applications is now granted explicitly"
            );
        }
        let output = runner.docker(&["network", "disconnect", name, container]).await?;
        if output.exit_code != 0 {
            let detail = output.stderr.trim();
            let detail = if detail.is_empty() { "docker network disconnect failed".to_string() } else { detail.to_string() };
            return Err(AppError::Connection(format!(
                "couldn't disconnect this application from the '{name}' network, so it can still reach whatever else is on it: {detail}"
            )));
        }
    }
    Ok(())
}

/// Tears down the networks an Application owned, once its container is gone.
///
/// Best-effort, returning what it could not do rather than failing: this
/// runs inside `delete_application`'s teardown, where every other step
/// reports into the same warning list, and a leftover empty bridge network
/// is untidy rather than dangerous - it has no members left to expose
/// anything to. `former_peers` comes from the link rows read *before* the
/// cascade deleted them.
pub async fn remove_networks(runner: &dyn DockerCommandRunner, application_id: Uuid, former_peers: &[Uuid]) -> Vec<String> {
    let mut warnings = Vec::new();
    let mut names = vec![app_network_name(application_id)];
    for peer in former_peers {
        names.push(link_network_name(application_id, *peer));
    }
    for name in names {
        // A non-zero exit is the normal case here - the network may never
        // have been created, or the peer may not have disconnected yet - so
        // only a transport failure is worth reporting.
        if let Err(err) = runner.docker(&["network", "rm", &name]).await {
            warnings.push(format!("couldn't remove the '{name}' network: {err}"));
        }
    }
    warnings
}

async fn container_exists(runner: &dyn DockerCommandRunner, name: &str) -> AppResult<bool> {
    validate_container_ref(name)?;
    let output = runner.docker(&["inspect", name]).await?;
    Ok(output.exit_code == 0)
}

/// Pure argument construction, separated from `create_container`'s actual
/// execution so the flag placement can be unit tested without a live
/// connection - same split `runtime::systemd::render_unit_file` already uses
/// for the same reason.
///
/// Arguments rather than a command line: the local runner hands these to the
/// process directly, and quoting for a shell that is not there is how a
/// working directory with a space in it becomes two arguments.
fn build_create_args(ctx: &RuntimeContext<'_>, config: &DockerConfig, name: &str, user_flag: Option<&str>, env_file: Option<&str>) -> AppResult<Vec<String>> {
    validate_container_ref(name)?;
    reject_newlines(&config.image, "the image")?;
    for arg in &config.command {
        reject_newlines(arg, "a command argument")?;
    }
    validate_environment(ctx.environment)?;
    validate_resource_limits(config.memory_limit_mb, config.cpu_limit_cores)?;
    let restart_policy = restart_policy_or_default(config)?;
    reject_newlines(&ctx.application.working_directory, "the working directory")?;
    for port in ctx.ports {
        reject_newlines(&port.bind_address, "a port's bind address")?;
    }

    let working_directory = ctx.application.working_directory.clone();
    let container_directory = container_working_directory(&working_directory).to_string();
    // `-i` keeps STDIN open even with nothing attached yet - harmless if the
    // Console tab is never used, and what makes `attach_console_fifo`'s
    // later `docker attach` able to feed the container's stdin at all (an
    // application never created with this can't become interactive after
    // the fact without a recreate, same "baked in at create time" rule
    // `--user`/`--memory`/etc already follow here).
    //
    // `--add-host host.docker.internal:host-gateway` is what Docker Desktop
    // gives a container for free but plain Linux dockerd (what every real
    // Node here runs) doesn't - without it, a container that needs to reach
    // something on its own host (a self-hosted MySQL/MariaDB a phpMyAdmin
    // Application connects to, say) has no portable name for "the host" to
    // use, only the bridge network's own gateway IP, which isn't fixed or
    // guessable. Harmless when nothing inside the container ever resolves
    // that name.
    //
    // `--network` puts the container on its *own* private network and
    // nothing else - see `APP_NETWORK_PREFIX`'s doc comment for why the
    // shared one this used to name was removed. Any connection to another
    // Application is a separate network joined afterwards by
    // `reconcile_networks`, because `docker create` takes only one
    // `--network` and because a connection can be granted or revoked long
    // after the container was created.
    //
    // The alias travels with it onto every network it joins, so the far end
    // of a granted connection addresses this Application by name rather than
    // by an IP that does not survive a recreate.
    let mut args: Vec<String> = vec![
        "create".into(),
        "-i".into(),
        "--name".into(),
        name.into(),
        "--restart".into(),
        restart_policy.into(),
        "--add-host".into(),
        "host.docker.internal:host-gateway".into(),
        "--network".into(),
        app_network_name(ctx.application.id),
        "--network-alias".into(),
        network_alias(ctx.application),
        "-v".into(),
        format!("{working_directory}:{container_directory}"),
        "-w".into(),
        container_directory.clone(),
    ];
    // Only ever set for `run_as_dedicated_user` (see that field's own doc
    // comment) - runs the process as this Application's own dedicated
    // Linux account, so a file it creates there is immediately editable by
    // that same account (via `files::sudo_user`), instead of coming out
    // owned by root (the image's default) and reachable by no one but a
    // manual `sudo` session.
    if let Some(user) = user_flag {
        args.push("--user".into());
        args.push(user.into());
    }
    if let Some(mb) = config.memory_limit_mb {
        args.push("--memory".into());
        args.push(format!("{mb}m"));
    }
    if let Some(cores) = config.cpu_limit_cores {
        args.push("--cpus".into());
        args.push(cores.to_string());
    }
    for port in ctx.ports {
        // `external_port` unset means "declared but not meant to be
        // reachable from outside the container's own network" (e.g. a
        // database another container reaches internally) - see
        // `ApplicationPort::external_port`'s own doc comment. Only publish
        // the ones that actually asked for it.
        let Some(external_port) = port.external_port else { continue };
        let proto = match port.protocol {
            PortProtocol::Tcp => "tcp",
            PortProtocol::Udp => "udp",
        };
        args.push("-p".into());
        args.push(format!("{}:{external_port}:{}/{proto}", port.bind_address, port.internal_port));
    }
    // `--env-file`, never `-e KEY=VALUE`.
    //
    // A value passed as an argument ends up in the container process's argv,
    // and on Linux `/proc/<pid>/cmdline` is world-readable: any local account
    // could have read a database password out of `ps` while the container
    // was being created, and a process-accounting or audit daemon would have
    // written it to disk. The file this points at is mode 0600 in a 0700
    // directory and is deleted straight after - see `write_private_file`.
    if let Some(path) = env_file {
        args.push("--env-file".into());
        args.push(path.to_string());
    }
    args.push(config.image.clone());
    for arg in &config.command {
        args.push(arg.clone());
    }
    Ok(args)
}

async fn create_container(runner: &dyn DockerCommandRunner, ctx: &RuntimeContext<'_>, config: &DockerConfig, name: &str) -> AppResult<()> {
    // Not internal: an Application's own network is also its way out.
    ensure_network(runner, &app_network_name(ctx.application.id), false).await?;

    // `run_as_dedicated_user` does not survive the trip to a local daemon,
    // and that is not a downgrade being waved through.
    //
    // An earlier version of this refused outright, on the reasoning that an
    // Application which asked for that isolation must not quietly run
    // without it. The premise was wrong: nothing asks for it.
    // `render_java_docker_config` sets the flag unconditionally for every
    // Java blueprint, as the mechanism by which a *remote* Node keeps an
    // Application's files out of root's ownership - `files::sudo_user` reads
    // them back through that same account.
    //
    // Locally there is no such account and no need for one: the files are
    // the user's own, and `files::provider_for` picks the local provider
    // from the Application's location rather than from this flag. So the
    // isolation is not lost here; it is a remote-Node mechanism that has
    // nothing to do on this machine. Refusing only made every Minecraft
    // blueprint impossible to run locally.
    let dedicated = dedicated_user_connection(config, runner, ctx);
    if config.run_as_dedicated_user && dedicated.is_none() {
        log::info!("{}: running without a dedicated user - the local daemon has no such accounts", ctx.application.name);
    }
    let user_flag = if let Some(connection) = dedicated {
        let username = dedicated_user::username(ctx.application.id);
        dedicated_user::ensure_provisioned(connection, &username).await?;
        Some(dedicated_user::user_id(connection, &username).await?)
    } else {
        None
    };
    // Written before the arguments are built, because the path is one of
    // them - and removed whatever the outcome, including the failure paths,
    // which is why the result is held rather than returned with `?`.
    let env_file = if ctx.environment.is_empty() { None } else { Some(runner.write_private_file(&env_file_contents(ctx.environment)).await?) };

    let args = build_create_args(ctx, config, name, user_flag.as_deref(), env_file.as_ref().map(|file| file.path.as_str()))?;
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    let outcome = runner.docker(&borrowed).await;

    if let Some(file) = env_file {
        // Best effort: the container has already been created either way,
        // and failing the operation because a temporary file survived would
        // trade a real success for a cosmetic failure. It is still worth
        // saying, because what is left behind holds secrets.
        if let Err(err) = remove_private_file(runner, &file).await {
            log::warn!("couldn't remove the temporary environment file {}: {err}", file.path);
        }
    }

    expect_success(outcome?, "create the container")
}

/// The `KEY=VALUE` lines `--env-file` expects.
///
/// No quoting, deliberately: Docker reads the whole of the rest of the line
/// as the value, so a quote here would end up *in* the value. Newlines are
/// the one thing that would break the format, and `validate_environment`
/// rejects them before anything gets this far.
fn env_file_contents(environment: &[EnvironmentVariable]) -> String {
    let mut out = String::new();
    for env in environment {
        out.push_str(&env.key);
        out.push('=');
        out.push_str(&env.value);
        out.push('\n');
    }
    out
}

/// Removes the directory `write_private_file` made, and the file in it.
async fn remove_private_file(runner: &dyn DockerCommandRunner, file: &PrivateFile) -> AppResult<()> {
    runner.remove_private_directory(&file.directory).await
}

/// Feeds `docker attach`'s stdin from a host-side named pipe, so
/// `DockerConsole::write` (a plain, one-off SSH exec per keystroke/command)
/// can reach the container's own stdin without holding an SSH channel open
/// for the console's whole lifetime - the exact same "background process
/// holds the pipe open, a later fresh command just writes into it" trick
/// `runtime::remote_process::build_start_script` already uses for a bare
/// `nohup`'d process, here pointed at `docker attach` instead of the
/// application's own binary. `exec 3<>{fifo}` (read-write, not a plain
/// blocking `<{fifo}`) is what lets the not-yet-connected FIFO be handed to
/// the backgrounded `docker attach` as its stdin without a chicken-and-egg
/// deadlock - see that same line in `build_start_script`'s own doc comment.
/// A fresh attach is needed after *every* `start`/`restart`: `docker
/// attach`'s stream is tied to one running instance of the container's
/// entrypoint process, so it exits the moment that instance stops, even
/// though the container (and this same FIFO) survives to be reused next
/// start. Best-effort by design (the caller ignores this call's own
/// error) - a container that starts fine but can't get an attacher still
/// runs; it just falls back to a read-only console, same as
/// `RemoteProcessRuntime`'s own "no true interactive TTY" limitation for a
/// blueprint image this attach step doesn't support for some reason.
/// Pure script construction, same "separate from the actual SSH exec so it
/// can be unit tested without a live connection" split
/// `build_create_command`/`create_container` already establish in this
/// module.
///
/// **The FIFO deliberately does not live in `working_directory`.** It used
/// to, created under `sudo` and then `chmod 666`'d so the connecting
/// admin's own `exec 3<>` could open a root-created fifo inside a directory
/// owned by the Application's dedicated account. Mode 666 on a path inside
/// the bind-mounted application directory means *every* local account on
/// the Node - including every other Application's dedicated account, and
/// the Application's own process from inside its container - could write to
/// it, and this fifo **is** the container's stdin. For a game server that
/// is arbitrary console commands (`op`, `stop`); for any other image it is
/// arbitrary input to PID 1. That defeats the isolation the dedicated-account
/// model exists to provide.
///
/// Moving it to `node_paths::CONSOLE_DIR` - a 0700 directory owned by the
/// connecting admin - removes the need for both the `sudo` and the
/// permissive mode: the admin creates and opens a fifo it already owns, in
/// a directory nothing else can enter, and `docker attach` (which does run
/// under `sudo`) reads it through an already-open file descriptor rather
/// than by path.
///
/// Self-heals a fifo path that isn't actually a fifo: `mkfifo` refuses to
/// create one where *anything* else already exists, silently - normally a
/// harmless no-op since a real fifo from a previous start is exactly what's
/// expected to be there, but a regular file left at that exact path would
/// permanently wedge every future console attach with no error to explain
/// why. `test -p` checks the existing path is genuinely a fifo before
/// trusting it, and clears anything else out of the way first.
///
/// Also removes the old world-writable fifo from `working_directory` if a
/// previous version of VibeSSH left one there - otherwise upgrading would
/// fix new Applications while quietly leaving the vulnerable fifo in place
/// for every Application that had already been started once.
fn build_attach_script(ctx: &RuntimeContext<'_>, name: &str) -> AppResult<String> {
    validate_container_ref(name)?;
    let fifo = shell_quote(&console_fifo_path(ctx.application.id));
    let legacy_fifo = shell_quote(&remote_path(&ctx.application.working_directory, &fifo_file_name(ctx.application.id)));
    let ensure_dirs = crate::node_paths::ensure_runtime_dirs_command();
    Ok(format!(
        "{ensure_dirs}\n\
         sudo rm -f {legacy_fifo}\n\
         if [ -e {fifo} ] && [ ! -p {fifo} ]; then rm -f {fifo}; fi\n\
         [ -p {fifo} ] || mkfifo -m 600 {fifo}\n\
         chmod 600 {fifo}\n\
         exec 3<>{fifo}\n\
         nohup sudo docker attach --sig-proxy=false {name} <&3 3<&- >/dev/null 2>&1 &\n\
         disown\n"
    ))
}

async fn attach_console_fifo(connection: &SshSession, ctx: &RuntimeContext<'_>, name: &str) -> AppResult<()> {
    let script = build_attach_script(ctx, name)?;
    let output = connection.execute_command(&script).await?;
    if output.exit_code != 0 {
        let detail = output.stderr.trim();
        let detail = if detail.is_empty() { "couldn't attach the console".to_string() } else { detail.to_string() };
        return Err(AppError::Connection(detail));
    }
    Ok(())
}

/// `--user` (see `DockerConfig::run_as_dedicated_user`'s own doc comment)
/// only decides who *new* files the container writes from now on belong to
/// - it does nothing for whatever's already sitting in the bind mount (a
/// config/jar/world save written by a prior, root-default image, or by this
/// same blueprint before it started opting into `run_as_dedicated_user`, or
/// before this Application's dedicated account even existed). Called on
/// every `start`/`restart`, not just a fresh `docker create`, so a file the
/// dedicated account still can't write to becomes writable the next time
/// the user hits Start/Restart - not only after they specifically think to
/// hit Recreate. Cheap and idempotent (a no-op chown on files that already
/// have the right owner) - best-effort, same as `attach_console_fifo`: a
/// failure here doesn't block start/restart, it just leaves whichever
/// specific files it couldn't reach still unwritable.
async fn ensure_working_directory_owned_by_dedicated_user(connection: &SshSession, ctx: &RuntimeContext<'_>) {
    let username = dedicated_user::username(ctx.application.id);
    if dedicated_user::ensure_provisioned(connection, &username).await.is_err() {
        return;
    }
    if let Err(err) = connection
        .execute_command(&format!("sudo chown -R {} {}", shell_quote(&username), shell_quote(&ctx.application.working_directory)))
        .await
    {
        // Not fatal to the start - the container runs either way - but the
        // Files tab will then fail on every write with a permission error
        // that names the file rather than the cause.
        log::warn!("couldn't hand '{}' to its dedicated account: {err}", ctx.application.working_directory);
    }
}

/// `.State.Status` alone can't distinguish a clean stop from a crash - both
/// are `"exited"`. Combined with `.State.ExitCode` (fetched in the same
/// `docker inspect` call), this is a genuinely more reliable Failed signal
/// than `runtime::remote_process` can offer for a plain `nohup`'d process -
/// a real capability difference between runtimes, not something to flatten
/// away (brief's own "nie udawaj identycznych capabilities").
fn map_container_status(status: &str, exit_code: &str) -> ApplicationStatus {
    match status {
        "running" => ApplicationStatus::Running,
        "restarting" => ApplicationStatus::Starting,
        "removing" | "paused" => ApplicationStatus::Stopping,
        "dead" => ApplicationStatus::Failed,
        "exited" if exit_code != "0" => ApplicationStatus::Failed,
        // "exited" with code 0, "created" (made but never started), or
        // anything unrecognized (including the container not existing yet)
        // - Stopped is the honest default.
        _ => ApplicationStatus::Stopped,
    }
}

/// Parses `docker inspect --format '{{.State.Status}}|{{.State.ExitCode}}'`.
fn parse_status_output(output: &str) -> (String, String) {
    let mut fields = output.trim().splitn(2, '|');
    let status = fields.next().unwrap_or("").to_string();
    let exit_code = fields.next().unwrap_or("").trim().to_string();
    (status, exit_code)
}

/// Parses `docker stats --no-stream --format '{{.CPUPerc}}|{{.MemUsage}}'` -
/// e.g. `"1.23%|45.6MiB / 512MiB"`.
fn parse_stats_output(output: &str) -> (Option<f32>, Option<u64>) {
    let mut fields = output.trim().splitn(2, '|');
    let cpu_percent = fields.next().and_then(|f| f.trim().trim_end_matches('%').parse::<f32>().ok());
    let ram_bytes = fields.next().and_then(|mem| mem.split('/').next()).and_then(|used| parse_docker_byte_size(used.trim()));
    (cpu_percent, ram_bytes)
}

/// Docker's `go-units.BytesSize` formatting - a number immediately followed
/// by a unit suffix, no space between them (`"45.6MiB"`). IEC units
/// (`KiB`/`MiB`/`GiB`/`TiB`) are what modern Docker actually emits; decimal
/// units (`kB`/`MB`/`GB`) are accepted too since older/differently
/// configured builds have used them.
fn parse_docker_byte_size(value: &str) -> Option<u64> {
    let split_at = value.find(|c: char| !c.is_ascii_digit() && c != '.')?;
    let (number, unit) = value.split_at(split_at);
    let number: f64 = number.parse().ok()?;
    let multiplier = match unit {
        "B" => 1.0,
        "KiB" => 1024.0,
        "MiB" => 1024.0 * 1024.0,
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        "TiB" => 1024.0_f64.powi(4),
        "kB" => 1000.0,
        "MB" => 1000.0 * 1000.0,
        "GB" => 1000.0 * 1000.0 * 1000.0,
        _ => return None,
    };
    Some((number * multiplier) as u64)
}

/// `.State.StartedAt` is always RFC3339 - unlike `runtime::systemd`'s
/// `ActiveEnterTimestamp` (skipped there for being locale/timezone-
/// dependent), this is a standard, unambiguous format `chrono` parses
/// directly.
fn parse_started_at(value: &str) -> Option<u64> {
    let started_at = DateTime::parse_from_rfc3339(value.trim()).ok()?;
    let elapsed = Utc::now().signed_duration_since(started_at.with_timezone(&Utc));
    u64::try_from(elapsed.num_seconds()).ok()
}

pub struct DockerRuntime;

impl DockerRuntime {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DockerRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ApplicationRuntime for DockerRuntime {
    async fn validate(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        let runner = runner(ctx);
        let config = parse_config(ctx)?;
        if config.image.trim().is_empty() {
            return Err(AppError::InvalidInput("no image configured for this application".into()));
        }
        reject_newlines(&config.image, "the image")?;
        for arg in &config.command {
            reject_newlines(arg, "a command argument")?;
        }
        validate_environment(ctx.environment)?;

        let output = runner.docker(&["version", "--format", "{{.Server.Version}}"]).await?;
        if output.exit_code != 0 {
            // Its own code, not a generic invalid-input: the UI can offer
            // to install Docker, which is a real next step rather than a
            // sentence.
            return Err(AppError::DockerUnavailable);
        }
        Ok(())
    }

    async fn start(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        let runner = runner(ctx);
        let config = parse_config(ctx)?;
        let name = container_name(ctx.application.id);

        if !container_exists(runner, &name).await? {
            create_container(runner, ctx, &config, &name).await?;
        }
        // Before the container runs, not after: this is where a container
        // created under the old shared network gets taken off it, and where
        // a connection revoked while this Application was stopped actually
        // stops applying. Propagates on failure - see `reconcile_networks`.
        reconcile_networks(runner, ctx, &name).await?;
        // Only where it can be honoured - see `dedicated_user_connection`.
        // This used to be `if config.run_as_dedicated_user` alone followed by
        // `connection_ref(ctx)?`, which made a local Java Application
        // startable only in theory: created fine, and then every start
        // failed with a message about SSH.
        if let Some(connection) = dedicated_user_connection(&config, runner, ctx) {
            ensure_working_directory_owned_by_dedicated_user(connection, ctx).await;
            // Best-effort, same reasoning as everything else on this path -
            // Application Files just falls back to failing clearly on its
            // own next call if this doesn't land, it doesn't block start.
            let _ = crate::files::sudo_user::ensure_helper_installed(connection).await;
        }
        expect_success(runner.docker(&["start", &name]).await?, "start the container")?;
        // Best-effort, per `attach_console_fifo`'s own doc comment - a
        // console attach failure must never fail the start itself. Attempted
        // only over SSH: the FIFO it builds is a POSIX object with no local
        // Windows counterpart, and the local console is wired up separately.
        if let Some(connection) = ctx.connection.as_deref() {
            let _ = attach_console_fifo(connection, ctx, &name).await;
        }
        Ok(())
    }

    /// `docker stop` is already a graceful stop by construction (SIGTERM,
    /// then SIGKILL after its own timeout) - same reasoning
    /// `runtime::systemd::stop` documents for why `graceful` doesn't map to
    /// anything further here; the real graceful/immediate split is
    /// `stop()` vs `kill()`.
    async fn stop(&self, ctx: &RuntimeContext<'_>, _graceful: bool) -> AppResult<()> {
        let runner = runner(ctx);
        let name = container_name(ctx.application.id);
        if !container_exists(runner, &name).await? {
            return Err(AppError::InvalidInput("this application isn't running".into()));
        }
        expect_success(runner.docker(&["stop", &name]).await?, "stop the container")
    }

    /// Applies a granted or revoked connection to a container that already
    /// exists, without restarting it - `docker network connect`/`disconnect`
    /// both work on a running container, which is what makes revoking take
    /// effect at the moment the operator asks rather than at some later
    /// restart they might never perform.
    ///
    /// Nothing to do for an Application that has never been started: there
    /// is no container to attach, and `start` reconciles before it runs one.
    async fn sync_connections(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        let runner = runner(ctx);
        let name = container_name(ctx.application.id);
        if !container_exists(runner, &name).await? {
            return Ok(());
        }
        reconcile_networks(runner, ctx, &name).await
    }

    /// Restarts the existing container in place - does not recreate it
    /// (see the module doc comment). Falls back to `start()` if nothing has
    /// been created yet, same as the other two SSH runtimes.
    async fn restart(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        // Was the last method here still written against an `SshSession`
        // rather than the runner, so restarting a local container failed
        // before it did anything at all. `restart_container` was only ever
        // `docker restart <name>`, which the runner does on either target.
        let runner = runner(ctx);
        let config = parse_config(ctx)?;
        let name = container_name(ctx.application.id);
        if !container_exists(runner, &name).await? {
            return self.start(ctx).await;
        }
        reconcile_networks(runner, ctx, &name).await?;
        if let Some(connection) = dedicated_user_connection(&config, runner, ctx) {
            ensure_working_directory_owned_by_dedicated_user(connection, ctx).await;
            let _ = crate::files::sudo_user::ensure_helper_installed(connection).await;
        }
        expect_success(runner.docker(&["restart", &name]).await?, "restart the container")?;
        // Best-effort, per `attach_console_fifo`'s own doc comment - the
        // previous attach process died along with the pre-restart instance,
        // a fresh one is needed for the new one. Over SSH only: the FIFO is
        // a POSIX object, and the local console attaches separately.
        if let Some(connection) = ctx.connection.as_deref() {
            let _ = attach_console_fifo(connection, ctx, &name).await;
        }
        Ok(())
    }

    async fn kill(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        let runner = runner(ctx);
        let name = container_name(ctx.application.id);
        if !container_exists(runner, &name).await? {
            return Err(AppError::InvalidInput("this application isn't running".into()));
        }
        expect_success(runner.docker(&["kill", &name]).await?, "kill the container")
    }

    async fn status(&self, ctx: &RuntimeContext<'_>) -> AppResult<ApplicationStatus> {
        let runner = runner(ctx);
        let name = container_name(ctx.application.id);
        validate_container_ref(&name)?;
        let output = runner.docker(&["inspect", "--format", "{{.State.Status}}|{{.State.ExitCode}}", &name]).await?;
        if output.exit_code != 0 {
            // Never created, or removed - Stopped, not an error: the same
            // "no other place it could be running" reasoning the other two
            // SSH runtimes use.
            return Ok(ApplicationStatus::Stopped);
        }
        let (status, exit_code) = parse_status_output(&output.stdout);
        Ok(map_container_status(&status, &exit_code))
    }

    async fn resource_usage(&self, ctx: &RuntimeContext<'_>) -> AppResult<ResourceUsage> {
        let empty = ResourceUsage { cpu_percent: None, ram_bytes: None, uptime_seconds: None };
        let runner = runner(ctx);
        let name = container_name(ctx.application.id);
        validate_container_ref(&name)?;

        // One SSH round trip, not two. This is polled every few seconds per
        // Application, and each `execute_command` opens its own SSH channel -
        // so splitting `stats` and `inspect` across two calls doubled the
        // channel churn for a reading that is always wanted together.
        //
        // `stats` is the slow half (Docker samples the container for a
        // moment), so it runs first and its failure short-circuits: a
        // container that is not running has no stats and no meaningful
        // uptime either.
        // Two readings, and how they are fetched depends on where docker is.
        // Over SSH they are chained into one command because each
        // `execute_command` opens its own channel, and this is polled every
        // few seconds per Application - the round trip is the cost worth
        // avoiding. Locally there is no round trip, so two plain invocations
        // are simpler and need no shell.
        let (stats_line, started_line) = match ctx.connection.as_deref() {
            Some(connection) => {
                let output = connection
                    .execute_command(&format!(
                        "sudo docker stats --no-stream --format '{{{{.CPUPerc}}}}|{{{{.MemUsage}}}}' {name} 2>/dev/null &&                  sudo docker inspect --format '{{{{.State.StartedAt}}}}' {name} 2>/dev/null"
                    ))
                    .await?;
                if output.exit_code != 0 {
                    return Ok(empty);
                }
                let mut lines = output.stdout.lines();
                (lines.next().unwrap_or("").to_string(), lines.next().unwrap_or("").to_string())
            }
            None => {
                let stats = runner.docker(&["stats", "--no-stream", "--format", "{{.CPUPerc}}|{{.MemUsage}}", &name]).await?;
                if stats.exit_code != 0 {
                    return Ok(empty);
                }
                let started = runner.docker(&["inspect", "--format", "{{.State.StartedAt}}", &name]).await?;
                (stats.stdout.trim().to_string(), started.stdout.trim().to_string())
            }
        };
        let (cpu_percent, ram_bytes) = parse_stats_output(&stats_line);
        let uptime_seconds = parse_started_at(&started_line);

        Ok(ResourceUsage { cpu_percent, ram_bytes, uptime_seconds })
    }

    async fn health_check(&self, ctx: &RuntimeContext<'_>, spec: &HealthCheckSpec) -> AppResult<HealthStatus> {
        let status = self.status(ctx).await?;
        health_check::default_health_check(ctx, spec, status, Some("the container exited with a non-zero status")).await
    }

    /// `None` for a container that isn't running (never started, still
    /// created-but-stopped, or created before this runtime started passing
    /// `-i`/attaching a console FIFO - the trait's own documented example of
    /// a legitimate `None`) - the UI shows a clear read-only state rather
    /// than accepting input `attach_console_fifo` was never actually able to
    /// wire up.
    async fn console(&self, ctx: &RuntimeContext<'_>) -> AppResult<Option<Box<dyn ApplicationConsole>>> {
        if self.status(ctx).await? != ApplicationStatus::Running {
            return Ok(None);
        }
        match ctx.connection.clone() {
            Some(connection) => {
                let fifo_path = console_fifo_path(ctx.application.id);
                let attach_script = build_attach_script(ctx, &container_name(ctx.application.id))?;
                Ok(Some(Box::new(DockerConsole { connection, fifo_path, attach_script })))
            }
            None => {
                let name = container_name(ctx.application.id);
                validate_container_ref(&name)?;
                let stdin = super::local_docker_console::attach(ctx.application.id, &name).await?;
                Ok(Some(Box::new(LocalDockerConsole { application_id: ctx.application.id, stdin })))
            }
        }
    }

    async fn logs(&self, ctx: &RuntimeContext<'_>) -> AppResult<Box<dyn LogProvider>> {
        Ok(Box::new(DockerLogs { runner: runner_arc(ctx), name: container_name(ctx.application.id) }))
    }

    /// `docker rm -f` - stops (if running) and removes in one step, same as
    /// the CLI's own `-f` semantics. A no-op, not an error, when nothing was
    /// ever created (e.g. "Recreate Container" clicked before the first
    /// successful start) - `destroy()` guarantees "no container with this
    /// name exists after this returns Ok", not "a container existed before
    /// this ran".
    async fn destroy(&self, ctx: &RuntimeContext<'_>) -> AppResult<()> {
        // Whatever was holding this container's stdin open locally has
        // nothing left to hold it open for.
        super::local_docker_console::detach(ctx.application.id).await;
        let runner = runner(ctx);
        let name = container_name(ctx.application.id);
        validate_container_ref(&name)?;
        if !container_exists(runner, &name).await? {
            return Ok(());
        }
        expect_success(runner.docker(&["rm", "-f", &name]).await?, "remove the container")
    }
}

/// Types into a container on this machine.
///
/// The remote console below has to send each line as its own SSH exec into a
/// FIFO, because an exec channel cannot be held open for the tab's whole
/// life. Nothing here is under that constraint: the `docker attach` keeping
/// this container's stdin open is a child of this process, so a line is just
/// a write.
struct LocalDockerConsole {
    application_id: Uuid,
    stdin: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
}

#[async_trait::async_trait]
impl ApplicationConsole for LocalDockerConsole {
    async fn write(&self, input: &str) -> AppResult<()> {
        reject_newlines(input, "console input")?;
        self.stdin
            .send(format!("{input}
").into_bytes())
            .map_err(|_| AppError::Connection("the container is no longer accepting input - it may have stopped".into()))
    }

    fn close(&self) {
        // The attach outlives any one open tab - closing it here would take
        // the console away from anybody else looking at the same
        // Application, and reopening the tab would start another. It is
        // dropped when the container stops, which is when it stops meaning
        // anything.
        let application_id = self.application_id;
        let _ = application_id;
    }

    fn supports_input(&self) -> bool {
        true
    }
}

/// The shell command that puts one line into the console fifo.
///
/// Split out because it is a shell program built by interpolation, and one
/// of the things interpolated is whatever somebody typed into a console box.
/// It is quoted twice on purpose - once for `printf`'s own argument, once
/// for the `sh -c` that `timeout` needs in order to bound a redirect - and
/// getting that wrong is a remote shell, not a cosmetic bug.
fn console_write_command(fifo_path: &str, input: &str) -> String {
    let write = format!("printf '%s\n' {} >> {}", shell_quote(input), shell_quote(fifo_path));
    format!("timeout {CONSOLE_WRITE_TIMEOUT_SECONDS} sh -c {}", shell_quote(&write))
}

impl DockerConsole {
    /// One bounded attempt. `Ok(false)` means the write timed out, which for
    /// a fifo means one thing only: nothing had it open for reading.
    ///
    /// A redirect rather than the `tee` this used to pipe into - one process
    /// under `timeout` rather than a pipeline whose second half outlives the
    /// signal sent to the first, holding the fifo open on its way out.
    /// No `sudo` either way: the fifo lives in a directory owned by the
    /// connecting admin (see `build_attach_script`).
    async fn write_once(&self, input: &str) -> AppResult<bool> {
        let output = self.connection.execute_command(&console_write_command(&self.fifo_path, input)).await?;
        match output.exit_code {
            0 => Ok(true),
            TIMED_OUT => Ok(false),
            _ => {
                let detail = output.stderr.trim();
                let detail = if detail.is_empty() { "couldn't write to the application's console" } else { detail };
                Err(AppError::Connection(detail.to_string()))
            }
        }
    }
}

/// Writes into `attach_console_fifo`'s named pipe - a plain one-off SSH exec
/// per message, same shape as `RemoteConsole::write`, since the actual
/// long-lived connection to the container's stdin is the background
/// `docker attach` process that fifo feeds, not this struct.
struct DockerConsole {
    connection: Arc<SshSession>,
    fifo_path: String,
    /// Re-runs `attach_console_fifo`'s work when the pipe turns out to have
    /// no reader. Built here rather than looked up later because it needs the
    /// `RuntimeContext`, which only `console()` has.
    attach_script: String,
}

/// How long a console write waits for the container to be listening.
///
/// Opening a fifo for writing blocks until something opens it for reading -
/// that is what a fifo is. So a `docker attach` that has died takes the write
/// with it, forever, and the command sits in the box looking like it was
/// never sent. Long enough that a busy Node is not mistaken for a dead
/// attach; short enough that nobody wonders whether the button works.
const CONSOLE_WRITE_TIMEOUT_SECONDS: u8 = 5;

/// `timeout`'s own exit code for "the command was still running". Anything
/// else came from the command itself.
const TIMED_OUT: i32 = 124;

#[async_trait::async_trait]
impl ApplicationConsole for DockerConsole {
    async fn write(&self, input: &str) -> AppResult<()> {
        reject_newlines(input, "console input")?;
        // `sudo tee`, not a plain `>` redirect: a `run_as_dedicated_user`
        // Application's whole `working_directory` - this fifo included -
        // gets `chown -R`'d to that Application's own dedicated account
        // (`ensure_working_directory_owned_by_dedicated_user`), so the
        // connecting admin writing here directly would otherwise need the
        // fifo to be group/other-writable, which `mkfifo`'s own default
        // mode doesn't guarantee. `sudo` sidesteps the ownership question
        // entirely, same as every other cross-user write this module
        // already does.
        if self.write_once(input).await? {
            return Ok(());
        }

        // Nothing is reading the pipe, so the `docker attach` behind it is
        // gone. It is tied to one running instance of the container, and a
        // restart - Docker's own restart policy after a crash, or a start
        // from anywhere but VibeSSH - leaves the fifo with no reader and no
        // step that ever notices. Reattaching here is the difference between
        // a console that recovers and one that has to be explained.
        let output = self.connection.execute_command(&self.attach_script).await?;
        if output.exit_code != 0 {
            let detail = output.stderr.trim();
            let detail = if detail.is_empty() { "couldn't reattach the application's console" } else { detail };
            return Err(AppError::Connection(detail.to_string()));
        }

        if self.write_once(input).await? {
            return Ok(());
        }
        Err(AppError::Connection(
            "the application isn't reading its console - it was started without an interactive stdin, which only a Recreate can change".into(),
        ))
    }

    fn close(&self) {
        // The container isn't tied to a console UI's lifecycle - closing the
        // console must not stop it, same reasoning as
        // `RemoteConsole`/`LocalProcessRuntime`'s own console.
    }

    fn supports_input(&self) -> bool {
        true
    }
}

struct DockerLogs {
    runner: Arc<dyn DockerCommandRunner>,
    name: String,
}

#[async_trait::async_trait]
impl LogProvider for DockerLogs {
    async fn tail(&self, max_lines: u32) -> AppResult<Vec<String>> {
        validate_container_ref(&self.name)?;
        // Both streams, in the order docker printed them as best as two pipes
        // allow: this used to lean on `2>&1` in a shell, and a container that
        // logs to stderr - which most do - would otherwise show nothing at
        // all on the local path.
        let output = self.runner.docker(&["logs", "--tail", &max_lines.to_string(), "--timestamps", &self.name]).await?;
        if output.exit_code != 0 {
            let detail = output.stderr.trim();
            let detail = if detail.is_empty() { "docker logs failed".to_string() } else { detail.to_string() };
            return Err(AppError::Connection(format!("couldn't read the container's logs: {detail}")));
        }
        let mut lines: Vec<String> = output.stdout.lines().map(str::to_string).collect();
        lines.extend(output.stderr.lines().map(str::to_string));
        Ok(lines)
    }
}

#[cfg(test)]
mod tests {

    /// What a console write actually runs.
    ///
    /// A command typed into an application's console box sat there doing
    /// nothing: opening a fifo for writing blocks until something opens it
    /// for reading, and the `docker attach` that was reading had died with a
    /// container restart. The write had no bound on it, so it waited forever
    /// and the interface had nothing to say.
    mod console_write {
        use super::super::{console_write_command, CONSOLE_WRITE_TIMEOUT_SECONDS};

        #[test]
        fn is_bounded_so_a_dead_attach_cannot_hang_the_interface() {
            let command = console_write_command("/tmp/vibessh/console.fifo", "stop");

            assert!(command.starts_with(&format!("timeout {CONSOLE_WRITE_TIMEOUT_SECONDS} ")), "{command}");
        }

        /// The input is whatever somebody typed into a box, and it reaches a
        /// remote shell through two levels of quoting. Run for real rather
        /// than pattern-matched: the only convincing evidence is the bytes
        /// that come out the other end.
        ///
        /// Skipped where no POSIX shell is on PATH - a plain Windows box is
        /// not a defect in the command.
        #[test]
        fn a_console_line_full_of_shell_metacharacters_arrives_verbatim() {
            let target = std::env::temp_dir().join(format!("vibessh-console-{}.txt", uuid::Uuid::new_v4()));
            let nasty = r#"lp user CrispiDEV parent add 'mod'; touch /tmp/pwned $(id) `id` "x""#;
            let command = console_write_command(&target.to_string_lossy(), nasty);

            let run = std::process::Command::new("sh").arg("-c").arg(&command).output();
            let Ok(output) = run else {
                eprintln!("no POSIX shell on PATH - skipped");
                return;
            };
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));

            let written = std::fs::read_to_string(&target).unwrap();
            let _ = std::fs::remove_file(&target);
            assert_eq!(written, format!("{nasty}
"), "the line must arrive exactly as typed, and nothing else must run");
            assert!(!std::path::Path::new("/tmp/pwned").exists(), "the injected command ran");
        }

        /// Appended, not overwritten: the fifo is a stream somebody else is
        /// reading, and a second command must not begin by discarding the
        /// first.
        #[test]
        fn a_second_line_joins_the_first() {
            let target = std::env::temp_dir().join(format!("vibessh-console-{}.txt", uuid::Uuid::new_v4()));
            for line in ["first", "second"] {
                let command = console_write_command(&target.to_string_lossy(), line);
                let Ok(output) = std::process::Command::new("sh").arg("-c").arg(&command).output() else {
                    eprintln!("no POSIX shell on PATH - skipped");
                    return;
                };
                assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            }

            let written = std::fs::read_to_string(&target).unwrap();
            let _ = std::fs::remove_file(&target);
            assert_eq!(written, "first
second
");
        }
    }
    use super::*;
    use crate::models::{ApplicationPort, HealthCheckType, RuntimeType};

    fn stub_application(id: Uuid) -> Application {
        Application {
            id,
            server_id: Some(Uuid::new_v4()),
            name: "My App".to_string(),
            description: None,
            blueprint_id: "generic".to_string(),
            blueprint_version: 1,
            runtime_type: RuntimeType::Docker,
            working_directory: "/srv/my-app".to_string(),
            status: ApplicationStatus::Unknown,
            last_status_check_at: None,
            health_check_type: HealthCheckType::Process,
            health_check_port_id: None,
            health_check_http_path: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    /// The default, and the claim the rest of the isolation story rests on:
    /// an Application nobody has connected to anything is on exactly one
    /// network, its own, which no other container ever joins.
    #[test]
    fn an_application_with_no_connections_is_alone_on_its_own_network() {
        let id = Uuid::new_v4();
        let application = stub_application(id);
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], links: &[], connection: None };

        assert_eq!(desired_networks(&ctx), vec![app_network_name(id)]);
    }

    /// Both ends have to name the shared network identically or each would
    /// create its own and the connection would exist only on paper. The
    /// pair is normalised, so the two Applications derive it from opposite
    /// orderings of the same ids.
    #[test]
    fn both_ends_of_a_connection_derive_the_same_network_name() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        assert_eq!(link_network_name(a, b), link_network_name(b, a));
        assert_ne!(link_network_name(a, b), app_network_name(a));
        assert!(link_network_name(a, b).starts_with(MANAGED_NETWORK_PREFIX));
    }

    #[test]
    fn a_connection_adds_exactly_one_network_and_leaves_the_application_on_its_own() {
        let id = Uuid::new_v4();
        let peer = Uuid::new_v4();
        let application = stub_application(id);
        let runtime_config = serde_json::json!({});
        let links = [peer, peer];
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], links: &links, connection: None };

        // Duplicated peer collapses: a repeated row must not produce a
        // second `docker network connect` that then fails.
        assert_eq!(desired_networks(&ctx), vec![app_network_name(id), link_network_name(id, peer)]);
    }

    /// A container is trivially able to reach itself, and a one-member
    /// network would be a permanent no-op that `reconcile_networks` kept
    /// re-creating.
    #[test]
    fn a_self_link_never_becomes_a_network() {
        let id = Uuid::new_v4();
        let application = stub_application(id);
        let runtime_config = serde_json::json!({});
        let links = [id];
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], links: &links, connection: None };

        assert_eq!(desired_networks(&ctx), vec![app_network_name(id)]);
    }

    /// The legacy shared network is not in any desired set, and its name is
    /// covered by the prefix `reconcile_networks` uses to decide what it may
    /// disconnect - which together are what actually migrate an existing
    /// Node off it.
    #[test]
    fn the_legacy_shared_network_is_never_desired_but_is_always_managed() {
        let id = Uuid::new_v4();
        let peer = Uuid::new_v4();
        let application = stub_application(id);
        let runtime_config = serde_json::json!({});
        let links = [peer];
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], links: &links, connection: None };

        assert!(LEGACY_SHARED_NETWORK.starts_with(MANAGED_NETWORK_PREFIX));
        assert!(!desired_networks(&ctx).iter().any(|name| name == LEGACY_SHARED_NETWORK));
    }

    #[test]
    fn container_name_is_stable_and_namespaced() {
        let id = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        assert_eq!(container_name(id), "vibessh-app-11111111-2222-3333-4444-555555555555");
        assert!(validate_container_ref(&container_name(id)).is_ok());
    }

    /// The property the whole shape exists for. A command containing quotes,
    /// a semicolon and a backtick would end the script and start another one
    /// if it were spliced into the text - as its own argument it is data the
    /// script reads, and cannot be anything else.
    #[test]
    fn a_console_command_is_its_own_argument_never_part_of_the_script() {
        let shell = "exec mongosh --quiet --eval \"$1\"";
        let nasty = r#""; rm -rf / #"#;

        let args = exec_command_args("vibessh-app", shell, nasty);

        assert_eq!(args.last().map(String::as_str), Some(nasty), "the command should arrive whole: {args:?}");
        assert_eq!(args.iter().filter(|arg| arg.as_str() == shell).count(), 1, "the script should appear once, unmodified");
        // Everything before the command is fixed, so there is nowhere for it
        // to have been interpolated.
        assert_eq!(args[..6], ["exec", "vibessh-app", "sh", "-c", shell, "vibessh"].map(String::from));
    }

    /// `$0` is the shell's own name, so the command has to be `$1` - the
    /// blueprints' snippets are written against that and would read an empty
    /// string if this ever changed.
    #[test]
    fn the_command_is_the_first_positional_parameter() {
        let args = exec_command_args("vibessh-app", "echo \"$1\"", "KEYS *");

        assert_eq!(args[5], "vibessh", "$0 should be a placeholder name, not the command");
        assert_eq!(args[6], "KEYS *");
    }

    #[test]
    fn network_alias_slugifies_the_application_name() {
        let mut application = stub_application(Uuid::new_v4());
        application.name = "Paper Survival!!".to_string();
        assert_eq!(network_alias(&application), "paper-survival");
    }

    #[test]
    fn network_alias_falls_back_to_the_container_name_for_an_all_symbol_name() {
        let id = Uuid::new_v4();
        let mut application = stub_application(id);
        application.name = "!!!".to_string();
        assert_eq!(network_alias(&application), container_name(id));
    }

    /// A real bug, not a hypothetical: without this cap, an Application
    /// name longer than 63 characters produced a `--network-alias` Docker's
    /// own embedded DNS rejects outright - see `network_alias`'s own doc
    /// comment for why RFC 1123's 63-character DNS label limit is the exact
    /// bound.
    #[test]
    fn network_alias_truncates_to_the_rfc1123_dns_label_limit() {
        let mut application = stub_application(Uuid::new_v4());
        application.name = "a".repeat(80);
        let alias = network_alias(&application);
        assert_eq!(alias.len(), 63);
        assert_eq!(alias, "a".repeat(63));
    }

    /// The value that follows a flag, or `None` if the flag is absent. Says
    /// what the old string assertions were really asking - that the flag and
    /// its value are adjacent, which a substring search only implied.
    fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
        args.windows(2).find(|pair| pair[0] == flag).map(|pair| pair[1].as_str())
    }

    fn index_of(args: &[String], value: &str) -> Option<usize> {
        args.iter().position(|arg| arg == value)
    }

    #[test]
    fn build_create_command_joins_its_own_private_network_with_an_alias_before_the_image() {
        let id = Uuid::new_v4();
        let application = stub_application(id);
        let config = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: None, run_as_dedicated_user: false };
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], links: &[], connection: None };

        let args = build_create_args(&ctx, &config, "vibessh-app-test", None, None).unwrap();
        assert_eq!(flag_value(&args, "--network"), Some(app_network_name(id).as_str()), "{args:?}");
        assert_eq!(flag_value(&args, "--network-alias"), Some("my-app"), "{args:?}");
        // The whole point of S-018: nothing is created on the shared network
        // any more, so no container starts life able to reach another.
        assert!(!args.iter().any(|arg| arg == LEGACY_SHARED_NETWORK), "{args:?}");
        assert!(index_of(&args, "--network") < index_of(&args, "alpine:latest"), "{args:?}");
    }

    #[test]
    fn is_valid_env_key_rejects_equals_and_leading_digits() {
        assert!(is_valid_env_key("PORT"));
        assert!(!is_valid_env_key("PORT=8080"));
        assert!(!is_valid_env_key("8080PORT"));
    }

    #[test]
    fn map_container_status_distinguishes_a_clean_exit_from_a_crash() {
        assert_eq!(map_container_status("running", "0"), ApplicationStatus::Running);
        assert_eq!(map_container_status("exited", "0"), ApplicationStatus::Stopped);
        assert_eq!(map_container_status("exited", "1"), ApplicationStatus::Failed);
        assert_eq!(map_container_status("dead", "137"), ApplicationStatus::Failed);
        assert_eq!(map_container_status("restarting", "0"), ApplicationStatus::Starting);
        assert_eq!(map_container_status("created", ""), ApplicationStatus::Stopped);
    }

    #[test]
    fn parse_status_output_splits_on_the_pipe() {
        assert_eq!(parse_status_output("exited|137\n"), ("exited".to_string(), "137".to_string()));
        assert_eq!(parse_status_output("running|0"), ("running".to_string(), "0".to_string()));
    }

    #[test]
    fn parse_docker_byte_size_handles_iec_and_decimal_units() {
        assert_eq!(parse_docker_byte_size("45.6MiB"), Some((45.6 * 1024.0 * 1024.0) as u64));
        assert_eq!(parse_docker_byte_size("512B"), Some(512));
        assert_eq!(parse_docker_byte_size("1.5GiB"), Some((1.5 * 1024.0 * 1024.0 * 1024.0) as u64));
        assert_eq!(parse_docker_byte_size("bogus"), None);
    }

    #[test]
    fn parse_stats_output_reads_cpu_and_the_used_side_of_mem_usage() {
        let (cpu, ram) = parse_stats_output("1.23%|45.6MiB / 512MiB\n");
        assert_eq!(cpu, Some(1.23));
        assert_eq!(ram, Some((45.6 * 1024.0 * 1024.0) as u64));
    }

    #[test]
    fn parse_started_at_reads_a_real_rfc3339_timestamp() {
        let five_minutes_ago = Utc::now() - chrono::Duration::minutes(5);
        let formatted = five_minutes_ago.to_rfc3339();
        let uptime = parse_started_at(&formatted).unwrap();
        assert!((295..=310).contains(&uptime), "expected roughly 300s, got {uptime}");
        assert_eq!(parse_started_at("not-a-timestamp"), None);
    }

    #[test]
    fn reject_newlines_rejects_embedded_newlines_and_accepts_plain_text() {
        assert!(reject_newlines("alpine:latest", "the image").is_ok());
        assert!(reject_newlines("alpine\nrm -rf /", "the image").is_err());
    }

    #[test]
    fn validate_environment_rejects_a_bad_key_but_accepts_a_good_one() {
        assert!(validate_environment(&[EnvironmentVariable { key: "PORT".into(), value: "25565".into(), is_secret: false }]).is_ok());
        assert!(validate_environment(&[EnvironmentVariable { key: "NOT VALID".into(), value: "x".into(), is_secret: false }]).is_err());
    }

    #[test]
    fn build_create_command_includes_memory_and_cpu_flags_before_the_image_when_set() {
        let application = stub_application(Uuid::new_v4());
        let config = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: Some(512), cpu_limit_cores: Some(1.5), restart_policy: None, run_as_dedicated_user: false };
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], links: &[], connection: None };

        let args = build_create_args(&ctx, &config, "vibessh-app-test", None, None).unwrap();
        assert_eq!(flag_value(&args, "--memory"), Some("512m"), "{args:?}");
        assert_eq!(flag_value(&args, "--cpus"), Some("1.5"), "{args:?}");
        assert!(index_of(&args, "--memory") < index_of(&args, "alpine:latest"), "{args:?}");
    }

    #[test]
    fn build_create_command_includes_a_user_flag_before_the_image_when_given_one() {
        let application = stub_application(Uuid::new_v4());
        let config = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: None, run_as_dedicated_user: true };
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], links: &[], connection: None };

        let args = build_create_args(&ctx, &config, "vibessh-app-test", Some("1000:1000"), None).unwrap();
        assert_eq!(flag_value(&args, "--user"), Some("1000:1000"), "{args:?}");
        assert!(index_of(&args, "--user") < index_of(&args, "alpine:latest"), "{args:?}");
    }

    #[test]
    fn build_create_command_omits_the_user_flag_when_none_is_given() {
        let application = stub_application(Uuid::new_v4());
        let config = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: None, run_as_dedicated_user: false };
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], links: &[], connection: None };

        let args = build_create_args(&ctx, &config, "vibessh-app-test", None, None).unwrap();
        assert!(index_of(&args, "--user").is_none(), "{args:?}");
    }

    #[test]
    fn build_create_command_always_includes_the_interactive_flag_before_the_image() {
        let application = stub_application(Uuid::new_v4());
        let config = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: None, run_as_dedicated_user: false };
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], links: &[], connection: None };

        let args = build_create_args(&ctx, &config, "vibessh-app-test", None, None).unwrap();
        assert_eq!(args.first().map(String::as_str), Some("create"), "{args:?}");
        assert!(index_of(&args, "-i") < index_of(&args, "alpine:latest"), "{args:?}");
    }

    #[test]
    fn build_attach_script_wires_the_fifo_and_the_container_name() {
        let application = stub_application(Uuid::new_v4());
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], links: &[], connection: None };

        let script = build_attach_script(&ctx, "vibessh-app-test").unwrap();
        assert!(script.contains("[ ! -p "), "{script}");
        assert!(script.contains("mkfifo -m 600 "), "{script}");
        assert!(script.contains(&format!("'{}/", crate::node_paths::CONSOLE_DIR)), "{script}");
        assert!(script.contains(".stdin'"), "{script}");
        assert!(script.contains("docker attach --sig-proxy=false vibessh-app-test "), "{script}");
        assert!(script.contains("<&3 3<&-"), "{script}");
        // `exec 3<>` depends on the fifo already existing.
        assert!(script.find("mkfifo").unwrap() < script.find("exec 3<>").unwrap(), "{script}");
    }

    /// The regression test for the finding this move exists for: the fifo
    /// is the container's stdin, so anything that can write to it can issue
    /// console commands to somebody else's Application.
    #[test]
    fn build_attach_script_never_makes_the_console_fifo_world_writable() {
        let application = stub_application(Uuid::new_v4());
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], links: &[], connection: None };

        let script = build_attach_script(&ctx, "vibessh-app-test").unwrap();
        // Match the mode as an argument, not as a bare substring - the
        // fifo path embeds a random UUID, which will occasionally contain
        // "666" on its own and make a substring check flake.
        assert!(!script.contains("chmod 666"), "{script}");
        assert!(!script.contains("mkfifo -m 666"), "{script}");
        assert!(script.contains("mkfifo -m 600 "), "{script}");
        assert!(script.contains("chmod 600 "), "{script}");
    }

    /// The fifo must not sit inside `working_directory`, which is
    /// bind-mounted into the container - a fifo there is reachable by the
    /// very process whose stdin it controls, and by anything else that can
    /// read that directory.
    #[test]
    fn build_attach_script_keeps_the_fifo_out_of_the_bind_mounted_directory() {
        let application = stub_application(Uuid::new_v4());
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], links: &[], connection: None };

        let fifo = console_fifo_path(application.id);
        assert!(!fifo.starts_with(&application.working_directory), "{fifo}");

        // ...and an Application started by an older build must have its
        // old, world-writable fifo removed rather than left behind.
        let script = build_attach_script(&ctx, "vibessh-app-test").unwrap();
        assert!(script.contains(&format!("sudo rm -f '/srv/my-app/.vibessh-app-{}.stdin'", application.id)), "{script}");
    }

    #[test]
    fn build_attach_script_rejects_an_invalid_container_name() {
        let application = stub_application(Uuid::new_v4());
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], links: &[], connection: None };

        assert!(build_attach_script(&ctx, "not; a valid name").is_err());
    }

    /// A Node's own path is a path the container can have, and goes through
    /// unchanged. This is the regression guard for the pair of tests below:
    /// nothing about making Windows work may quietly move a Linux
    /// Application's files to a different directory inside its container.
    #[test]
    fn build_create_command_bind_mounts_and_sets_the_workdir_to_the_working_directory() {
        let application = stub_application(Uuid::new_v4());
        let config = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: None, run_as_dedicated_user: false };
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], links: &[], connection: None };

        let args = build_create_args(&ctx, &config, "vibessh-app-test", None, None).unwrap();
        assert_eq!(flag_value(&args, "-v"), Some("/srv/my-app:/srv/my-app"), "{args:?}");
        assert_eq!(flag_value(&args, "-w"), Some("/srv/my-app"), "{args:?}");
        assert!(index_of(&args, "-v") < index_of(&args, "alpine:latest"), "{args:?}");
    }

    /// The bug this whole rule exists for. Creating a Paper Application on
    /// this computer with the Docker runtime produced
    /// `-v C:\...:C:\... -w C:\...`, and a Linux container has no such
    /// path. The daemon refused it with a message about a path, and there
    /// was nowhere in the wizard to give a different one - the container
    /// side was never asked for, because it was assumed to equal the host
    /// side.
    #[test]
    fn a_windows_working_directory_still_gets_a_posix_path_inside_the_container() {
        let mut application = stub_application(Uuid::new_v4());
        application.server_id = None; // local: the only way to get a Windows path here
        application.working_directory = r"C:\Users\kompu\AppData\Roaming\com.vibessh.app\applications\paper".to_string();
        let config = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: None, run_as_dedicated_user: false };
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], links: &[], connection: None };

        let args = build_create_args(&ctx, &config, "vibessh-app-test", None, None).unwrap();
        // The host half keeps the Windows path - that is the directory the
        // daemon is being asked to mount, and Docker Desktop translates it.
        assert_eq!(
            flag_value(&args, "-v"),
            Some(r"C:\Users\kompu\AppData\Roaming\com.vibessh.app\applications\paper:/home/container"),
            "{args:?}"
        );
        assert_eq!(flag_value(&args, "-w"), Some("/home/container"), "{args:?}");
    }

    /// A UNC path is not a drive letter and is still not a container path.
    /// Written down because a rule phrased as "starts with a drive letter"
    /// would pass the test above and fail here.
    #[test]
    fn a_unc_working_directory_is_replaced_too() {
        let mut application = stub_application(Uuid::new_v4());
        application.server_id = None;
        application.working_directory = r"\\nas\share\minecraft".to_string();
        let config = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: None, run_as_dedicated_user: false };
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], links: &[], connection: None };

        let args = build_create_args(&ctx, &config, "vibessh-app-test", None, None).unwrap();
        assert_eq!(flag_value(&args, "-v"), Some(r"\\nas\share\minecraft:/home/container"), "{args:?}");
        assert_eq!(flag_value(&args, "-w"), Some("/home/container"), "{args:?}");
    }

    /// `C:\Program Files\...` is where a path with a space actually turns
    /// up, and these are argv entries rather than a command line, so the
    /// space must survive inside one argument. `build_create_args` promising
    /// that is the reason the local runner hands the arguments to the
    /// process directly instead of rendering a shell string.
    #[test]
    fn a_windows_path_with_a_space_stays_one_argument() {
        let mut application = stub_application(Uuid::new_v4());
        application.server_id = None;
        application.working_directory = r"C:\Program Files\VibeSSH\my app".to_string();
        let config = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: None, run_as_dedicated_user: false };
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], links: &[], connection: None };

        let args = build_create_args(&ctx, &config, "vibessh-app-test", None, None).unwrap();
        assert_eq!(flag_value(&args, "-v"), Some(r"C:\Program Files\VibeSSH\my app:/home/container"), "{args:?}");
        assert!(!args.iter().any(|arg| arg == "Files\\VibeSSH\\my"), "the path was split: {args:?}");
    }

    /// The jar is a bare filename resolved against the container's cwd
    /// (`render_java_config` builds `java -jar <filename>`), so moving that
    /// cwd is only safe while the mount lands on it. Both halves are checked
    /// together because it is the pair that has to agree - a fixed `-w` with
    /// the mount left somewhere else would start a container whose jar is
    /// not there.
    #[test]
    fn the_workdir_is_always_the_far_end_of_the_mount() {
        for directory in ["/srv/my-app", r"C:\srv\my-app", r"\\nas\share\app"] {
            let mut application = stub_application(Uuid::new_v4());
            application.working_directory = directory.to_string();
            let config = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: None, run_as_dedicated_user: false };
            let runtime_config = serde_json::json!({});
            let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], links: &[], connection: None };

            let args = build_create_args(&ctx, &config, "vibessh-app-test", None, None).unwrap();
            let mount = flag_value(&args, "-v").unwrap();
            let workdir = flag_value(&args, "-w").unwrap();
            assert!(mount.ends_with(&format!(":{workdir}")), "{directory}: {mount} does not end at {workdir}");
            assert!(workdir.starts_with('/'), "{directory}: {workdir} is not a path a Linux container can have");
        }
    }

    #[test]
    fn build_create_command_defaults_restart_policy_to_unless_stopped() {
        let application = stub_application(Uuid::new_v4());
        let config = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: None, run_as_dedicated_user: false };
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], links: &[], connection: None };

        let args = build_create_args(&ctx, &config, "vibessh-app-test", None, None).unwrap();
        assert_eq!(flag_value(&args, "--restart"), Some("unless-stopped"), "{args:?}");
    }

    #[test]
    fn build_create_command_honors_an_explicit_restart_policy_and_rejects_an_invalid_one() {
        let application = stub_application(Uuid::new_v4());
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], links: &[], connection: None };

        let always = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: Some("always".into()), run_as_dedicated_user: false };
        let args = build_create_args(&ctx, &always, "vibessh-app-test", None, None).unwrap();
        assert_eq!(flag_value(&args, "--restart"), Some("always"), "{args:?}");

        let bogus = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: Some("whenever".into()), run_as_dedicated_user: false };
        assert!(build_create_args(&ctx, &bogus, "vibessh-app-test", None, None).is_err());
    }

    fn stub_port(protocol: PortProtocol, bind_address: &str, internal_port: u16, external_port: Option<u16>) -> ApplicationPort {
        ApplicationPort {
            id: Uuid::new_v4(),
            application_id: Uuid::new_v4(),
            name: "game".to_string(),
            protocol,
            bind_address: bind_address.to_string(),
            internal_port,
            external_port,
            visibility: crate::models::PortVisibility::Public,
            required: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn build_create_command_publishes_only_ports_with_an_external_port_set() {
        let application = stub_application(Uuid::new_v4());
        let config = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: None, run_as_dedicated_user: false };
        let runtime_config = serde_json::json!({});
        let ports = vec![
            stub_port(PortProtocol::Tcp, "0.0.0.0", 25565, Some(25565)),
            stub_port(PortProtocol::Udp, "0.0.0.0", 24454, Some(24454)),
            stub_port(PortProtocol::Tcp, "127.0.0.1", 3306, None),
        ];
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &ports, links: &[], connection: None };

        let args = build_create_args(&ctx, &config, "vibessh-app-test", None, None).unwrap();
        assert!(args.iter().any(|arg| arg == "0.0.0.0:25565:25565/tcp"), "{args:?}");
        assert!(args.iter().any(|arg| arg == "0.0.0.0:24454:24454/udp"), "{args:?}");
        // The port with no external_port must not be published at all.
        assert!(!args.iter().any(|arg| arg.contains("3306")), "{args:?}");
        assert!(index_of(&args, "-p") < index_of(&args, "alpine:latest"), "{args:?}");
    }

    #[test]
    fn build_create_command_publishes_nothing_when_no_ports_are_declared() {
        let application = stub_application(Uuid::new_v4());
        let config = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: None, run_as_dedicated_user: false };
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], links: &[], connection: None };

        let args = build_create_args(&ctx, &config, "vibessh-app-test", None, None).unwrap();
        assert!(index_of(&args, "-p").is_none(), "{args:?}");
    }

    #[test]
    fn build_create_command_rejects_a_newline_in_a_ports_bind_address() {
        let application = stub_application(Uuid::new_v4());
        let config = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: None, run_as_dedicated_user: false };
        let runtime_config = serde_json::json!({});
        let ports = vec![stub_port(PortProtocol::Tcp, "0.0.0.0\nrm -rf /", 25565, Some(25565))];
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &ports, links: &[], connection: None };

        assert!(build_create_args(&ctx, &config, "vibessh-app-test", None, None).is_err());
    }

    #[test]
    fn build_create_command_omits_limit_flags_when_unset() {
        let application = stub_application(Uuid::new_v4());
        let config = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: None, run_as_dedicated_user: false };
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], links: &[], connection: None };

        let args = build_create_args(&ctx, &config, "vibessh-app-test", None, None).unwrap();
        assert!(index_of(&args, "--memory").is_none(), "{args:?}");
        assert!(index_of(&args, "--cpus").is_none(), "{args:?}");
    }

    #[test]
    fn build_create_command_rejects_a_zero_memory_limit_or_non_positive_cpu_limit() {
        let application = stub_application(Uuid::new_v4());
        let runtime_config = serde_json::json!({});
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], links: &[], connection: None };

        let zero_memory = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: Some(0), cpu_limit_cores: None, restart_policy: None, run_as_dedicated_user: false };
        assert!(build_create_args(&ctx, &zero_memory, "vibessh-app-test", None, None).is_err());

        let negative_cpu = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: Some(-1.0), restart_policy: None, run_as_dedicated_user: false };
        assert!(build_create_args(&ctx, &negative_cpu, "vibessh-app-test", None, None).is_err());
    }

    /// The decision three methods have to agree on, and did not.
    ///
    /// A local Paper application was created successfully and then refused
    /// to start, with "DockerRuntime requires a connection" - a message
    /// about SSH shown to somebody who had chosen "this computer".
    /// `create_container` knew a local daemon has no POSIX accounts;
    /// `start` and `restart` still demanded a connection to set one up. The
    /// flag is set unconditionally by every Java blueprint, so every
    /// Minecraft application on this machine hit it.
    #[test]
    fn a_local_daemon_never_claims_the_dedicated_user_however_the_blueprint_asked() {
        let asked = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: None, run_as_dedicated_user: true };
        let did_not_ask = DockerConfig { run_as_dedicated_user: false, ..asked.clone() };

        // Locally: refused whatever the blueprint said, which is what keeps
        // `connection_ref` from ever being reached without a connection.
        assert!(!dedicated_user_applies(&asked, &LocalDocker));
        assert!(!dedicated_user_applies(&did_not_ask, &LocalDocker));

        // And the flag still means something where it can be honoured -
        // a rule that always answered "no" would silently drop the
        // isolation on every remote Node instead.
        struct Remote;
        #[async_trait::async_trait]
        impl DockerCommandRunner for Remote {
            async fn docker(&self, _args: &[&str]) -> AppResult<crate::transport::CommandOutput> {
                unreachable!("this stub exists for `supports_dedicated_user` alone")
            }
            fn supports_dedicated_user(&self) -> bool {
                true
            }
            async fn write_private_file(&self, _contents: &str) -> AppResult<PrivateFile> {
                unreachable!()
            }
            async fn remove_private_directory(&self, _directory: &str) -> AppResult<()> {
                unreachable!()
            }
        }
        assert!(dedicated_user_applies(&asked, &Remote));
        assert!(!dedicated_user_applies(&did_not_ask, &Remote));
    }

    /// This used to assert the opposite - that every one of these failed
    /// with "DockerRuntime requires a connection". No connection is not a
    /// fault any more: `server_id` is `None` exactly when the Application is
    /// local, so its absence is what says "the daemon on this machine".
    #[tokio::test]
    async fn without_a_connection_the_target_is_the_local_daemon() {
        let application = stub_application(Uuid::new_v4());
        let config = serde_json::json!({ "image": "alpine:latest", "command": [] });
        let ctx = RuntimeContext { application: &application, runtime_config: &config, environment: &[], ports: &[], links: &[], connection: None };

        // Asserted through the one thing that differs without needing a
        // daemon to be installed to check it.
        assert!(!runner(&ctx).supports_dedicated_user());
    }

    /// Every Java blueprint sets `run_as_dedicated_user` unconditionally, as
    /// the mechanism by which a remote Node keeps an Application's files out
    /// of root's ownership. Locally there is no such account and no need for
    /// one, so the flag must not be the thing that makes Paper impossible to
    /// run on this machine - which is what refusing it did.
    #[test]
    fn a_local_daemon_does_not_claim_to_offer_a_dedicated_user() {
        assert!(!LocalDocker.supports_dedicated_user());
    }

    /// The `--user` flag is what the dedicated account actually does to the
    /// container, and it is absent when there is no account to name.
    #[test]
    fn no_user_flag_is_rendered_without_a_dedicated_account() {
        let application = stub_application(Uuid::new_v4());
        let runtime_config = serde_json::json!({});
        let config = DockerConfig { image: "alpine:latest".into(), command: vec![], memory_limit_mb: None, cpu_limit_cores: None, restart_policy: None, run_as_dedicated_user: true };
        let ctx = RuntimeContext { application: &application, runtime_config: &runtime_config, environment: &[], ports: &[], links: &[], connection: None };

        let args = build_create_args(&ctx, &config, "vibessh-app-test", None, None).unwrap();

        assert!(index_of(&args, "--user").is_none(), "{args:?}");
    }

    /// `parse_docker_byte_size` reads a number out of remote command output
    /// and it lands in a resource graph. Wrong is bad; panicking on a
    /// container that printed something unexpected takes the whole stats
    /// call down with it.
    mod byte_size_properties {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn never_panics_on_arbitrary_input(value in "\\PC{0,40}") {
                let _ = parse_docker_byte_size(&value);
            }

            /// Multi-byte characters are the interesting case: this splits
            /// the string at a byte index found by searching for the first
            /// non-digit, which would panic if that index were not a
            /// character boundary.
            #[test]
            fn never_panics_on_multibyte_units(number in "[0-9]{1,6}", unit in "\\PC{0,6}") {
                let _ = parse_docker_byte_size(&format!("{number}{unit}"));
            }

            /// Units are ordered, and the parse has to preserve that: 1 GiB
            /// is more than 1 MiB is more than 1 KiB. A transposed
            /// multiplier is the kind of bug that reads as plausible in a
            /// single example.
            #[test]
            fn iec_units_are_strictly_increasing(n in 1u32..1000) {
                let b = parse_docker_byte_size(&format!("{n}B")).unwrap();
                let kib = parse_docker_byte_size(&format!("{n}KiB")).unwrap();
                let mib = parse_docker_byte_size(&format!("{n}MiB")).unwrap();
                let gib = parse_docker_byte_size(&format!("{n}GiB")).unwrap();
                let tib = parse_docker_byte_size(&format!("{n}TiB")).unwrap();
                prop_assert!(b < kib && kib < mib && mib < gib && gib < tib);
                prop_assert_eq!(kib, u64::from(n) * 1024);
            }

            /// A unit Docker never emits must be rejected rather than
            /// silently treated as bytes - a value read as 1000x too small
            /// is worse than no value.
            #[test]
            fn an_unknown_unit_is_rejected(n in 1u32..1000, unit in "[a-zA-Z]{1,4}") {
                prop_assume!(!["B", "KiB", "MiB", "GiB", "TiB", "kB", "MB", "GB"].contains(&unit.as_str()));
                prop_assert_eq!(parse_docker_byte_size(&format!("{n}{unit}")), None);
            }
        }
    }

}
