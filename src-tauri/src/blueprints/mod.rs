//! Blueprints - declarative descriptions of "what kind of application is
//! this" (docs/APPLICATIONS_ARCHITECTURE.md Section 7.5, brief's own
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
//! docs/APPLICATIONS_ARCHITECTURE.md Section 6's own SQL sketch**: this
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
use crate::models::{Blueprint, BlueprintField, BlueprintFieldType};
use crate::ssh::SshSession;

mod generic;
mod generic_docker;
mod generic_java;
mod mariadb;
mod nodejs_bot;
mod paper;
mod phpmyadmin;
mod purpur;
mod python_bot;
mod redis;
mod velocity;
mod waterfall;

pub use generic::GenericBlueprint;
pub use generic_docker::GenericDockerBlueprint;
pub use generic_java::GenericJavaBlueprint;
pub use mariadb::MariaDbBlueprint;
pub use nodejs_bot::NodejsBotBlueprint;
pub use paper::PaperBlueprint;
pub use phpmyadmin::PhpMyAdminBlueprint;
pub use purpur::PurpurBlueprint;
pub use python_bot::PythonBotBlueprint;
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
/// `<major>-jre-alpine` (e.g. `21-jre-alpine`). Used by
/// `PaperBlueprint`/`VelocityBlueprint`/`GenericJavaBlueprint`'s own
/// `render_runtime_config`, all three of which went Docker-only in Etap M1
/// (see each one's own doc comment) - a small, shared source of truth for
/// "which image a Java version maps to" rather than three copies of the
/// same string formatting.
pub(crate) fn temurin_image(java_version: &str) -> String {
    format!("eclipse-temurin:{}-jre-alpine", java_version.trim())
}

/// Builds the Docker-shape `runtime_config` (`{"image": ..., "command":
/// [...]}`, what `runtime::docker::DockerConfig` deserializes from) for a
/// Java application: `java`, then JVM args, then `-jar <jar>`, then program
/// args - the same argument ordering the pre-Etap-M1 process-shape config
/// used, just as the container's own `command` override instead of a
/// process's `command`+`args` pair (`DockerConfig.command` overrides the
/// image's `ENTRYPOINT`/`CMD` entirely, so this must be the *whole*
/// invocation, not just program arguments).
pub(crate) fn render_java_docker_config(java_version: &str, jvm_args: Vec<String>, jar: String, program_args: Vec<String>) -> serde_json::Value {
    let mut command = vec!["java".to_string()];
    command.extend(jvm_args);
    command.push("-jar".to_string());
    command.push(jar);
    command.extend(program_args);
    serde_json::json!({ "image": temurin_image(java_version), "command": command, "runAsDedicatedUser": true })
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
        let redis = RedisBlueprint::new();
        handlers.insert(redis.blueprint().id.clone(), Box::new(redis));
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
    use super::*;
    use crate::models::BlueprintFieldType;

    fn field(key: &str, required: bool, field_type: BlueprintFieldType, default_value: Option<serde_json::Value>) -> BlueprintField {
        BlueprintField { key: key.to_string(), label: key.to_string(), field_type, required, default_value, help_text: None }
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
        assert!(registry.get("redis").is_some());
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
                "nodejs-bot",
                "paper",
                "phpmyadmin",
                "purpur",
                "python-bot",
                "redis",
                "velocity",
                "waterfall",
            ]
        );
    }
}
