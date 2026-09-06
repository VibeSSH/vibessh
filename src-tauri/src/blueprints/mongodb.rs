use std::collections::HashMap;

use crate::errors::AppResult;
use crate::models::{Blueprint, BlueprintCommandConsole, BlueprintFeature, BlueprintField, BlueprintFieldType, RuntimeType};

use super::{text_input, validate_inputs, BlueprintHandler};

/// A self-hosted MongoDB server - a document database, where MariaDB is a
/// relational one.
///
/// **Data lives in the Application's own working directory** via
/// `--dbpath .`, the same relative-path trick `MariaDbBlueprint` and
/// `RedisBlueprint` use: the Docker runtime mounts `working_directory` at the
/// same path inside the container and makes it the container's cwd, so `.`
/// already means "the working directory" without this blueprint needing to
/// know its absolute path. That also means a backup of the Application is a
/// backup of the database.
///
/// **The root account comes from the environment, not from a field here.**
/// `MONGO_INITDB_ROOT_USERNAME` and `MONGO_INITDB_ROOT_PASSWORD` are read by
/// the official image itself, and setting both is also what makes its
/// entrypoint start the server with authentication enabled - so they belong
/// on the ordinary Environment tab next to every other variable that image
/// documents, exactly like MariaDB's own root password. The built-in
/// template puts both rows in front of the user with the password marked
/// secret, which is where somebody actually learns the names.
///
/// Unlike `MariaDbBlueprint`, this one has **not** been run against a real
/// container from inside VibeSSH yet. The shape is derived from the official
/// image's documented behaviour and mirrors the MariaDB blueprint that was
/// verified; the first real deployment is the check.
pub struct MongoDbBlueprint {
    definition: Blueprint,
}

impl MongoDbBlueprint {
    pub fn new() -> Self {
        Self {
            definition: Blueprint {
                id: "mongodb".to_string(),
                name: "MongoDB".to_string(),
                description: "A self-hosted MongoDB document database - data is stored in this Application's own working directory. Set MONGO_INITDB_ROOT_USERNAME and MONGO_INITDB_ROOT_PASSWORD in the Environment tab before starting it: together they create the administrator account and turn authentication on.".to_string(),
                schema_version: 1,
                blueprint_version: 1,
                supported_runtime_types: vec![RuntimeType::Docker],
                features: vec![BlueprintFeature::Logs, BlueprintFeature::Environment, BlueprintFeature::Ports, BlueprintFeature::HealthCheck, BlueprintFeature::Files],
                fields: vec![BlueprintField {
                    key: "mongodbVersion".to_string(),
                    label: "MongoDB version".to_string(),
                    field_type: BlueprintFieldType::Text,
                    required: false,
                    default_value: Some(serde_json::Value::String("8".to_string())),
                    help_text: Some("A Docker Hub tag, e.g. 8, 7, or 6.".to_string()),
                }],
                known_files: vec![],
                // No default_ports, for the reason `MariaDbBlueprint` sets
                // out at length: everything declared there is published as
                // Public the moment the Application is created, with no
                // review step, and that is the wrong default for a database
                // port. The user adds one on the Ports tab, ideally scoped
                // to the Vibe Network.
                default_ports: vec![],
                connects_to: None,
                // The administrator's password is expanded *inside the
                // container*, from the environment the image already holds -
                // so it never reaches the argument list of the `docker`
                // command this host runs, where a local `ps` would read it.
                // Without those variables the server has no authentication
                // either, and the plain client is the right call.
                command_console: Some(BlueprintCommandConsole {
                    shell: r#"if [ -n "$MONGO_INITDB_ROOT_USERNAME" ]; then exec mongosh --quiet -u "$MONGO_INITDB_ROOT_USERNAME" -p "$MONGO_INITDB_ROOT_PASSWORD" --authenticationDatabase admin --eval "$1"; fi; exec mongosh --quiet --eval "$1""#.to_string(),
                    placeholder: "db.getMongo().getDBNames()".to_string(),
                }),
                is_builtin: true,
            },
        }
    }
}

impl Default for MongoDbBlueprint {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BlueprintHandler for MongoDbBlueprint {
    fn blueprint(&self) -> &Blueprint {
        &self.definition
    }

    fn render_runtime_config(&self, inputs: &HashMap<String, serde_json::Value>) -> AppResult<serde_json::Value> {
        validate_inputs(&self.definition, inputs)?;
        let version = text_input(inputs, &self.definition, "mongodbVersion")?;
        let version = version.trim();
        let tag = if version.is_empty() { "8" } else { version };
        // Flags only, no `mongod`: the image's entrypoint prepends it when
        // the first argument starts with a dash, and this is the same shape
        // `MariaDbBlueprint` passes `--datadir=.` in. Going through the
        // entrypoint rather than around it is what keeps the root-account
        // and authentication handling the image does for itself.
        Ok(serde_json::json!({ "image": format!("mongo:{tag}"), "command": ["--dbpath", "."] }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_the_versioned_image_and_a_relative_dbpath() {
        let blueprint = MongoDbBlueprint::new();

        let config = blueprint.render_runtime_config(&HashMap::new()).unwrap();

        assert_eq!(config, serde_json::json!({ "image": "mongo:8", "command": ["--dbpath", "."] }));
    }

    #[test]
    fn a_custom_version_overrides_the_default_image_tag() {
        let blueprint = MongoDbBlueprint::new();
        let mut inputs = HashMap::new();
        inputs.insert("mongodbVersion".to_string(), serde_json::json!("7"));

        let config = blueprint.render_runtime_config(&inputs).unwrap();

        assert_eq!(config["image"], serde_json::json!("mongo:7"));
    }

    /// A blank field is somebody clearing the box, not a request for
    /// `mongo:` - which is not a valid reference and would fail at pull time
    /// with an error about the tag rather than about the empty field.
    #[test]
    fn an_empty_version_falls_back_to_the_default_tag() {
        let blueprint = MongoDbBlueprint::new();
        let mut inputs = HashMap::new();
        inputs.insert("mongodbVersion".to_string(), serde_json::json!("   "));

        let config = blueprint.render_runtime_config(&inputs).unwrap();

        assert_eq!(config["image"], serde_json::json!("mongo:8"));
    }

    /// The database's port is not published on creation - see the blueprint's
    /// own comment. This is the same guard `MariaDbBlueprint` carries.
    #[test]
    fn nothing_is_published_on_creation() {
        assert!(MongoDbBlueprint::new().blueprint().default_ports.is_empty());
    }
}
