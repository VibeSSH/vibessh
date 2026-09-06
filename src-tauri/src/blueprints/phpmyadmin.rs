use std::collections::HashMap;

use crate::errors::AppResult;
use crate::models::{Blueprint, BlueprintConnection, BlueprintFeature, BlueprintField, BlueprintFieldType, RuntimeType};

use super::{text_input, validate_inputs, BlueprintHandler};

/// A web UI for managing a MySQL/MariaDB server.
///
/// `PMA_HOST` and `PMA_PORT` are real env vars the official image reads
/// itself, so they stay ordinary Environment rows rather than becoming
/// blueprint fields - this blueprint still only picks the image. What
/// changed is who fills them in: leaving that to the user is what made this
/// the most reported broken setup in the app, because the host name is the
/// target's network alias (not its Application name as typed) and, on top of
/// that, nothing is reachable across Applications until a connection is
/// granted. `connects_to` below hands all three steps to
/// `create_application`.
///
/// Verified against a real `phpmyadmin:latest` container before writing this
/// (served a real 200 on its default port 80 with no command override needed
/// at all).
pub struct PhpMyAdminBlueprint {
    definition: Blueprint,
}

impl PhpMyAdminBlueprint {
    pub fn new() -> Self {
        Self {
            definition: Blueprint {
                id: "phpmyadmin".to_string(),
                name: "phpMyAdmin".to_string(),
                description: "A web UI for managing a MySQL/MariaDB server. Pick the MariaDB Application it should manage and VibeSSH sets PMA_HOST/PMA_PORT and grants the connection between them; to reach a server VibeSSH does not manage, leave that empty and set PMA_HOST yourself on the Environment tab.".to_string(),
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
                // The whole point of the "Database" picker in the wizard:
                // `create_application` turns the chosen MariaDB Application
                // into PMA_HOST/PMA_PORT rows and grants the connection that
                // makes that host resolvable at all.
                connects_to: Some(BlueprintConnection {
                    blueprint_ids: vec!["mariadb".to_string()],
                    host_env: "PMA_HOST".to_string(),
                    port_env: "PMA_PORT".to_string(),
                    default_port: 3306,
                }),
                command_console: None,
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
