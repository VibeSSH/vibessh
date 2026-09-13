//! Blueprints - declarative descriptions of "what kind of application is
//! this" (docs/architecture/APPLICATIONS_ARCHITECTURE.md Section 7.5, brief's own
//! "declarative, versioned schema controlling both backend behavior AND
//! UI"). `models::Blueprint` is the *data* - id, display info, which
//! `RuntimeType`s it supports, which UI features it needs, and the input
//! fields a Create Application wizard (Phase 6, not built yet) would ask
//! for. A `BlueprintHandler` is the *behavior* attached to that data:
//! turning a filled-in set of inputs into the concrete `runtime_config`
//! JSON one of the four `ApplicationRuntime` implementations
//! (`runtime::{local_process,systemd,remote_process,docker}`) actually
//! reads - e.g. `{"command": "...", "args": [...]}`, the shape
//! `LocalProcessConfig`/`SystemdConfig`/`RemoteProcessConfig` all share.
//!
//! **Deliberate scope decision, differing from
//! docs/architecture/APPLICATIONS_ARCHITECTURE.md Section 6's own SQL sketch**: this
//! phase does NOT persist blueprint definitions into a `blueprints` SQLite
//! table. That table's only real purpose is versioning/serving *custom*
//! (user-imported) blueprint definitions - a feature that doesn't exist yet
//! (no import flow, no Tauri commands calling this module at all so far).
//! Built-in blueprints are inherently code (their render logic has to be
//! Rust regardless of where their static data lives), so persisting a
//! redundant copy of that data before anything queries it that way would
//! be unused schema. Revisit once custom blueprint import is actually
//! being built - `BlueprintRegistry` below is the seam that work would
//! extend (e.g. a second, SQLite-backed source merged into `list()`).

use std::collections::HashMap;
use std::sync::Arc;

use crate::errors::{AppError, AppResult};
use crate::models::{Blueprint, BlueprintField, BlueprintFieldType, RuntimeType};
use crate::ssh::SshSession;

mod generic;
mod generic_docker;
mod generic_java;
mod mariadb;
mod mongodb;
mod nodejs_bot;
mod paper;
mod phpmyadmin;
mod postgres;
mod purpur;
mod python_bot;
mod nats;
mod redis;
mod velocity;
mod waterfall;

pub use generic::GenericBlueprint;
pub use generic_docker::GenericDockerBlueprint;
pub use generic_java::GenericJavaBlueprint;
pub use mariadb::MariaDbBlueprint;
pub use mongodb::MongoDbBlueprint;
pub use postgres::PostgresBlueprint;
pub use nodejs_bot::NodejsBotBlueprint;
pub use paper::PaperBlueprint;
pub use phpmyadmin::PhpMyAdminBlueprint;
pub use purpur::PurpurBlueprint;
pub use python_bot::PythonBotBlueprint;
pub use nats::NatsBlueprint;
pub use redis::RedisBlueprint;
pub use velocity::VelocityBlueprint;
pub use waterfall::WaterfallBlueprint;

/// What a blueprint's `provision` step needs to actually reach the host the
/// application will run on - `None` connection = Local (act on the local
/// filesystem/process directly), `Some` = Remote (act over this SSH
/// session). Deliberately the same Local/Remote split
/// `runtime::RuntimeContext` already uses, not a new concept.
pub struct ProvisionContext<'a> {
    pub working_directory: &'a str,
    pub connection: Option<Arc<SshSession>>,
    /// Which runtime this Application was created for.
    ///
    /// A blueprint's provisioning differs by it: a Java server run as a local
    /// process needs a JVM on this machine, where the Docker one gets its own
    /// from the image.
    pub runtime_type: RuntimeType,
    /// Where a downloaded Java runtime is kept - the app's own data
    /// directory, shared by every Application rather than one copy each.
    pub java_root: &'a std::path::Path,
}

/// Where `provision` records the JVM it found or downloaded, for
/// `render_java_config` to read back. An input rather than a parameter
/// because that is how a blueprint already hands its own discoveries forward
/// - Paper's jar filename travels the same way.
/// Makes a Minecraft server colour its own output.
///
/// Paper's logger asks whether it is writing to a terminal and, finding a
/// pipe, drops the escape sequences. Both of this app's paths are pipes: a
/// container is created without `-t`, and a local process has its stdout
/// captured. So the console rendered plain text no matter how well it could
/// parse colour - there was none to parse.
///
/// Prepended rather than appended, so anyone who sets the property
/// themselves overrides this rather than fighting it: later `-D` flags win.
pub(crate) fn with_forced_ansi(mut jvm_args: Vec<String>) -> Vec<String> {
    jvm_args.insert(0, "-Dterminal.ansi=true".to_string());
    jvm_args
}

pub(crate) const JAVA_PATH_KEY: &str = "javaPath";

/// Records a JVM for an Application that will run as a local process.
///
/// Nothing to do for Docker: the image carries its own. For a local process
/// this is the difference between "install a JDK first" and clicking create -
/// an already-installed Java is used when there is one, and otherwise a
/// runtime is downloaded into the app's own directory.
pub(crate) async fn ensure_java_for(
    context: &ProvisionContext<'_>,
    java_version: &str,
    discovered: &mut HashMap<String, serde_json::Value>,
) -> AppResult<()> {
    if context.runtime_type != RuntimeType::LocalProcess {
        return Ok(());
    }
    let java = crate::services::java_runtime_service::ensure_java(context.java_root, java_version).await?;
    discovered.insert(JAVA_PATH_KEY.to_string(), serde_json::Value::String(java.to_string_lossy().into_owned()));
    Ok(())
}

/// Turns a filled-in set of wizard inputs into the concrete `runtime_config`
/// JSON stored on an `Application` (`application_runtime_config.config_json`
/// - what a future `RuntimeContext.runtime_config` gets deserialized from).
#[async_trait::async_trait]
pub trait BlueprintHandler: Send + Sync {
    fn blueprint(&self) -> &Blueprint;
    fn render_runtime_config(&self, inputs: &HashMap<String, serde_json::Value>) -> AppResult<serde_json::Value>;

    /// Runs once, at Application creation - after the working directory
    /// exists, before `render_runtime_config` - for whatever a blueprint
    /// needs set up before its command can actually run (downloading a
    /// server jar, writing a EULA acceptance file). Returns inputs
    /// *discovered* during provisioning (e.g. the real filename of a jar
    /// whose exact name wasn't known until it was actually downloaded) -
    /// merged into the input map `render_runtime_config` sees next, so it
    /// never needs its own network/filesystem access to learn the same
    /// thing again. The default no-op covers every blueprint that needs
    /// nothing beyond `render_runtime_config` (Generic, Generic Java) -
    /// only `PaperBlueprint` overrides this so far.
    async fn provision(
        &self,
        _inputs: &HashMap<String, serde_json::Value>,
        _context: &ProvisionContext<'_>,
    ) -> AppResult<HashMap<String, serde_json::Value>> {
        Ok(HashMap::new())
    }
}

/// Every field in `blueprint.fields` is checked for presence (falling back
/// to its `default_value`) and a rough type match - the same two things a
/// Create Application wizard form would enforce client-side, done here too
/// so a handler's `render_runtime_config` never has to defend against a
/// missing/mistyped field itself.
pub fn validate_inputs(blueprint: &Blueprint, inputs: &HashMap<String, serde_json::Value>) -> AppResult<()> {
    for field in &blueprint.fields {
        let value = inputs.get(&field.key).or(field.default_value.as_ref());
        match value {
            None if field.required => return Err(AppError::InvalidInput(format!("'{}' is required", field.label))),
            None => continue,
            Some(value) => validate_field_type(field, value)?,
        }
    }
    Ok(())
}

fn validate_field_type(field: &BlueprintField, value: &serde_json::Value) -> AppResult<()> {
    let matches_type = match field.field_type {
        BlueprintFieldType::Text | BlueprintFieldType::Path | BlueprintFieldType::JavaVersion | BlueprintFieldType::PapermcVersion => {
            value.is_string()
        }
        BlueprintFieldType::Number => value.is_number(),
        BlueprintFieldType::Boolean => value.is_boolean(),
        BlueprintFieldType::TextList => value.is_array() && value.as_array().is_some_and(|items| items.iter().all(serde_json::Value::is_string)),
    };
    if !matches_type {
        return Err(AppError::InvalidInput(format!("'{}' has the wrong type", field.label)));
    }
    Ok(())
}

fn find_field<'a>(blueprint: &'a Blueprint, key: &str) -> AppResult<&'a BlueprintField> {
    blueprint
        .fields
        .iter()
        .find(|field| field.key == key)
        .ok_or_else(|| AppError::Internal(format!("blueprint '{}' has no field '{key}'", blueprint.id)))
}

/// Reads one input by key, falling back to its declared default, as a
/// plain string - used by `GenericBlueprint`/`GenericJavaBlueprint` so they
/// don't each re-derive "field or default, then coerce" by hand. Assumes
/// `validate_inputs` already ran (both handlers here call it first); the
/// `required` check is still real, not dead code, for a handler that calls
/// this without validating first.
pub(crate) fn text_input(inputs: &HashMap<String, serde_json::Value>, blueprint: &Blueprint, key: &str) -> AppResult<String> {
    let field = find_field(blueprint, key)?;
    let value = inputs.get(key).or(field.default_value.as_ref());
    match value.and_then(serde_json::Value::as_str) {
        Some(text) => Ok(text.to_string()),
        None if field.required => Err(AppError::InvalidInput(format!("'{}' is required", field.label))),
        None => Ok(String::new()),
    }
}

/// Reads a boolean input as a plain `bool` - `false` when absent and not
/// required (an unset "accept this" checkbox is exactly the "no" it looks
/// like, not an error), used for `PaperBlueprint`'s EULA acceptance field.
pub(crate) fn bool_input(inputs: &HashMap<String, serde_json::Value>, blueprint: &Blueprint, key: &str) -> AppResult<bool> {
    let field = find_field(blueprint, key)?;
    let value = inputs.get(key).or(field.default_value.as_ref());
    match value.and_then(serde_json::Value::as_bool) {
        Some(flag) => Ok(flag),
        None => Ok(false),
    }
}

/// The Docker image a Java-based Egg's rendered `DockerConfig` runs -
/// Eclipse Temurin's own official, widely-used JRE builds (Adoptium), tagged
/// `<major>-jre` (e.g. `21-jre`). Used by
/// `PaperBlueprint`/`VelocityBlueprint`/`GenericJavaBlueprint`'s own
/// `render_runtime_config`, all three of which went Docker-only in Etap M1
/// (see each one's own doc comment) - a small, shared source of truth for
/// "which image a Java version maps to" rather than three copies of the
/// same string formatting.
pub(crate) fn temurin_image(java_version: &str) -> String {
    // Deliberately *not* the `-alpine` variant. Alpine is musl, and a Java
    // plugin that ships a compiled native library ships a glibc build of it:
    // `sqlite-jdbc` (LuckPerms and most permission and antibot plugins) and
    // netty's native transports both fail to load, with a stack trace whose
    // only clue is `os.name=Linux-Musl`. The saving is about fifty megabytes
    // an image, against plugins that do not load.
    format!("eclipse-temurin:{}-jre", java_version.trim())
}

/// Builds the Docker-shape `runtime_config` (`{"image": ..., "command":
/// [...]}`, what `runtime::docker::DockerConfig` deserializes from) for a
/// Java application: `java`, then JVM args, then `-jar <jar>`, then program
/// args - the same argument ordering the pre-Etap-M1 process-shape config
/// used, just as the container's own `command` override instead of a
/// process's `command`+`args` pair (`DockerConfig.command` overrides the
/// image's `ENTRYPOINT`/`CMD` entirely, so this must be the *whole*
/// invocation, not just program arguments).
///
/// Fallible because of the JVM arguments. They sit *before* `-jar`, and Java
/// stops parsing options at the first argument that is not one - so a stray
/// token there is read as a main class, and the container restarts forever
/// on `Could not find or load main class`. That is what a pasted start
/// script produces: the field splits on whitespace, so `#!/bin/bash` becomes
/// the first argument and Java reports it with the slashes turned into dots.
/// Refused here rather than left to a crash loop, where the error names a
/// class nobody wrote.
///
/// `java_path` is what decides the shape. Absent means Docker, where the
/// image brings its own JVM. Present means this machine's own - a local
/// process, given the path to a runtime that `provision` either found
/// already installed or downloaded.
///
/// `stop_command` is what the application is told to shut itself down with,
/// and only reaches the local shape: a container is stopped by Docker. Paper
/// takes `stop`, the proxies take `end`, and a bare jar takes neither.
pub(crate) fn render_java_config(
    java_version: &str,
    jvm_args: Vec<String>,
    jar: String,
    program_args: Vec<String>,
    java_path: Option<&str>,
    stop_command: Option<&str>,
) -> AppResult<serde_json::Value> {
    for arg in &jvm_args {
        if arg.starts_with('#') {
            return Err(AppError::InvalidInput(format!(
                "'{arg}' is not a JVM argument - this looks like a shell script pasted into the JVM arguments. Put flags like -Xmx4G there, and the jar under 'Jar file'."
            )));
        }
    }

    let mut command = vec!["java".to_string()];
    command.extend(jvm_args);
    command.push("-jar".to_string());
    command.push(jar);
    command.extend(program_args);

    let Some(java_path) = java_path else {
        return Ok(serde_json::json!({ "image": temurin_image(java_version), "command": command, "runAsDedicatedUser": true }));
    };

    // The local shape splits the invocation the way `LocalProcessConfig`
    // wants it: the binary, then its arguments. `command[0]` is the literal
    // "java" the Docker shape needs and the local one replaces with a real
    // path.
    let mut args = command;
    args.remove(0);
    let mut config = serde_json::json!({ "command": java_path, "args": args });
    if let Some(stop_command) = stop_command {
        config["stopCommand"] = serde_json::Value::String(stop_command.to_string());
    }
    Ok(config)
}

pub(crate) fn text_list_input(inputs: &HashMap<String, serde_json::Value>, blueprint: &Blueprint, key: &str) -> AppResult<Vec<String>> {
    let field = find_field(blueprint, key)?;
    let value = inputs.get(key).or(field.default_value.as_ref());
    match value.and_then(serde_json::Value::as_array) {
        Some(items) => items
            .iter()
            .map(|item| item.as_str().map(str::to_string).ok_or_else(|| AppError::InvalidInput(format!("'{}' must be a list of strings", field.label))))
            .collect(),
        None => Ok(Vec::new()),
    }
}

/// Looks up a built-in `BlueprintHandler` by `Blueprint::id` - the one and
/// only place that matches on blueprint id, mirroring
/// `runtime::runtime_type_display_name`'s own "one match site" reasoning -
/// adding a new built-in blueprint later means one more entry in
/// `with_builtins`, not hunting for scattered `if blueprint_id ==
/// "generic"` checks.
pub struct BlueprintRegistry {
    handlers: HashMap<String, Box<dyn BlueprintHandler>>,
}

impl BlueprintRegistry {
    pub fn with_builtins() -> Self {
        let mut handlers: HashMap<String, Box<dyn BlueprintHandler>> = HashMap::new();
        let generic = GenericBlueprint::new();
        handlers.insert(generic.blueprint().id.clone(), Box::new(generic));
        let generic_docker = GenericDockerBlueprint::new();
        handlers.insert(generic_docker.blueprint().id.clone(), Box::new(generic_docker));
        let generic_java = GenericJavaBlueprint::new();
        handlers.insert(generic_java.blueprint().id.clone(), Box::new(generic_java));
        let paper = PaperBlueprint::new();
        handlers.insert(paper.blueprint().id.clone(), Box::new(paper));
        let purpur = PurpurBlueprint::new();
        handlers.insert(purpur.blueprint().id.clone(), Box::new(purpur));
        let velocity = VelocityBlueprint::new();
        handlers.insert(velocity.blueprint().id.clone(), Box::new(velocity));
        let waterfall = WaterfallBlueprint::new();
        handlers.insert(waterfall.blueprint().id.clone(), Box::new(waterfall));
        let mariadb = MariaDbBlueprint::new();
        handlers.insert(mariadb.blueprint().id.clone(), Box::new(mariadb));
        let mongodb = MongoDbBlueprint::new();
        handlers.insert(mongodb.blueprint().id.clone(), Box::new(mongodb));
        let postgres = PostgresBlueprint::new();
        handlers.insert(postgres.blueprint().id.clone(), Box::new(postgres));
        let redis = RedisBlueprint::new();
        handlers.insert(redis.blueprint().id.clone(), Box::new(redis));
        let nats = NatsBlueprint::new();
        handlers.insert(nats.blueprint().id.clone(), Box::new(nats));
        let phpmyadmin = PhpMyAdminBlueprint::new();
        handlers.insert(phpmyadmin.blueprint().id.clone(), Box::new(phpmyadmin));
        let nodejs_bot = NodejsBotBlueprint::new();
        handlers.insert(nodejs_bot.blueprint().id.clone(), Box::new(nodejs_bot));
        let python_bot = PythonBotBlueprint::new();
        handlers.insert(python_bot.blueprint().id.clone(), Box::new(python_bot));
        Self { handlers }
    }

    pub fn get(&self, blueprint_id: &str) -> Option<&dyn BlueprintHandler> {
        self.handlers.get(blueprint_id).map(AsRef::as_ref)
    }

    pub fn list(&self) -> Vec<&Blueprint> {
        let mut blueprints: Vec<&Blueprint> = self.handlers.values().map(|handler| handler.blueprint()).collect();
        blueprints.sort_by(|a, b| a.id.cmp(&b.id));
        blueprints
    }
}

impl Default for BlueprintRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

#[cfg(test)]
mod tests {

    /// Without this the console is grey whatever it can parse: Paper's logger
    /// checks whether it is writing to a terminal, finds a pipe - a container
    /// created without `-t`, or a captured stdout - and drops the colour.
    #[test]
    fn a_minecraft_server_is_told_to_colour_its_own_output() {
        let args = with_forced_ansi(vec!["-Xmx4G".to_string()]);

        assert_eq!(args[0], "-Dterminal.ansi=true");
        // First, so somebody who sets the property themselves wins: later
        // -D flags override earlier ones.
        assert_eq!(args[1], "-Xmx4G");
    }


    /// The Docker shape, unchanged - an image brings its own JVM, so no path
    /// to one is passed and none appears.
    #[test]
    fn without_a_java_path_the_config_names_an_image() {
        let config = render_java_config("21", vec!["-Xmx2G".into()], "server.jar".into(), vec!["nogui".into()], None, Some("stop")).unwrap();

        assert!(config["image"].as_str().unwrap().contains("temurin"), "{config}");
        assert_eq!(config["command"][0], "java");
        // A container is stopped by Docker, so the application's own quit
        // command has nothing to do here.
        assert!(config.get("stopCommand").is_none(), "{config}");
    }

    /// The local shape: a real binary and its arguments, which is what
    /// `LocalProcessConfig` deserializes - and the quit command, because
    /// nothing else will stop a Minecraft server cleanly.
    #[test]
    fn a_java_path_produces_a_local_process_config() {
        let config = render_java_config("21", vec!["-Xmx2G".into()], "server.jar".into(), vec!["nogui".into()], Some("/opt/java/bin/java"), Some("stop")).unwrap();

        assert_eq!(config["command"], "/opt/java/bin/java");
        assert_eq!(config["args"], serde_json::json!(["-Xmx2G", "-jar", "server.jar", "nogui"]));
        assert_eq!(config["stopCommand"], "stop");
        // No image: there is no container.
        assert!(config.get("image").is_none(), "{config}");
    }

    use super::*;
    use crate::models::BlueprintFieldType;

    fn field(key: &str, required: bool, field_type: BlueprintFieldType, default_value: Option<serde_json::Value>) -> BlueprintField {
        BlueprintField { key: key.to_string(), label: key.to_string(), field_type, required, default_value, help_text: None }
    }

    /// The image must not be a musl one, and this is the reason rather than
    /// a preference.
    ///
    /// A Java plugin that ships a compiled native library ships a glibc
    /// build of it. On Alpine that library simply is not found, and the only
    /// hint in the resulting stack trace is `os.name=Linux-Musl` - reported
    /// from a real server whose antibot plugin could not load `sqlite-jdbc`.
    /// The failure is at plugin load, far from anything that names an image.
    #[test]
    fn java_runs_on_glibc_so_plugins_with_native_libraries_load() {
        for version in ["8", "17", "21", "25"] {
            let image = temurin_image(version);
            assert!(!image.contains("alpine"), "{image} is musl; plugins shipping native libraries cannot load there");
            assert_eq!(image, format!("eclipse-temurin:{version}-jre"));
        }
        // Whitespace around a version typed into the Settings tab must not
        // reach the image reference.
        assert_eq!(temurin_image("  21  "), "eclipse-temurin:21-jre");
    }

    fn stub_blueprint(fields: Vec<BlueprintField>) -> Blueprint {
        Blueprint {
            id: "stub".to_string(),
            name: "Stub".to_string(),
            description: String::new(),
            schema_version: 1,
            blueprint_version: 1,
            supported_runtime_types: vec![],
            features: vec![],
            fields,
            known_files: vec![],
            default_ports: vec![],
            connects_to: None,
            command_console: None,
            is_builtin: true,
        }
    }

    #[test]
    fn validate_inputs_rejects_a_missing_required_field() {
        let blueprint = stub_blueprint(vec![field("name", true, BlueprintFieldType::Text, None)]);
        assert!(validate_inputs(&blueprint, &HashMap::new()).is_err());
    }

    #[test]
    fn validate_inputs_accepts_a_missing_optional_field_with_a_default() {
        let blueprint = stub_blueprint(vec![field("count", false, BlueprintFieldType::Number, Some(serde_json::json!(1)))]);
        assert!(validate_inputs(&blueprint, &HashMap::new()).is_ok());
    }

    #[test]
    fn validate_inputs_rejects_a_type_mismatch() {
        let blueprint = stub_blueprint(vec![field("count", true, BlueprintFieldType::Number, None)]);
        let mut inputs = HashMap::new();
        inputs.insert("count".to_string(), serde_json::json!("not a number"));
        assert!(validate_inputs(&blueprint, &inputs).is_err());
    }

    #[test]
    fn validate_inputs_rejects_a_text_list_containing_a_non_string() {
        let blueprint = stub_blueprint(vec![field("args", true, BlueprintFieldType::TextList, None)]);
        let mut inputs = HashMap::new();
        inputs.insert("args".to_string(), serde_json::json!(["ok", 5]));
        assert!(validate_inputs(&blueprint, &inputs).is_err());
    }

    #[test]
    fn registry_contains_every_builtin_sorted_by_id() {
        let registry = BlueprintRegistry::with_builtins();
        assert!(registry.get("generic").is_some());
        assert!(registry.get("generic-docker").is_some());
        assert!(registry.get("generic-java").is_some());
        assert!(registry.get("paper").is_some());
        assert!(registry.get("purpur").is_some());
        assert!(registry.get("velocity").is_some());
        assert!(registry.get("waterfall").is_some());
        assert!(registry.get("mariadb").is_some());
        assert!(registry.get("mongodb").is_some());
        assert!(registry.get("redis").is_some());
        assert!(registry.get("nats").is_some());
        assert!(registry.get("phpmyadmin").is_some());
        assert!(registry.get("nodejs-bot").is_some());
        assert!(registry.get("python-bot").is_some());
        assert!(registry.get("nonexistent").is_none());

        let ids: Vec<&str> = registry.list().iter().map(|blueprint| blueprint.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "generic",
                "generic-docker",
                "generic-java",
                "mariadb",
                "mongodb",
                "nats",
                "nodejs-bot",
                "paper",
                "phpmyadmin",
                "postgres",
                "purpur",
                "python-bot",
                "redis",
                "velocity",
                "waterfall",
            ]
        );
    }
}
