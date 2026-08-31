use std::collections::HashMap;

use crate::errors::AppResult;
use crate::models::{Blueprint, BlueprintFeature, BlueprintField, BlueprintFieldType, RuntimeType};

use super::{text_input, text_list_input, validate_inputs, BlueprintHandler};

/// Runs any command, no framework-specific assumptions - the simplest
/// possible blueprint, and the fallback for anything that doesn't fit
/// `GenericJavaBlueprint` or a more specific blueprint not built yet
/// (Paper/Velocity, Phase 8/9).
pub struct GenericBlueprint {
    definition: Blueprint,
}

impl GenericBlueprint {
    pub fn new() -> Self {
        Self {
            definition: Blueprint {
                id: "generic".to_string(),
                name: "Generic Application".to_string(),
                description: "Runs any command you give it.".to_string(),
                schema_version: 1,
                blueprint_version: 1,
                // Not Docker: a raw command isn't a Docker image, and this
                // blueprint has no field for one - see
                // `blueprints::GenericDockerBlueprint` for that.
                supported_runtime_types: vec![RuntimeType::LocalProcess, RuntimeType::RemoteProcess, RuntimeType::Systemd],
                features: vec![BlueprintFeature::Console, BlueprintFeature::Logs, BlueprintFeature::Environment, BlueprintFeature::Ports, BlueprintFeature::HealthCheck, BlueprintFeature::Files],
                fields: vec![
                    BlueprintField {
                        key: "command".to_string(),
                        label: "Command".to_string(),
                        field_type: BlueprintFieldType::Path,
                        required: true,
                        default_value: None,
                        help_text: Some("The executable to run, e.g. /usr/bin/python3".to_string()),
                    },
                    BlueprintField {
                        key: "args".to_string(),
                        label: "Arguments".to_string(),
                        field_type: BlueprintFieldType::TextList,
                        required: false,
                        default_value: Some(serde_json::json!([])),
                        help_text: Some("Arguments passed to the command, in order.".to_string()),
                    },
                ],
                known_files: vec![],
                is_builtin: true,
            },
        }
    }
}

impl Default for GenericBlueprint {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BlueprintHandler for GenericBlueprint {
    fn blueprint(&self) -> &Blueprint {
        &self.definition
    }

    fn render_runtime_config(&self, inputs: &HashMap<String, serde_json::Value>) -> AppResult<serde_json::Value> {
        validate_inputs(&self.definition, inputs)?;
        let command = text_input(inputs, &self.definition, "command")?;
        let args = text_list_input(inputs, &self.definition, "args")?;
        Ok(serde_json::json!({ "command": command, "args": args }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_command_and_args_straight_through() {
        let blueprint = GenericBlueprint::new();
        let mut inputs = HashMap::new();
        inputs.insert("command".to_string(), serde_json::json!("/usr/bin/python3"));
        inputs.insert("args".to_string(), serde_json::json!(["-u", "app.py"]));

        let config = blueprint.render_runtime_config(&inputs).unwrap();

        assert_eq!(config, serde_json::json!({ "command": "/usr/bin/python3", "args": ["-u", "app.py"] }));
    }

    #[test]
    fn args_default_to_an_empty_list_when_omitted() {
        let blueprint = GenericBlueprint::new();
        let mut inputs = HashMap::new();
        inputs.insert("command".to_string(), serde_json::json!("/usr/bin/echo"));

        let config = blueprint.render_runtime_config(&inputs).unwrap();

        assert_eq!(config, serde_json::json!({ "command": "/usr/bin/echo", "args": [] }));
    }

    #[test]
    fn rejects_a_missing_command() {
        let blueprint = GenericBlueprint::new();
        assert!(blueprint.render_runtime_config(&HashMap::new()).is_err());
    }
}
