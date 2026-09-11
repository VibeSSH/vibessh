use std::collections::HashMap;

use crate::errors::AppResult;
use crate::models::{Blueprint, BlueprintFeature, BlueprintField, BlueprintFieldType, RuntimeType};

use super::{text_input, validate_inputs, BlueprintHandler};

/// A self-hosted MariaDB server - data persists in the Application's own
/// bind-mounted working directory via `--datadir=.` (a relative path: Etap
/// M1's Docker runtime always mounts `working_directory` at the *same* path
/// inside the container and sets it as the container's cwd, so `.` already
/// means "the working directory" without this blueprint needing to know its
/// absolute path - the same trick the Java blueprints already use for a bare
/// jar filename). Verified against a real `mariadb:11` container before
/// writing this: `--datadir=.` initializes and starts cleanly under that
/// exact bind-mount shape.
///
/// No `default_ports` deliberately - `services::application_service::
/// create_application` publishes every `default_ports` entry as `Public`
/// (0.0.0.0) immediately on creation with no review step in between, which
/// is the wrong default for a database port. The user adds one themselves
/// on the Ports tab, ideally as VibeNetwork-only rather than Public.
pub struct MariaDbBlueprint {
    definition: Blueprint,
}

impl MariaDbBlueprint {
    pub fn new() -> Self {
        Self {
            definition: Blueprint {
                id: "mariadb".to_string(),
                name: "MariaDB".to_string(),
                description: "A self-hosted MariaDB database server - data is stored in this Application's own working directory. Set MYSQL_ROOT_PASSWORD (and any MYSQL_DATABASE/MYSQL_USER/MYSQL_PASSWORD you want pre-created) in the Environment tab before starting it.".to_string(),
                schema_version: 1,
                blueprint_version: 1,
                supported_runtime_types: vec![RuntimeType::Docker],
                features: vec![BlueprintFeature::Logs, BlueprintFeature::Environment, BlueprintFeature::Ports, BlueprintFeature::HealthCheck, BlueprintFeature::Files],
                fields: vec![BlueprintField {
                    key: "mariadbVersion".to_string(),
                    label: "MariaDB version".to_string(),
                    field_type: BlueprintFieldType::Text,
                    required: false,
                    default_value: Some(serde_json::Value::String("11".to_string())),
                    help_text: Some("A Docker Hub tag, e.g. 11, 10.11, 10.6, or lts.".to_string()),
                }],
                known_files: vec![],
                default_ports: vec![],
                connects_to: None,
                command_console: None,
                is_builtin: true,
            },
        }
    }
}

impl Default for MariaDbBlueprint {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BlueprintHandler for MariaDbBlueprint {
    fn blueprint(&self) -> &Blueprint {
        &self.definition
    }

    fn render_runtime_config(&self, inputs: &HashMap<String, serde_json::Value>) -> AppResult<serde_json::Value> {
        validate_inputs(&self.definition, inputs)?;
        let version = text_input(inputs, &self.definition, "mariadbVersion")?;
        let image = format!("mariadb:{}", version.trim());
        Ok(serde_json::json!({ "image": image, "command": ["--datadir=."] }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_the_versioned_image_and_a_relative_datadir_command() {
        let blueprint = MariaDbBlueprint::new();
        let config = blueprint.render_runtime_config(&HashMap::new()).unwrap();
        assert_eq!(config, serde_json::json!({ "image": "mariadb:11", "command": ["--datadir=."] }));
    }

    #[test]
    fn a_custom_version_overrides_the_default_image_tag() {
        let blueprint = MariaDbBlueprint::new();
        let mut inputs = HashMap::new();
        inputs.insert("mariadbVersion".to_string(), serde_json::json!("10.11"));
        let config = blueprint.render_runtime_config(&inputs).unwrap();
        assert_eq!(config["image"], serde_json::json!("mariadb:10.11"));
    }

    #[test]
    fn declares_no_default_ports_so_nothing_is_published_publicly_without_review() {
        let blueprint = MariaDbBlueprint::new();
        assert!(blueprint.blueprint().default_ports.is_empty());
    }
}
