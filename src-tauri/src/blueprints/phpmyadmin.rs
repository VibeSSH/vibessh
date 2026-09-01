use std::collections::HashMap;

use crate::errors::AppResult;
use crate::models::{Blueprint, BlueprintFeature, BlueprintField, BlueprintFieldType, RuntimeType};

use super::{text_input, validate_inputs, BlueprintHandler};

/// A web UI for managing a MySQL/MariaDB server - `PMA_HOST` (required) and
/// `PMA_PORT` (optional, defaults to 3306) are real env vars the official
/// image reads itself, so - same reasoning as `MariaDbBlueprint`'s own root
/// password - they belong on the ordinary Environment tab, not as fields
/// here; this blueprint only picks the right image. Verified against a real
/// `phpmyadmin:latest` container before writing this (served a real 200 on
/// its default port 80 with no command override needed at all).
pub struct PhpMyAdminBlueprint {
    definition: Blueprint,
}

impl PhpMyAdminBlueprint {
    pub fn new() -> Self {
        Self {
            definition: Blueprint {
                id: "phpmyadmin".to_string(),
                name: "phpMyAdmin".to_string(),
                description: "A web UI for managing a MySQL/MariaDB server - point it at any reachable server (e.g. a Database Host, or a MariaDB Application on the Vibe Network) by setting PMA_HOST (and PMA_PORT, if not 3306) in the Environment tab.".to_string(),
                schema_version: 1,
                blueprint_version: 1,
                supported_runtime_types: vec![RuntimeType::Docker],
                features: vec![BlueprintFeature::Logs, BlueprintFeature::Environment, BlueprintFeature::Ports, BlueprintFeature::HealthCheck],
                fields: vec![BlueprintField {
                    key: "phpMyAdminVersion".to_string(),
                    label: "phpMyAdmin version".to_string(),
                    field_type: BlueprintFieldType::Text,
                    required: false,
                    default_value: Some(serde_json::Value::String("latest".to_string())),
                    help_text: Some("A Docker Hub tag, e.g. latest or a specific version.".to_string()),
                }],
                known_files: vec![],
                // No default_ports - see `MariaDbBlueprint`'s own doc
                // comment on why every new template here leaves publishing
                // to the user's own explicit choice on the Ports tab, rather
                // than auto-publishing Public on creation.
                default_ports: vec![],
                is_builtin: true,
            },
        }
    }
}

impl Default for PhpMyAdminBlueprint {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BlueprintHandler for PhpMyAdminBlueprint {
    fn blueprint(&self) -> &Blueprint {
        &self.definition
    }

    fn render_runtime_config(&self, inputs: &HashMap<String, serde_json::Value>) -> AppResult<serde_json::Value> {
        validate_inputs(&self.definition, inputs)?;
        let version = text_input(inputs, &self.definition, "phpMyAdminVersion")?;
        let tag = if version.trim().is_empty() { "latest" } else { version.trim() };
        Ok(serde_json::json!({ "image": format!("phpmyadmin:{tag}"), "command": [] }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_the_latest_tag_by_default() {
        let blueprint = PhpMyAdminBlueprint::new();
        let config = blueprint.render_runtime_config(&HashMap::new()).unwrap();
        assert_eq!(config, serde_json::json!({ "image": "phpmyadmin:latest", "command": [] }));
    }

    #[test]
    fn a_custom_version_overrides_the_default_image_tag() {
        let blueprint = PhpMyAdminBlueprint::new();
        let mut inputs = HashMap::new();
        inputs.insert("phpMyAdminVersion".to_string(), serde_json::json!("5"));
        let config = blueprint.render_runtime_config(&inputs).unwrap();
        assert_eq!(config["image"], serde_json::json!("phpmyadmin:5"));
    }
}
