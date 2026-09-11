use std::collections::HashMap;

use crate::errors::AppResult;
use crate::models::{Blueprint, BlueprintFeature, BlueprintField, BlueprintFieldType, RuntimeType};

use super::{text_input, text_list_input, validate_inputs, BlueprintHandler};
// The one shared implementation - every module that builds a remote
// command used to carry its own byte-identical copy of this.
use crate::ssh::command::quote as shell_quote;

/// Runs a Node.js script/bot from the Application's own working directory.
/// There's no `provision()` step that runs `npm install` ahead of time -
/// unlike `PaperBlueprint`'s jar download, that would need Node itself
/// installed wherever `provision()` runs (the desktop, or the Node's own
/// host directly), neither of which is guaranteed. Instead the rendered
/// command installs dependencies *inside* the container, right before
/// starting, every time the container starts: a `package.json` presence
/// check keeps a dependency-free script from paying an `npm install` for
/// nothing, and paying that cost on every start (not just the first) is the
/// price of not needing a separate build/provisioning step at all - the
/// user only ever has to upload code, never remember to also trigger an
/// install.
pub struct NodejsBotBlueprint {
    definition: Blueprint,
}

impl NodejsBotBlueprint {
    pub fn new() -> Self {
        Self {
            definition: Blueprint {
                id: "nodejs-bot".to_string(),
                name: "Node.js Bot".to_string(),
                description: "Runs a Node.js script or bot - installs npm dependencies from package.json (if present) before every start.".to_string(),
                schema_version: 1,
                blueprint_version: 1,
                supported_runtime_types: vec![RuntimeType::Docker],
                features: vec![BlueprintFeature::Logs, BlueprintFeature::Environment, BlueprintFeature::Ports, BlueprintFeature::HealthCheck, BlueprintFeature::Files],
                fields: vec![
                    BlueprintField {
                        key: "entryFile".to_string(),
                        label: "Entry file".to_string(),
                        field_type: BlueprintFieldType::Path,
                        required: true,
                        default_value: None,
                        help_text: Some("Path to the script, relative to the working directory, e.g. index.js or src/bot.js.".to_string()),
                    },
                    BlueprintField {
                        key: "nodeVersion".to_string(),
                        label: "Node.js version".to_string(),
                        field_type: BlueprintFieldType::Text,
                        required: false,
                        default_value: Some(serde_json::Value::String("22".to_string())),
                        help_text: Some("A Docker Hub tag, e.g. 22, 20, or 18.".to_string()),
                    },
                    BlueprintField {
                        key: "programArgs".to_string(),
                        label: "Program arguments".to_string(),
                        field_type: BlueprintFieldType::TextList,
                        required: false,
                        default_value: Some(serde_json::json!([])),
                        help_text: Some("Arguments passed to the script itself.".to_string()),
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

impl Default for NodejsBotBlueprint {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BlueprintHandler for NodejsBotBlueprint {
    fn blueprint(&self) -> &Blueprint {
        &self.definition
    }

    fn render_runtime_config(&self, inputs: &HashMap<String, serde_json::Value>) -> AppResult<serde_json::Value> {
        validate_inputs(&self.definition, inputs)?;
        let entry_file = text_input(inputs, &self.definition, "entryFile")?;
        let node_version = text_input(inputs, &self.definition, "nodeVersion")?;
        let program_args = text_list_input(inputs, &self.definition, "programArgs")?;

        let mut script = format!("if [ -f package.json ]; then npm install --omit=dev; fi; exec node {}", shell_quote(&entry_file));
        for arg in &program_args {
            script.push(' ');
            script.push_str(&shell_quote(arg));
        }

        let image = format!("node:{}-alpine", node_version.trim());
        Ok(serde_json::json!({ "image": image, "command": ["sh", "-c", script] }))
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(entry_file: &str) -> HashMap<String, serde_json::Value> {
        let mut inputs = HashMap::new();
        inputs.insert("entryFile".to_string(), serde_json::json!(entry_file));
        inputs
    }

    #[test]
    fn renders_a_conditional_install_then_exec_node_with_the_entry_file() {
        let blueprint = NodejsBotBlueprint::new();
        let config = blueprint.render_runtime_config(&inputs("index.js")).unwrap();

        assert_eq!(config["image"], serde_json::json!("node:22-alpine"));
        assert_eq!(
            config["command"],
            serde_json::json!(["sh", "-c", "if [ -f package.json ]; then npm install --omit=dev; fi; exec node 'index.js'"])
        );
    }

    #[test]
    fn program_args_are_appended_after_the_entry_file() {
        let blueprint = NodejsBotBlueprint::new();
        let mut input_map = inputs("src/bot.js");
        input_map.insert("programArgs".to_string(), serde_json::json!(["--verbose"]));

        let config = blueprint.render_runtime_config(&input_map).unwrap();

        assert_eq!(
            config["command"],
            serde_json::json!(["sh", "-c", "if [ -f package.json ]; then npm install --omit=dev; fi; exec node 'src/bot.js' '--verbose'"])
        );
    }

    #[test]
    fn a_single_quote_in_the_entry_file_is_safely_escaped() {
        let blueprint = NodejsBotBlueprint::new();
        let config = blueprint.render_runtime_config(&inputs("it's/bot.js")).unwrap();
        let script = config["command"][2].as_str().unwrap();
        assert!(script.contains(r"'it'\''s/bot.js'"), "{script}");
    }

    #[test]
    fn a_custom_node_version_overrides_the_default_image_tag() {
        let blueprint = NodejsBotBlueprint::new();
        let mut input_map = inputs("index.js");
        input_map.insert("nodeVersion".to_string(), serde_json::json!("20"));
        let config = blueprint.render_runtime_config(&input_map).unwrap();
        assert_eq!(config["image"], serde_json::json!("node:20-alpine"));
    }

    #[test]
    fn rejects_a_missing_entry_file() {
        let blueprint = NodejsBotBlueprint::new();
        assert!(blueprint.render_runtime_config(&HashMap::new()).is_err());
    }
}
