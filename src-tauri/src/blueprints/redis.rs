use std::collections::HashMap;

use crate::errors::AppResult;
use crate::models::{Blueprint, BlueprintFeature, BlueprintField, BlueprintFieldType, RuntimeType};

use super::{text_input, validate_inputs, BlueprintHandler};

/// A self-hosted Redis instance with append-only persistence, storing data
/// in the Application's own bind-mounted working directory via `--dir .` -
/// same relative-path trick `MariaDbBlueprint` uses, verified against a real
/// `redis:7` container before writing this (a `SET`/`GET` round trip and an
/// `appendonlydir` actually appearing in the working directory).
///
/// Unlike MariaDB's root password (a real env var the official image reads
/// itself, so it flows through the ordinary Environment tab untouched),
/// Redis auth is a command-line flag (`--requirepass`) with no env var
/// equivalent - `blueprint_inputs` (this blueprint's own fields) and
/// `environment` (the Environment tab) are two separate channels with no
/// bridge between them, so a command-line-only setting has to be a field
/// here, not left to the Environment tab the way MariaDB's password is.
///
/// No `default_ports` - see `MariaDbBlueprint`'s own doc comment for why.
pub struct RedisBlueprint {
    definition: Blueprint,
}

impl RedisBlueprint {
    pub fn new() -> Self {
        Self {
            definition: Blueprint {
                id: "redis".to_string(),
                name: "Redis".to_string(),
                description: "A self-hosted Redis instance with append-only persistence - data is stored in this Application's own working directory.".to_string(),
                schema_version: 1,
                blueprint_version: 1,
                supported_runtime_types: vec![RuntimeType::Docker],
                features: vec![BlueprintFeature::Logs, BlueprintFeature::Environment, BlueprintFeature::Ports, BlueprintFeature::HealthCheck, BlueprintFeature::Files],
                fields: vec![
                    BlueprintField {
                        key: "redisVersion".to_string(),
                        label: "Redis version".to_string(),
                        field_type: BlueprintFieldType::Text,
                        required: false,
                        default_value: Some(serde_json::Value::String("7".to_string())),
                        help_text: Some("A Docker Hub tag, e.g. 7, 8, or alpine.".to_string()),
                    },
                    BlueprintField {
                        key: "requirePassword".to_string(),
                        label: "Password".to_string(),
                        field_type: BlueprintFieldType::Text,
                        required: false,
                        default_value: None,
                        help_text: Some("Sets --requirepass. Leave empty to run without authentication - only safe if this port stays private (see the Ports tab's visibility setting).".to_string()),
                    },
                ],
                known_files: vec![],
                default_ports: vec![],
                is_builtin: true,
            },
        }
    }
}

impl Default for RedisBlueprint {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BlueprintHandler for RedisBlueprint {
    fn blueprint(&self) -> &Blueprint {
        &self.definition
    }

    fn render_runtime_config(&self, inputs: &HashMap<String, serde_json::Value>) -> AppResult<serde_json::Value> {
        validate_inputs(&self.definition, inputs)?;
        let version = text_input(inputs, &self.definition, "redisVersion")?;
        let password = text_input(inputs, &self.definition, "requirePassword")?;

        let mut command = vec!["redis-server".to_string(), "--dir".to_string(), ".".to_string()];
        if !password.trim().is_empty() {
            command.push("--requirepass".to_string());
            command.push(password);
        }
        command.push("--appendonly".to_string());
        command.push("yes".to_string());

        let image = format!("redis:{}", version.trim());
        Ok(serde_json::json!({ "image": image, "command": command }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_the_versioned_image_and_a_relative_dir_with_persistence_enabled() {
        let blueprint = RedisBlueprint::new();
        let config = blueprint.render_runtime_config(&HashMap::new()).unwrap();
        assert_eq!(config, serde_json::json!({ "image": "redis:7", "command": ["redis-server", "--dir", ".", "--appendonly", "yes"] }));
    }

    #[test]
    fn a_set_password_adds_requirepass_before_the_persistence_flags() {
        let blueprint = RedisBlueprint::new();
        let mut inputs = HashMap::new();
        inputs.insert("requirePassword".to_string(), serde_json::json!("hunter2"));
        let config = blueprint.render_runtime_config(&inputs).unwrap();
        assert_eq!(config["command"], serde_json::json!(["redis-server", "--dir", ".", "--requirepass", "hunter2", "--appendonly", "yes"]));
    }

    #[test]
    fn a_custom_version_overrides_the_default_image_tag() {
        let blueprint = RedisBlueprint::new();
        let mut inputs = HashMap::new();
        inputs.insert("redisVersion".to_string(), serde_json::json!("alpine"));
        let config = blueprint.render_runtime_config(&inputs).unwrap();
        assert_eq!(config["image"], serde_json::json!("redis:alpine"));
    }
}
