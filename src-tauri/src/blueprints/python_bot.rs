use std::collections::HashMap;

use crate::errors::AppResult;
use crate::models::{Blueprint, BlueprintFeature, BlueprintField, BlueprintFieldType, RuntimeType};

use super::{text_input, text_list_input, validate_inputs, BlueprintHandler};

/// Runs a Python script/bot from the Application's own working directory -
/// same "install dependencies inside the container on every start" design
/// as `NodejsBotBlueprint`'s own doc comment explains (identical reasoning
/// here, just `requirements.txt`/`pip` instead of `package.json`/`npm`).
/// `python:<version>-slim` rather than `-alpine`: Alpine's musl libc trips
/// up pip wheels for common packages with C extensions (a well-known, still-
/// current gotcha for the official Python image), so `-slim` (Debian-based)
/// is the safer default for a template whose whole point is "just works".
pub struct PythonBotBlueprint {
    definition: Blueprint,
}

impl PythonBotBlueprint {
    pub fn new() -> Self {
        Self {
            definition: Blueprint {
                id: "python-bot".to_string(),
                name: "Python Bot".to_string(),
                description: "Runs a Python script or bot - installs pip dependencies from requirements.txt (if present) before every start.".to_string(),
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
                        help_text: Some("Path to the script, relative to the working directory, e.g. bot.py or src/main.py.".to_string()),
                    },
                    BlueprintField {
                        key: "pythonVersion".to_string(),
                        label: "Python version".to_string(),
                        field_type: BlueprintFieldType::Text,
                        required: false,
                        default_value: Some(serde_json::Value::String("3.13".to_string())),
                        help_text: Some("A Docker Hub tag, e.g. 3.13, 3.12, or 3.11.".to_string()),
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
                is_builtin: true,
            },
        }
    }
}

impl Default for PythonBotBlueprint {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BlueprintHandler for PythonBotBlueprint {
    fn blueprint(&self) -> &Blueprint {
        &self.definition
    }

    fn render_runtime_config(&self, inputs: &HashMap<String, serde_json::Value>) -> AppResult<serde_json::Value> {
        validate_inputs(&self.definition, inputs)?;
        let entry_file = text_input(inputs, &self.definition, "entryFile")?;
        let python_version = text_input(inputs, &self.definition, "pythonVersion")?;
        let program_args = text_list_input(inputs, &self.definition, "programArgs")?;

        let mut script = format!("if [ -f requirements.txt ]; then pip install --no-cache-dir -r requirements.txt; fi; exec python {}", shell_quote(&entry_file));
        for arg in &program_args {
            script.push(' ');
            script.push_str(&shell_quote(arg));
        }

        let image = format!("python:{}-slim", python_version.trim());
        Ok(serde_json::json!({ "image": image, "command": ["sh", "-c", script] }))
    }
}

/// Same reasoning as `nodejs_bot::shell_quote` (identical here).
fn shell_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
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
    fn renders_a_conditional_install_then_exec_python_with_the_entry_file() {
        let blueprint = PythonBotBlueprint::new();
        let config = blueprint.render_runtime_config(&inputs("bot.py")).unwrap();

        assert_eq!(config["image"], serde_json::json!("python:3.13-slim"));
        assert_eq!(
            config["command"],
            serde_json::json!(["sh", "-c", "if [ -f requirements.txt ]; then pip install --no-cache-dir -r requirements.txt; fi; exec python 'bot.py'"])
        );
    }

    #[test]
    fn program_args_are_appended_after_the_entry_file() {
        let blueprint = PythonBotBlueprint::new();
        let mut input_map = inputs("src/main.py");
        input_map.insert("programArgs".to_string(), serde_json::json!(["--debug"]));

        let config = blueprint.render_runtime_config(&input_map).unwrap();

        assert_eq!(
            config["command"],
            serde_json::json!(["sh", "-c", "if [ -f requirements.txt ]; then pip install --no-cache-dir -r requirements.txt; fi; exec python 'src/main.py' '--debug'"])
        );
    }

    #[test]
    fn a_custom_python_version_overrides_the_default_image_tag() {
        let blueprint = PythonBotBlueprint::new();
        let mut input_map = inputs("bot.py");
        input_map.insert("pythonVersion".to_string(), serde_json::json!("3.11"));
        let config = blueprint.render_runtime_config(&input_map).unwrap();
        assert_eq!(config["image"], serde_json::json!("python:3.11-slim"));
    }

    #[test]
    fn rejects_a_missing_entry_file() {
        let blueprint = PythonBotBlueprint::new();
        assert!(blueprint.render_runtime_config(&HashMap::new()).is_err());
    }
}
