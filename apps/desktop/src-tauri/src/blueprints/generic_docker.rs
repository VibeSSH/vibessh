use std::collections::HashMap;

use crate::errors::AppResult;
use crate::models::{Blueprint, BlueprintFeature, BlueprintField, BlueprintFieldType, RuntimeType};

use super::{text_input, text_list_input, validate_inputs, BlueprintHandler};

/// Runs any Docker image, no framework-specific assumptions - the Docker
/// counterpart to `GenericBlueprint`, which deliberately excludes
/// `RuntimeType::Docker` because a raw command isn't a Docker image (see
/// that blueprint's own doc comment). `runtime::docker::DockerRuntime` has
/// been fully implemented since Phase 4; this is the first (and, so far,
/// only) built-in blueprint that actually declares support for it, closing
/// that gap.
pub struct GenericDockerBlueprint {
    definition: Blueprint,
}

impl GenericDockerBlueprint {
    pub fn new() -> Self {
        Self {
            definition: Blueprint {
                id: "generic-docker".to_string(),
                name: "Generic Docker Container".to_string(),
                description: "Runs any Docker image, optionally overriding its command.".to_string(),
                schema_version: 1,
                blueprint_version: 1,
                supported_runtime_types: vec![RuntimeType::Docker],
                // Console included. It used to be left out because a
                // container created here was "never started with `-i`", and
                // a tab that could only ever show its read-only fallback is
                // worse than no tab. That stopped being true when
                // `build_create_args` began passing `-i` unconditionally -
                // every container this runtime creates keeps stdin open, and
                // `attach_console_fifo` wires it up on start.
                //
                // It matters most for a server somebody adopted rather than
                // created: a Minecraft server with no console is one nobody
                // can type `stop` into.
                features: vec![
                    BlueprintFeature::Console,
                    BlueprintFeature::Logs,
                    BlueprintFeature::Environment,
                    BlueprintFeature::Ports,
                    BlueprintFeature::HealthCheck,
                    BlueprintFeature::Files,
                ],
                fields: vec![
                    BlueprintField {
                        key: "image".to_string(),
                        label: "Image".to_string(),
                        field_type: BlueprintFieldType::Text,
                        required: true,
                        default_value: None,
                        help_text: Some("A Docker image reference, e.g. nginx:latest or itzg/minecraft-server:latest".to_string()),
                    },
                    BlueprintField {
                        key: "command".to_string(),
                        label: "Command override".to_string(),
                        field_type: BlueprintFieldType::TextList,
                        required: false,
                        default_value: Some(serde_json::json!([])),
                        help_text: Some("Overrides the image's own ENTRYPOINT/CMD, one argument per entry. Leave empty to run the image as authored.".to_string()),
                    },
                ],
                known_files: vec![],
                default_ports: vec![],
                connects_to: None,
                command_console: None,
                is_builtin: true,
            },
        }
    }
}

impl Default for GenericDockerBlueprint {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BlueprintHandler for GenericDockerBlueprint {
    fn blueprint(&self) -> &Blueprint {
        &self.definition
    }

    fn render_runtime_config(&self, inputs: &HashMap<String, serde_json::Value>) -> AppResult<serde_json::Value> {
        validate_inputs(&self.definition, inputs)?;
        let image = text_input(inputs, &self.definition, "image")?;
        let command = text_list_input(inputs, &self.definition, "command")?;
        // `memoryLimitMb`/`cpuLimitCores` are deliberately absent here -
        // set post-creation through `services::set_application_resource_limits`,
        // not the wizard (see `runtime::docker::DockerConfig`'s own doc
        // comment).
        Ok(serde_json::json!({ "image": image, "command": command }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_image_and_command_straight_through() {
        let blueprint = GenericDockerBlueprint::new();
        let mut inputs = HashMap::new();
        inputs.insert("image".to_string(), serde_json::json!("nginx:latest"));
        inputs.insert("command".to_string(), serde_json::json!(["nginx", "-g", "daemon off;"]));

        let config = blueprint.render_runtime_config(&inputs).unwrap();

        assert_eq!(config, serde_json::json!({ "image": "nginx:latest", "command": ["nginx", "-g", "daemon off;"] }));
    }

    #[test]
    fn command_defaults_to_an_empty_list_when_omitted() {
        let blueprint = GenericDockerBlueprint::new();
        let mut inputs = HashMap::new();
        inputs.insert("image".to_string(), serde_json::json!("alpine:latest"));

        let config = blueprint.render_runtime_config(&inputs).unwrap();

        assert_eq!(config, serde_json::json!({ "image": "alpine:latest", "command": [] }));
    }

    #[test]
    fn rejects_a_missing_image() {
        let blueprint = GenericDockerBlueprint::new();
        assert!(blueprint.render_runtime_config(&HashMap::new()).is_err());
    }

    #[test]
    fn only_supports_the_docker_runtime_type() {
        let blueprint = GenericDockerBlueprint::new();
        assert_eq!(blueprint.blueprint().supported_runtime_types, vec![RuntimeType::Docker]);
    }
}
