//! The Blueprint domain model - see `blueprints` module's own doc comment
//! for how this data connects to the four `ApplicationRuntime`
//! implementations, and docs/APPLICATIONS_ARCHITECTURE.md Section 7.5 for
//! why `features` here is a different concept from host-level
//! `AgentCapabilities`.

use serde::{Deserialize, Serialize};

use super::{PortProtocol, RuntimeType};

/// A declarative description of "what kind of application is this" - which
/// `RuntimeType`s it can run under, which UI features it needs
/// (`features`), and which inputs a Create Application wizard would ask
/// for (`fields`). Behavior (turning filled-in `fields` into a concrete
/// `runtime_config`) lives separately, in a `blueprints::BlueprintHandler`
/// - this struct is pure data, serializable as-is to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blueprint {
    /// e.g. `"generic"`, `"generic-java"` - stored verbatim as
    /// `Application::blueprint_id`.
    pub id: String,
    pub name: String,
    pub description: String,
    /// The version of *this struct's own shape* - bumped if
    /// `Blueprint`/`BlueprintField`'s fields themselves ever change in a
    /// way older stored definitions couldn't parse. Distinct from
    /// `blueprint_version`, which versions one specific blueprint's own
    /// content.
    pub schema_version: i32,
    pub blueprint_version: i32,
    pub supported_runtime_types: Vec<RuntimeType>,
    pub features: Vec<BlueprintFeature>,
    pub fields: Vec<BlueprintField>,
    /// "Quick Files" shortcuts on the Files tab (design brief's Known File
    /// Shortcuts section) - paths relative to `working_directory` this
    /// blueprint knows are worth surfacing directly (Paper's
    /// `server.properties`, Velocity's `velocity.toml`, ...), each opening
    /// through the exact same Files/editor UI as browsing to it by hand -
    /// never a separate, blueprint-specific editor.
    #[serde(default)]
    pub known_files: Vec<KnownFile>,
    /// Ports this blueprint's service is conventionally reached on (Paper/
    /// Velocity's `25565`) - created as real, `Public`-visibility
    /// `ApplicationPort` rows the moment the Application is (`services::
    /// application_service::create_application`), not left for the user to
    /// discover and add by hand on the Ports tab before their server is
    /// actually reachable from outside. The "plug and play" bar this whole
    /// feature set follows: a Node's firewall reconcile (`services::
    /// firewall_service::desired_rules`) already derives its rules from
    /// exactly these `ApplicationPort` rows, so a blueprint that forgets to
    /// declare its own default port would also silently never get a
    /// firewall rule for it, on top of never getting Docker's own `-p`
    /// publish flag (`runtime::docker::build_create_command`). Empty by
    /// default (`GenericBlueprint`/`GenericDockerBlueprint`/
    /// `GenericJavaBlueprint` have no single well-known port to assume) -
    /// still user-editable/removable afterward like any other port, this is
    /// only ever the *starting* value.
    #[serde(default)]
    pub default_ports: Vec<DefaultPort>,
    /// Another Application this one is meant to talk to, and how it is told
    /// where to find it - see `BlueprintConnection`. `None` for anything
    /// that stands alone, which is most of them.
    #[serde(default)]
    pub connects_to: Option<BlueprintConnection>,
    /// How this kind of application answers an ad-hoc command - see
    /// `BlueprintCommandConsole`. `None` for anything with no such notion,
    /// which is most of them.
    #[serde(default)]
    pub command_console: Option<BlueprintCommandConsole>,
    pub is_builtin: bool,
}

/// A console for asking a *server* something, as opposed to typing into a
/// process's stdin.
///
/// **Why this is a second mechanism and not the existing console.** The
/// console on the Overview tab writes to the container's stdin, which works
/// because a Minecraft server reads its commands from there. A database does
/// not: `redis-server` and `mongod` ignore stdin entirely, so wiring them to
/// that console would produce an input box that silently swallows everything
/// typed into it. Asking a database something means running its client -
/// `redis-cli`, `mongosh` - and that is a different shape: one command in,
/// one answer out.
///
/// **Each command is its own run.** No session is kept, so nothing carries
/// over between commands - `use some-database` in mongosh applies to the
/// command it was typed with and nothing after it. The alternative, a
/// long-lived interactive session, needs a bidirectional stream built twice
/// over (once for local Docker, once through the FIFO an SSH channel
/// forces), and this shape reuses the ordinary command runner that already
/// works both ways.
///
/// **Credentials never appear in what VibeSSH runs.** `shell` is executed
/// *inside the container* with `sh -c`, so a password can be read from the
/// container's own environment there and expanded there. Nothing sensitive
/// reaches the argument list of the `docker` command this host runs, which
/// is what a local `ps` - or any account on the Node - would be able to read.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintCommandConsole {
    /// Run inside the container as `sh -c <shell> vibessh <command>`, so the
    /// user's command arrives as `"$1"` - a positional parameter, never
    /// spliced into the script. It cannot end the script and start another,
    /// whatever it contains.
    pub shell: String,
    /// An example command, shown in the empty input box. The fastest way to
    /// tell somebody what this console speaks.
    pub placeholder: String,
}

/// How a blueprint that exists to point at *another* Application gets told
/// which one.
///
/// **Why this is declared rather than hardcoded in the wizard.** phpMyAdmin
/// with no `PMA_HOST` is the single most reported broken setup in this app,
/// and it breaks in three places at once: the variable is unset, the port is
/// unset, and - the part nobody guesses - the two containers are on separate
/// private networks, so even a correct host name resolves to nothing until a
/// connection is granted (`services::application_service::links`). All three
/// are mechanical once the target is known, so the wizard asks for the target
/// and `create_application` does the rest.
///
/// Declaring it on the blueprint keeps that out of the wizard's own logic:
/// anything else pointed at a sibling service later (a Grafana at a
/// Prometheus, a bot at a Redis) fills this in and gets the same treatment
/// with no new UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintConnection {
    /// Blueprint ids whose Applications are valid targets. Empty means any
    /// Docker Application on the same Node, which is the honest answer for
    /// something like phpMyAdmin only if it really can talk to anything -
    /// prefer naming the ones that work.
    pub blueprint_ids: Vec<String>,
    /// Given the target's own network alias - the name it is resolvable by
    /// on the network the granted connection creates.
    pub host_env: String,
    /// Given `default_port`. Separate from `host_env` because the port is
    /// the half somebody may legitimately want to change afterwards on the
    /// Environment tab.
    pub port_env: String,
    /// The port the target listens on *inside* its container, which is not
    /// affected by what it publishes to the host - a database deliberately
    /// left unpublished is still on 3306 to a container that has been
    /// granted a route to it.
    pub default_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownFile {
    /// Relative to the Application's own `working_directory` - resolved
    /// (and sandboxed) through the same `ApplicationFileProvider` any other
    /// Files path goes through, not a special-cased lookup.
    pub path: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultPort {
    pub name: String,
    pub protocol: PortProtocol,
    pub internal_port: u16,
    /// Published to this same port on the host by default - the ordinary
    /// case for a single well-known service port. Still just a starting
    /// value the user can change afterward, same as `internal_port`.
    pub external_port: u16,
}

/// UI-facing capabilities this application exposes - not host-level
/// `AgentCapabilities` (does the host have Docker/systemd at all), a
/// different concept covering what tabs/actions the Application detail
/// page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlueprintFeature {
    Console,
    Logs,
    Environment,
    Ports,
    HealthCheck,
    /// Gates the Databases tab (docs/APPLICATIONS_ARCHITECTURE.md Section
    /// 12.2) - not declared by every blueprint the way Health Check/Ports
    /// are; a self-managed database only makes sense for something that
    /// actually talks to one, matching the original brief's own
    /// Paper/Velocity/Generic Java tab list.
    Databases,
    /// Gates the Files tab (`files::ApplicationFileProvider`) - declared by
    /// every built-in blueprint, unlike Databases: any Application has a
    /// `working_directory` worth browsing, regardless of what runs in it.
    Files,
}

/// One input a Create Application wizard would collect for this blueprint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintField {
    pub key: String,
    pub label: String,
    pub field_type: BlueprintFieldType,
    pub required: bool,
    /// JSON rather than a typed value, matching `field_type` - so a
    /// `TextList` field's default can be a real `[]`/`["a","b"]`, not a
    /// stringly-typed encoding of one.
    pub default_value: Option<serde_json::Value>,
    pub help_text: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlueprintFieldType {
    Text,
    Path,
    Number,
    Boolean,
    TextList,
    /// A path, same as `Path` for validation/storage purposes - the
    /// frontend renders it differently: a picker populated from
    /// `detect_java_installations` (real, actually-installed JVMs) with a
    /// free-text fallback, rather than a plain input the user has to know
    /// a path for. Not a generic "Select" type - there's nothing else
    /// today that needs host-detected, dynamically-populated options, and
    /// inventing that generality before a second user exists would be
    /// speculative.
    JavaVersion,
    /// A string, same as `Text` for validation/storage purposes - the
    /// frontend renders it as a picker populated from whichever PaperMC
    /// project's release list matches the field (`list_paper_versions` /
    /// `list_velocity_versions`, keyed off the field's own `key`), same
    /// "dynamic picker, not a hand-typed value" shape as `JavaVersion`.
    /// Named for the API family, not "Minecraft version" - Velocity's own
    /// release numbers (`"3.4.0"`) aren't Minecraft version numbers at all,
    /// even though Paper's happen to be.
    PapermcVersion,
}
